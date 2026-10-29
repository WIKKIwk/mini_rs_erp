//! Session-scoped, bounded list projections. Reads are driven by committed
//! invalidations, never a periodic list poll. The server diffs canonical windows
//! so inserts/removals correctly backfill the sorted limit/offset boundary.
use super::inventory_movements::{inventory_actor, inventory_asset_rows};
use super::*;
use crate::core::inventory_movements::{
    InventoryActor, InventoryAssetQuery, InventoryTransferQuery,
};
use crate::http::ws::{Message, WebSocket, WebSocketUpgrade};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const MAX_VIEWS: usize = 8;
const MAX_ROWS: usize = 500;
const MAX_MESSAGE_BYTES: usize = 32 * 1024;
static NEXT_CONNECTION: AtomicU64 = AtomicU64::new(1);

#[derive(Default, Deserialize)]
pub struct InventoryLiveQuery {
    #[serde(default)]
    token: String,
    /// Native/Iroh live transports are receive-only. URL subscriptions offer
    /// exactly the same bounded protocol as the optional subscribe message.
    #[serde(default)]
    subscriptions: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Subscription {
    id: String,
    kind: ViewKind,
    #[serde(default = "empty_query")]
    query: Value,
    #[serde(default)]
    version: String,
}
fn empty_query() -> Value {
    json!({})
}

#[derive(Clone, Copy, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ViewKind {
    Assets,
    Transfers,
    Locations,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubscribeMessage {
    #[serde(rename = "type")]
    kind: String,
    subscriptions: Vec<Subscription>,
}

#[derive(Debug, PartialEq, Eq)]
enum LiveError {
    Scope,
    Read,
    Transport,
}
impl From<AdminError> for LiveError {
    fn from(error: AdminError) -> Self {
        if matches!(error.0, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            Self::Scope
        } else {
            Self::Read
        }
    }
}

struct View {
    subscription: Subscription,
    rows: Vec<Value>,
    revision: u64,
    version: String,
}

pub async fn inventory_live(
    State(state): State<AppState>,
    Query(query): Query<InventoryLiveQuery>,
    mut headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, AdminError> {
    if !query.token.trim().is_empty() {
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", query.token.trim())
                .parse()
                .map_err(|_| unauthorized())?,
        );
    }
    let _ = inventory_actor(&state, &headers).await?;
    if query.subscriptions.len() > MAX_MESSAGE_BYTES {
        return Err(bad_request("inventory_live_subscriptions_invalid"));
    }
    let subscriptions = if query.subscriptions.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str::<Vec<Subscription>>(&query.subscriptions)
            .map_err(|_| bad_request("inventory_live_subscriptions_invalid"))?
    };
    validate_subscriptions(&subscriptions)
        .map_err(|_| bad_request("inventory_live_subscriptions_invalid"))?;
    Ok(ws
        .on_upgrade(move |socket| live_socket(state, socket, headers, subscriptions))
        .into_response())
}

fn validate_subscriptions(subscriptions: &[Subscription]) -> Result<(), ()> {
    if subscriptions.len() > MAX_VIEWS {
        return Err(());
    }
    let mut ids = BTreeSet::new();
    for subscription in subscriptions {
        if subscription.id.trim().is_empty()
            || subscription.id.len() > 128
            || !ids.insert(&subscription.id)
            || subscription.version.len() > 128
        {
            return Err(());
        }
        match subscription.kind {
            ViewKind::Assets => {
                let query =
                    serde_json::from_value::<InventoryAssetQuery>(subscription.query.clone())
                        .map_err(|_| ())?;
                if query.limit == 0
                    || query.limit > MAX_ROWS
                    || query.offset > 100_000
                    || query.query.len() > 256
                    || query.warehouse_id.len() > 256
                {
                    return Err(());
                }
            }
            ViewKind::Transfers => {
                let query =
                    serde_json::from_value::<InventoryTransferQuery>(subscription.query.clone())
                        .map_err(|_| ())?;
                if query.limit == 0
                    || query.limit > MAX_ROWS
                    || query.offset > 100_000
                    || query.direction.len() > 32
                    || query.status.len() > 32
                {
                    return Err(());
                }
            }
            ViewKind::Locations => {
                if subscription
                    .query
                    .as_object()
                    .is_none_or(|query| !query.is_empty())
                {
                    return Err(());
                }
            }
        }
    }
    Ok(())
}

async fn scope(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(InventoryActor, Value), AdminError> {
    tokio::time::timeout(Duration::from_secs(10), read_scope(state, headers))
        .await
        .map_err(|_| server_error("inventory live authorization timed out"))?
}

async fn read_scope(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(InventoryActor, Value), AdminError> {
    let actor = inventory_actor(state, headers).await?;
    let mut capabilities = state
        .admin
        .principal_capability_codes(&actor.principal)
        .await;
    let mut apparatus = state
        .admin
        .principal_assigned_apparatus(&actor.principal)
        .await;
    let mut item_groups = state
        .admin
        .principal_assigned_item_group_scope(&actor.principal)
        .await
        .map_err(|_| server_error("inventory scope unavailable"))?;
    capabilities.sort();
    capabilities.dedup();
    apparatus.sort();
    apparatus.dedup();
    item_groups.sort();
    item_groups.dedup();
    let value = json!({
        "role": actor.principal.role, "ref": actor.principal.ref_,
        "admin": actor.is_admin, "warehouses": actor.assigned_warehouses,
        "capabilities": capabilities, "apparatus": apparatus, "item_groups": item_groups,
    });
    Ok((actor, value))
}

async fn rows(
    state: &AppState,
    actor: &InventoryActor,
    subscription: &Subscription,
) -> Result<Vec<Value>, AdminError> {
    tokio::time::timeout(
        Duration::from_secs(10),
        read_rows(state, actor, subscription),
    )
    .await
    .map_err(|_| server_error("inventory live read timed out"))?
}

async fn read_rows(
    state: &AppState,
    actor: &InventoryActor,
    subscription: &Subscription,
) -> Result<Vec<Value>, AdminError> {
    let value = match subscription.kind {
        ViewKind::Assets => serde_json::to_value(
            inventory_asset_rows(
                state,
                actor,
                serde_json::from_value(subscription.query.clone())
                    .map_err(|_| bad_request("inventory_live_query_invalid"))?,
            )
            .await?,
        )
        .map_err(|_| server_error("inventory live serialization failed"))?,
        ViewKind::Transfers => serde_json::to_value(
            state
                .inventory_movements
                .transfers(
                    actor,
                    serde_json::from_value(subscription.query.clone())
                        .map_err(|_| bad_request("inventory_live_query_invalid"))?,
                )
                .await
                .map_err(|_| server_error("inventory live read failed"))?,
        )
        .map_err(|_| server_error("inventory live serialization failed"))?,
        ViewKind::Locations => {
            let mut locations = state
                .inventory_movements
                .locations()
                .await
                .map_err(|_| server_error("inventory live read failed"))?;
            if actor.principal.role == PrincipalRole::TayyorlovMasteri {
                locations.retain(|location| location.kind == crate::core::inventory_movements::InventoryLocationKind::Warehouse);
            }
            serde_json::to_value(locations)
                .map_err(|_| server_error("inventory live serialization failed"))?
        },
    };
    value
        .as_array()
        .cloned()
        .ok_or_else(|| server_error("inventory live rows invalid"))
}

fn version(scope: &Value, subscription: &Subscription, rows: &[Value]) -> String {
    let payload = json!([scope, subscription.kind, subscription.query, rows]);
    let hash = Sha256::digest(serde_json::to_vec(&payload).expect("JSON values always serialize"));
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn key(kind: ViewKind, row: &Value) -> String {
    match kind {
        ViewKind::Assets => format!(
            "{}:{}",
            row["kind"].as_str().unwrap_or_default(),
            row["asset_ref"].as_str().unwrap_or_default()
        ),
        _ => row["id"].as_str().unwrap_or_default().to_string(),
    }
}

/// The order vector includes unchanged row keys, never their payloads. This
/// avoids client-side sorting guesses and preserves capped window membership.
fn delta(kind: ViewKind, previous: &[Value], current: &[Value]) -> Option<Value> {
    if previous == current {
        return None;
    }
    let old = previous
        .iter()
        .map(|row| (key(kind, row), row))
        .collect::<BTreeMap<_, _>>();
    let new = current
        .iter()
        .map(|row| (key(kind, row), row))
        .collect::<BTreeMap<_, _>>();
    let upserts = current
        .iter()
        .filter(|row| old.get(&key(kind, row)).is_none_or(|old| **old != **row))
        .collect::<Vec<_>>();
    let removed = old
        .keys()
        .filter(|key| !new.contains_key(*key))
        .collect::<Vec<_>>();
    Some(
        json!({"upserts":upserts,"removed":removed,"order": current.iter().map(|row| key(kind, row)).collect::<Vec<_>>() }),
    )
}

async fn send(socket: &mut WebSocket, event: Value) -> bool {
    matches!(
        tokio::time::timeout(
            Duration::from_secs(15),
            socket.send(Message::Text(event.to_string().into()))
        )
        .await,
        Ok(Ok(()))
    )
}

async fn reset(socket: &mut WebSocket) {
    let _ = send(
        socket,
        json!({"event":"inventory.reset","message":"inventory_scope_changed"}),
    )
    .await;
}

async fn initial_views(
    state: &AppState,
    headers: &HeaderMap,
    socket: &mut WebSocket,
    epoch: &str,
    expected_scope: &Value,
    subscriptions: Vec<Subscription>,
) -> Result<Vec<View>, LiveError> {
    let (actor, current_scope) = scope(state, headers).await.map_err(LiveError::from)?;
    if &current_scope != expected_scope {
        return Err(LiveError::Scope);
    }
    let mut loaded = Vec::with_capacity(subscriptions.len());
    for subscription in subscriptions {
        let rows = rows(state, &actor, &subscription)
            .await
            .map_err(LiveError::from)?;
        let version = version(expected_scope, &subscription, &rows);
        loaded.push(View {
            subscription,
            rows,
            revision: 0,
            version,
        });
    }
    // A permissions edit racing a database read must not publish old scope.
    if scope(state, headers).await.map_err(LiveError::from)?.1 != *expected_scope {
        return Err(LiveError::Scope);
    }
    for view in &loaded {
        let mut event = json!({"event":"inventory.ready", "id":view.subscription.id,
            "epoch":epoch,"revision":view.revision,"version":view.version});
        if view.version != view.subscription.version {
            event["event"] = json!("inventory.snapshot");
            event["rows"] = json!(view.rows);
        }
        if !send(socket, event).await {
            return Err(LiveError::Transport);
        }
    }
    Ok(loaded)
}

async fn refresh_views(
    state: &AppState,
    headers: &HeaderMap,
    socket: &mut WebSocket,
    epoch: &str,
    expected_scope: &Value,
    views: &mut [View],
) -> Result<(), LiveError> {
    let (actor, current_scope) = scope(state, headers).await.map_err(LiveError::from)?;
    if &current_scope != expected_scope {
        return Err(LiveError::Scope);
    }
    let mut loaded = Vec::with_capacity(views.len());
    for view in views.iter() {
        loaded.push(
            rows(state, &actor, &view.subscription)
                .await
                .map_err(LiveError::from)?,
        );
    }
    if scope(state, headers).await.map_err(LiveError::from)?.1 != *expected_scope {
        return Err(LiveError::Scope);
    }
    for (view, rows) in views.iter_mut().zip(loaded) {
        if let Some(mut event) = delta(view.subscription.kind, &view.rows, &rows) {
            let next_version = version(expected_scope, &view.subscription, &rows);
            event["event"] = json!("inventory.delta");
            event["id"] = json!(view.subscription.id);
            event["epoch"] = json!(epoch);
            event["base_revision"] = json!(view.revision);
            event["revision"] = json!(view.revision + 1);
            event["version"] = json!(next_version);
            if !send(socket, event).await {
                return Err(LiveError::Transport);
            }
            view.revision += 1;
            view.version = next_version;
            view.rows = rows;
        }
    }
    Ok(())
}

async fn fail(socket: &mut WebSocket, error: LiveError) {
    if error == LiveError::Scope {
        reset(socket).await;
    } else if error == LiveError::Read {
        let _ = send(
            socket,
            json!({"event":"inventory.error","message":"inventory_live_read_failed"}),
        )
        .await;
    }
    let _ = tokio::time::timeout(Duration::from_secs(15), socket.send(Message::Close(None))).await;
}

async fn live_socket(
    state: AppState,
    mut socket: WebSocket,
    headers: HeaderMap,
    subscriptions: Vec<Subscription>,
) {
    // Subscribe before the baseline read: commits racing that read remain queued.
    let mut changes = state.warehouse_events.subscribe_inventory();
    let epoch = format!(
        "{}:{}:{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed)
    );
    let expected_scope = match scope(&state, &headers).await {
        Ok((_, value)) => value,
        Err(error) => {
            fail(&mut socket, error.into()).await;
            return;
        }
    };
    let mut views = match initial_views(
        &state,
        &headers,
        &mut socket,
        &epoch,
        &expected_scope,
        subscriptions,
    )
    .await
    {
        Ok(views) => views,
        Err(error) => {
            fail(&mut socket, error).await;
            return;
        }
    };
    let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
    heartbeat.tick().await;
    loop {
        tokio::select! {
            received = changes.recv() => {
                if matches!(received, Err(tokio::sync::broadcast::error::RecvError::Closed)) { break }
                tokio::time::sleep(Duration::from_millis(75)).await;
                // A lag is repaired by re-reading this bounded canonical window,
                // not by trusting incomplete notifications or broadcasting rows.
                for _ in 0..128 { if changes.try_recv().is_err() { break } }
                if let Err(error) = refresh_views(&state, &headers, &mut socket, &epoch, &expected_scope, &mut views).await {
                    fail(&mut socket, error).await; break
                }
            }
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Text(text))) if text.len() <= MAX_MESSAGE_BYTES => {
                        let message = serde_json::from_str::<SubscribeMessage>(&text);
                        let Ok(message) = message else { break };
                        if message.kind != "subscribe" || validate_subscriptions(&message.subscriptions).is_err() { break }
                        match initial_views(&state, &headers, &mut socket, &epoch, &expected_scope, message.subscriptions).await {
                            Ok(loaded) => views = loaded,
                            Err(error) => { fail(&mut socket, error).await; break }
                        }
                    }
                    Some(Ok(Message::Ping(bytes))) => if socket.send(Message::Pong(bytes)).await.is_err() { break },
                    Some(Ok(Message::Pong(_))) => {},
                    _ => break,
                }
            }
            _ = heartbeat.tick() => {
                match scope(&state, &headers).await {
                    Ok((_, value)) if value == expected_scope => {},
                    Ok(_) => { fail(&mut socket, LiveError::Scope).await; break },
                    Err(error) => { fail(&mut socket, error.into()).await; break },
                }
                if !send(&mut socket, json!({"event":"inventory.heartbeat","epoch":epoch})).await { break }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn asset(id: &str, name: &str) -> Value {
        json!({"kind":"raw_material","asset_ref":id,"item_name":name,"order_assignment":null})
    }
    #[test]
    fn inventory_live_delta_replaces_only_changed_rows_and_backfills_window() {
        let previous = vec![asset("a", "A"), asset("b", "B")];
        let current = vec![asset("b", "B edited"), asset("c", "C")];
        let change = delta(ViewKind::Assets, &previous, &current).unwrap();
        assert_eq!(change["removed"], json!(["raw_material:a"]));
        assert_eq!(change["upserts"], json!([current[0], current[1]]));
        assert_eq!(change["order"], json!(["raw_material:b", "raw_material:c"]));
    }
    #[test]
    fn inventory_live_delta_ignores_unchanged_and_supports_reorder_and_badges() {
        let original = vec![asset("a", "A"), asset("b", "B")];
        assert!(delta(ViewKind::Assets, &original, &original).is_none());
        let reordered = vec![original[1].clone(), original[0].clone()];
        let change = delta(ViewKind::Assets, &original, &reordered).unwrap();
        assert_eq!(change["upserts"], json!([]));
        assert_eq!(change["order"], json!(["raw_material:b", "raw_material:a"]));
        let mut updated = original.clone();
        updated[0]["order_assignment"] =
            json!({"order_label":"Customer order","order_control":"frozen"});
        assert_eq!(
            delta(ViewKind::Assets, &original, &updated).unwrap()["upserts"],
            json!([updated[0]])
        );
    }
    #[test]
    fn inventory_live_bounds_and_fingerprints_are_scope_and_query_sensitive() {
        let mut subscription: Subscription =
            serde_json::from_value(json!({"id":"a","kind":"assets","query":{"limit":100}}))
                .unwrap();
        assert!(validate_subscriptions(&[subscription.clone()]).is_ok());
        assert!(validate_subscriptions(&[subscription.clone(), subscription.clone()]).is_err());
        let first = version(&json!({"warehouse":"one"}), &subscription, &[]);
        assert_ne!(
            first,
            version(&json!({"warehouse":"two"}), &subscription, &[])
        );
        subscription.query["warehouse_id"] = json!("new");
        assert_ne!(
            first,
            version(&json!({"warehouse":"one"}), &subscription, &[])
        );
        subscription.query["limit"] = json!(501);
        assert!(validate_subscriptions(&[subscription]).is_err());
    }
}
