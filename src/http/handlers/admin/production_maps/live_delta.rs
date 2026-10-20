use super::*;
use crate::core::production_map::{
    CompletedQueueOrder, CompletionRequestDecisionNotification, CompletionRequestNotification,
    ProductionMapLiveEvent, ProductionMapLiveSnapshot,
};
use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::time::{Duration, timeout};

// A slow client gets a compressed HTTP resync, never an unbounded WS frame.
const MAX_STATE_DELTA_BYTES: usize = 64 * 1024;

pub(super) struct LiveView {
    pub snapshot: Arc<ProductionMapLiveSnapshot>,
    pub revision: u64,
    pub epoch: String,
    pub scope: String,
    pub customers: BTreeMap<String, String>,
    pub completed: Vec<CompletedQueueOrder>,
    pub requests: Vec<CompletionRequestNotification>,
    pub decisions: Vec<CompletionRequestDecisionNotification>,
}

fn colour_history_unchanged(before_revision: u64, revision: u64, event_revision: u64) -> bool {
    event_revision == revision && before_revision.checked_add(1) == Some(revision)
}

pub(super) async fn read_view(
    state: &AppState,
    principal: &Principal,
    include_requests: bool,
    worker_scope: bool,
    reuse_history: Option<(&LiveView, u64)>,
    window: &super::worker_snapshot::WorkerQueueWindow,
) -> Result<LiveView, AdminError> {
    let service = &state.production_maps;
    loop {
        let started_revision = service.snapshot_revision();
        let actor = queue_action_actor(principal);
        // Reuse history only for an exact colour-only revision. Retry when a
        // concurrent commit moves the revision while history/snapshot is read.
        let reusable = reuse_history.filter(|(before, expected)| {
            colour_history_unchanged(before.revision, started_revision, *expected)
        });
        let (completed, requests, decisions) = if let Some((before, _)) = reusable {
            (
                before.completed.clone(),
                before.requests.clone(),
                before.decisions.clone(),
            )
        } else {
            tokio::try_join!(
                service.completed_queue_orders_for_actor(&actor.ref_, window.history_limit()),
                async {
                    if include_requests {
                        service.completion_requests(200).await
                    } else {
                        Ok(Vec::new())
                    }
                },
                service.completion_request_decisions_for_actor(&actor.ref_, 200),
            )
            .map_err(production_map_error)?
        };
        let (snapshot, revision, scope) = if worker_scope {
            super::worker_snapshot::read_for_principal(
                state,
                principal,
                completed.iter().map(|order| order.order_id.clone()).chain(
                    decisions
                        .iter()
                        .take(window.history_limit())
                        .map(|decision| decision.order_id.clone()),
                ),
                window,
            )
            .await?
        } else {
            let (snapshot, revision) = service
                .live_snapshot_shared_with_revision()
                .await
                .map_err(production_map_error)?;
            let snapshot = super::super::training::merge_worker_training_snapshot_shared(
                state, principal, snapshot,
            )
            .await
            .map_err(super::super::training::training_workspace_error)?;
            (snapshot, revision, String::new())
        };
        if revision != started_revision {
            continue;
        }
        let customers = production_map_order_customers(state, &snapshot.maps).await;
        if service.snapshot_revision() != revision {
            continue;
        }
        return Ok(LiveView {
            snapshot,
            revision,
            epoch: service.snapshot_epoch().to_string(),
            scope,
            customers,
            completed,
            requests,
            decisions,
        });
    }
}

// Values are compared before serialization. In particular, unchanged maps
// and compiled programs are never cloned/serialized into a colour update.
fn map_patch<T: PartialEq + Serialize>(
    before: &BTreeMap<String, T>,
    after: &BTreeMap<String, T>,
) -> Option<Value> {
    let upsert = after
        .iter()
        .filter(|(key, value)| before.get(*key) != Some(*value))
        .map(|(key, value)| (key.clone(), json!(value)))
        .collect::<Map<_, _>>();
    let remove = before
        .keys()
        .filter(|key| !after.contains_key(*key))
        .collect::<Vec<_>>();
    (!upsert.is_empty() || !remove.is_empty()).then(|| json!({"upsert": upsert, "remove": remove}))
}

fn nested_patch<T: PartialEq + Serialize>(
    before: &BTreeMap<String, BTreeMap<String, T>>,
    after: &BTreeMap<String, BTreeMap<String, T>>,
) -> Option<Value> {
    let empty = BTreeMap::new();
    let scopes = after
        .iter()
        .filter_map(|(scope, values)| {
            let patch = map_patch(before.get(scope).unwrap_or(&empty), values).or_else(|| {
                (!before.contains_key(scope)).then(|| json!({"upsert": {}, "remove": []}))
            });
            patch.map(|patch| (scope.clone(), patch))
        })
        .collect::<Map<_, _>>();
    let remove = before
        .keys()
        .filter(|scope| !after.contains_key(*scope))
        .collect::<Vec<_>>();
    (!scopes.is_empty() || !remove.is_empty()).then(|| json!({"scopes": scopes, "remove": remove}))
}

pub(super) fn snapshot_patch(
    old: &ProductionMapLiveSnapshot,
    new: &ProductionMapLiveSnapshot,
) -> Map<String, Value> {
    let mut patch = Map::new();
    macro_rules! field {
        ($field:ident) => {
            if let Some(value) = map_patch(&old.$field, &new.$field) {
                patch.insert(stringify!($field).into(), value);
            }
        };
    }
    macro_rules! nested {
        ($field:ident) => {
            if let Some(value) = nested_patch(&old.$field, &new.$field) {
                patch.insert(stringify!($field).into(), value);
            }
        };
    }
    field!(sequences);
    field!(sequence_versions);
    field!(sequence_revisions);
    field!(visible_order_ids);
    field!(order_statuses);
    field!(order_controls);
    field!(frozen_orders_by_apparatus);
    nested!(queue_states);
    nested!(stage_states);
    nested!(queue_action_controls);
    let old_maps = old
        .maps
        .iter()
        .map(|saved| (saved.map.id.clone(), saved))
        .collect();
    let new_maps = new
        .maps
        .iter()
        .map(|saved| (saved.map.id.clone(), saved))
        .collect();
    if let Some(value) = map_patch(&old_maps, &new_maps) {
        patch.insert("maps".into(), value);
    }
    if old
        .maps
        .iter()
        .map(|saved| &saved.map.id)
        .ne(new.maps.iter().map(|saved| &saved.map.id))
    {
        patch.insert(
            "map_order".into(),
            json!(
                new.maps
                    .iter()
                    .map(|saved| &saved.map.id)
                    .collect::<Vec<_>>()
            ),
        );
    }
    let old_policies = old
        .queue_policies
        .iter()
        .map(|p| (p.apparatus_id.to_string(), p))
        .collect();
    let new_policies = new
        .queue_policies
        .iter()
        .map(|p| (p.apparatus_id.to_string(), p))
        .collect();
    if let Some(value) = map_patch(&old_policies, &new_policies) {
        patch.insert("queue_policies".into(), value);
    }
    patch
}

fn personal_patch(before: &LiveView, after: &LiveView) -> Map<String, Value> {
    let mut patch = Map::new();
    if let Some(value) = map_patch(&before.customers, &after.customers) {
        patch.insert("order_customers".into(), value);
    }
    if before.completed != after.completed {
        patch.insert("completed_orders".into(), json!(after.completed));
    }
    if before.requests != after.requests {
        patch.insert("completion_requests".into(), json!(after.requests));
    }
    if before.decisions != after.decisions {
        patch.insert(
            "completion_request_decisions".into(),
            json!(after.decisions),
        );
    }
    patch
}

#[cfg(test)]
fn view_patch(before: &LiveView, after: &LiveView) -> Map<String, Value> {
    let mut patch = snapshot_patch(&before.snapshot, &after.snapshot);
    patch.extend(personal_patch(before, after));
    patch
}

async fn send(socket: &mut WebSocket, value: Value) -> bool {
    match serde_json::to_string(&value) {
        Ok(json) => send_text(socket, json).await,
        Err(_) => false,
    }
}

async fn send_text(socket: &mut WebSocket, json: String) -> bool {
    timeout(
        Duration::from_secs(15),
        socket.send(Message::Text(json.into())),
    )
    .await
    .is_ok_and(|result| result.is_ok())
}

async fn refresh_feed(
    state: &AppState,
    feed: &super::live_hub::StateFeed,
    principal: &Principal,
    include_requests: bool,
    worker_scope: bool,
    colour_revision: Option<u64>,
    window: &super::worker_snapshot::WorkerQueueWindow,
    force: bool,
) -> Result<(), AdminError> {
    // One async build per authorized actor/window/revision. Other sockets wait
    // for this same result rather than repeat database reads and serialization.
    let mut data = feed.data.lock().await;
    data.used = std::time::Instant::now();
    if !force
        && data
            .view
            .as_ref()
            .is_some_and(|view| view.revision == state.production_maps.snapshot_revision())
    {
        return Ok(());
    }
    let next = read_view(
        state,
        principal,
        include_requests,
        worker_scope,
        colour_revision.and_then(|revision| data.view.as_ref().map(|view| (view, revision))),
        window,
    )
    .await?;
    let patch = data.view.as_ref().and_then(|before| {
        if before.scope != next.scope || before.epoch != next.epoch {
            return None;
        }
        let common = state
            .production_state_live
            .snapshot_patch(&before.snapshot, &next.snapshot);
        let patch = super::live_hub::join_patch(&common, &personal_patch(before, &next));
        (patch.len() <= MAX_STATE_DELTA_BYTES.saturating_sub(512)).then_some(patch)
    });
    data.replace(next, patch);
    Ok(())
}

struct Delivery {
    frames: Vec<Utf8Bytes>,
    epoch: String,
    scope: String,
    revision: u64,
    resync: bool,
}

async fn delivery(
    feed: &super::live_hub::StateFeed,
    client_epoch: &str,
    client_revision: Option<u64>,
    client_scope: &str,
    checkpoint: bool,
) -> Delivery {
    let mut data = feed.data.lock().await;
    data.used = std::time::Instant::now();
    let view = data.view.as_ref().expect("initialized feed");
    let epoch = view.epoch.clone();
    let scope = view.scope.clone();
    let revision = view.revision;
    let history = if client_epoch == epoch && client_scope == scope {
        client_revision.and_then(|from| data.history.recover(from, revision))
    } else {
        None
    };
    let Some(history) = history else {
        return Delivery {
            frames: Vec::new(),
            epoch,
            scope,
            revision,
            resync: true,
        };
    };
    let mut cursor = client_revision.expect("recovery requires cursor");
    let mut frames = Vec::new();
    for delta in &history {
        if delta.empty {
            continue;
        }
        frames.push(delta.frame(cursor));
        cursor = delta.revision;
    }
    if checkpoint && cursor != revision {
        // A heartbeat or successful catch-up advances proven empty revisions
        // without rebuilding rows. During normal live traffic no-op events are
        // silent, and the next material delta is rebased over this proven chain.
        let last = history.last().expect("non-empty revision gap");
        frames.push(last.frame(cursor));
        cursor = revision;
    }
    Delivery {
        frames,
        epoch,
        scope,
        revision: cursor,
        resync: false,
    }
}

async fn send_delivery(socket: &mut WebSocket, delivery: &Delivery, ready: bool) -> bool {
    for frame in &delivery.frames {
        if !timeout(
            Duration::from_secs(15),
            socket.send(Message::Text(frame.clone())),
        )
        .await
        .is_ok_and(|result| result.is_ok())
        {
            return false;
        }
    }
    if ready || delivery.resync {
        return send(
            socket,
            json!({
                "ok": true,
                "type": if ready { "state_ready" } else { "state_resync" },
                "epoch": delivery.epoch, "scope": delivery.scope, "rev": delivery.revision,
                "resync": delivery.resync, "recovered": !delivery.resync,
                "recovery_supported": true,
            }),
        )
        .await;
    }
    true
}

pub(super) fn production_map_state_live_socket(
    state: AppState,
    mut socket: WebSocket,
    principal: Principal,
    include_requests: bool,
    mut client_epoch: String,
    mut client_revision: Option<u64>,
    worker_scope: bool,
    mut client_scope: String,
    window: super::worker_snapshot::WorkerQueueWindow,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async move {
        // Subscribe before any read so the initial cursor cannot miss a commit.
        let mut events = state.production_maps.subscribe_live();
        let assignment_scope = if worker_scope {
            match super::worker_snapshot::scope_token(&state, &principal).await {
                Ok((_, scope)) => scope,
                Err(error) => {
                    let _ = send(&mut socket, json!(error.1.0)).await;
                    return;
                }
            }
        } else {
            String::new()
        };
        let key = serde_json::to_string(&(
            state.production_maps.snapshot_epoch(),
            principal.role,
            &principal.ref_,
            include_requests,
            worker_scope,
            window.scope(&assignment_scope),
        ))
        .expect("authorized feed key");
        let feed = state.production_state_live.feed(key);
        if let Err(error) = refresh_feed(
            &state,
            &feed,
            &principal,
            include_requests,
            worker_scope,
            None,
            &window,
            false,
        )
        .await
        {
            let _ = send(&mut socket, json!(error.1.0)).await;
            return;
        }
        let initial = delivery(&feed, &client_epoch, client_revision, &client_scope, true).await;
        if !send_delivery(&mut socket, &initial, true).await {
            return;
        }
        client_epoch = initial.epoch;
        client_scope = initial.scope;
        client_revision = Some(initial.revision);
        let mut heartbeat = tokio::time::interval(Duration::from_secs(25));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // The initial handshake already checked scope and advanced the cursor.
        heartbeat.tick().await;
        loop {
            tokio::select! {
                inbound = socket.recv() => match inbound {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                },
                received = events.recv() => {
                    let colour_revision = match received {
                        Ok(ProductionMapLiveEvent::PrintPreflight { revision, .. }) => Some(revision),
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => None,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    if let Err(error) = refresh_feed(&state, &feed, &principal, include_requests,
                        worker_scope, colour_revision, &window, false).await {
                        let _ = send(&mut socket, json!(error.1.0)).await; break;
                    }
                    let next = delivery(&feed, &client_epoch, client_revision, &client_scope, false).await;
                    if !send_delivery(&mut socket, &next, false).await { break; }
                    client_epoch = next.epoch;
                    client_scope = next.scope;
                    client_revision = Some(next.revision);
                },
                _ = heartbeat.tick() => {
                    if super::completion::require_any_live_capability(&state, &principal).await.is_err() {
                        let _ = send(&mut socket, json!({"ok": false, "error": "forbidden"})).await;
                        break;
                    }
                    let scope_changed = if worker_scope {
                        match super::worker_snapshot::scope_token(&state, &principal).await {
                            Ok((_, scope)) => window.scope(&scope) != client_scope,
                            Err(_) => { let _ = send(&mut socket, json!({"ok": false, "error": "forbidden"})).await; break; }
                        }
                    } else { false };
                    if let Err(error) = refresh_feed(&state, &feed, &principal, include_requests,
                        worker_scope, None, &window, scope_changed).await {
                        let _ = send(&mut socket, json!(error.1.0)).await; break;
                    }
                    let next = delivery(&feed, &client_epoch, client_revision, &client_scope, true).await;
                    if !send_delivery(&mut socket, &next, false).await { break; }
                    client_epoch = next.epoch;
                    client_scope = next.scope;
                    client_revision = Some(next.revision);
                    if !timeout(Duration::from_secs(15), socket.send(Message::Ping(Vec::new().into())))
                        .await.is_ok_and(|result| result.is_ok()) { break; }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay_view(revision: u64, scope: &str) -> LiveView {
        LiveView {
            snapshot: Arc::new(ProductionMapLiveSnapshot::default()),
            revision,
            epoch: "epoch".into(),
            scope: scope.into(),
            customers: BTreeMap::new(),
            completed: Vec::new(),
            requests: Vec::new(),
            decisions: Vec::new(),
        }
    }

    #[tokio::test]
    async fn live_noop_is_silent_but_next_material_delta_rebases_over_proven_steps() {
        let feed = super::super::live_hub::StateFeed::default();
        {
            let mut data = feed.data.lock().await;
            data.replace(replay_view(1, "scope"), None);
            data.replace(replay_view(2, "scope"), Some(Arc::from("{}")));
        }
        let quiet = delivery(&feed, "epoch", Some(1), "scope", false).await;
        assert!(!quiet.resync);
        assert!(quiet.frames.is_empty());
        assert_eq!(quiet.revision, 1);
        {
            let mut data = feed.data.lock().await;
            data.replace(
                replay_view(4, "scope"),
                Some(Arc::from(
                    r#"{"sequences":{"upsert":{"a":["order"]},"remove":[]}}"#,
                )),
            );
        }
        let recovered = delivery(&feed, "epoch", Some(1), "scope", true).await;
        assert!(!recovered.resync);
        assert_eq!(recovered.frames.len(), 1);
        let frame: Value = serde_json::from_str(recovered.frames[0].as_str()).unwrap();
        assert_eq!(frame["base_rev"], 1);
        assert_eq!(frame["rev"], 4);
        assert_eq!(frame["patch"]["sequences"]["upsert"]["a"], json!(["order"]));
        // No view was observed at revision 3, so an HTTP cursor there cannot
        // be falsely accepted as an equivalent predecessor.
        assert!(
            delivery(&feed, "epoch", Some(3), "scope", true)
                .await
                .resync
        );
    }

    #[tokio::test]
    async fn heartbeat_checkpoint_advances_noop_without_changing_any_fields() {
        let feed = super::super::live_hub::StateFeed::default();
        {
            let mut data = feed.data.lock().await;
            data.replace(replay_view(8, "scope"), None);
            data.replace(replay_view(11, "scope"), Some(Arc::from("{}")));
        }
        let checkpoint = delivery(&feed, "epoch", Some(8), "scope", true).await;
        assert!(!checkpoint.resync);
        assert_eq!(checkpoint.revision, 11);
        let frame: Value = serde_json::from_str(checkpoint.frames[0].as_str()).unwrap();
        assert_eq!(frame["base_rev"], 8);
        assert_eq!(frame["patch"], json!({}));
        assert!(delivery(&feed, "old", Some(8), "scope", true).await.resync);
        assert!(
            delivery(&feed, "epoch", Some(8), "other-worker", true)
                .await
                .resync
        );
    }

    #[tokio::test]
    async fn scope_change_and_oversized_delta_require_canonical_resync() {
        let feed = super::super::live_hub::StateFeed::default();
        {
            let mut data = feed.data.lock().await;
            data.replace(replay_view(1, "old-assignment"), None);
            data.replace(replay_view(2, "new-assignment"), Some(Arc::from("{}")));
        }
        assert!(
            delivery(&feed, "epoch", Some(1), "old-assignment", true)
                .await
                .resync
        );
        assert!(
            delivery(&feed, "epoch", Some(1), "new-assignment", true)
                .await
                .resync
        );
        assert!(
            !delivery(&feed, "epoch", Some(2), "new-assignment", true)
                .await
                .resync
        );
        {
            let mut data = feed.data.lock().await;
            // None is the bounded-frame fallback used for an oversized patch.
            data.replace(replay_view(3, "new-assignment"), None);
        }
        assert!(
            delivery(&feed, "epoch", Some(2), "new-assignment", true)
                .await
                .resync
        );
    }

    #[test]
    fn colour_state_patch_does_not_repeat_285_unchanged_maps() {
        use crate::core::production_map::{
            ProductionMapDefinition, ProductionMapSaved, compile_map,
        };
        let maps = (0..285)
            .map(|id| {
                let map: ProductionMapDefinition = serde_json::from_value(json!({
                    "id": format!("order-{id}"), "product_code": format!("PRODUCT-{id}"),
                    "title": "Production order",
                    "nodes": [{"id":"start", "kind":"start", "title":"Start"},
                        {"id":"work", "kind":"task", "title":"Work"},
                        {"id":"end", "kind":"end", "title":"End"}],
                    "edges": [{"from":"start", "to":"work"}, {"from":"work", "to":"end"}],
                }))
                .unwrap();
                let program = compile_map(&map).unwrap();
                ProductionMapSaved { map, program }
            })
            .collect();
        let snapshot = ProductionMapLiveSnapshot {
            maps,
            sequences: BTreeMap::new(),
            sequence_versions: BTreeMap::new(),
            sequence_revisions: BTreeMap::new(),
            visible_order_ids: BTreeMap::new(),
            queue_states: BTreeMap::new(),
            stage_states: BTreeMap::new(),
            queue_policies: vec![],
            queue_action_controls: BTreeMap::new(),
            order_statuses: BTreeMap::new(),
            order_controls: BTreeMap::new(),
            frozen_orders_by_apparatus: BTreeMap::new(),
        };
        let full_bytes = serde_json::to_vec(&snapshot).unwrap().len();
        let view = |snapshot, revision| LiveView {
            snapshot: Arc::new(snapshot),
            revision,
            epoch: "epoch".into(),
            scope: "scope".into(),
            customers: BTreeMap::new(),
            completed: vec![],
            requests: vec![],
            decisions: vec![],
        };
        let before = view(snapshot.clone(), 8);
        let mut changed = snapshot;
        changed.queue_states.insert(
            "apparatus".into(),
            BTreeMap::from([("order-0".into(), "print_preflight".into())]),
        );
        changed.stage_states.insert(
            "order-0".into(),
            BTreeMap::from([("print".into(), "print_preflight".into())]),
        );
        let after = view(changed, 9);
        let patch = view_patch(&before, &after);
        assert!(!patch.contains_key("maps"));
        assert!(!patch.contains_key("map_order"));
        let delta_bytes = serde_json::to_vec(&patch).unwrap().len();
        assert!(delta_bytes < 1024);
        assert!(full_bytes > delta_bytes * 100);
        eprintln!(
            "synthetic 285-map snapshot: {full_bytes} bytes; colour state patch: {delta_bytes} bytes"
        );
    }

    #[test]
    fn colour_history_reuse_rejects_coalesced_or_delayed_events() {
        assert!(colour_history_unchanged(10, 11, 11));
        assert!(!colour_history_unchanged(10, 12, 11));
        assert!(!colour_history_unchanged(10, 12, 12));
        assert!(!colour_history_unchanged(11, 12, 10));
    }

    #[test]
    fn unchanged_fields_do_not_allocate_wire_updates() {
        let values = BTreeMap::from([("order".to_string(), json!({"state": "pending"}))]);
        assert!(map_patch(&values, &values).is_none());
    }

    #[test]
    fn nested_state_patch_only_contains_changed_order_and_removals() {
        let before = BTreeMap::from([(
            "apparatus".into(),
            BTreeMap::from([
                ("changed".into(), "pending"),
                ("unchanged".into(), "pending"),
                ("deleted".into(), "pending"),
            ]),
        )]);
        let after = BTreeMap::from([(
            "apparatus".into(),
            BTreeMap::from([
                ("changed".into(), "print_preflight"),
                ("unchanged".into(), "pending"),
            ]),
        )]);
        assert_eq!(
            nested_patch(&before, &after),
            Some(json!({
                "scopes": {"apparatus": {"upsert": {"changed": "print_preflight"}, "remove": ["deleted"]}},
                "remove": [],
            }))
        );
    }

    #[test]
    fn deleted_apparatus_is_explicitly_removed() {
        let before = BTreeMap::from([(
            "apparatus".into(),
            BTreeMap::from([("order".into(), "pending")]),
        )]);
        assert_eq!(
            nested_patch(&before, &BTreeMap::new()),
            Some(json!({"scopes": {}, "remove": ["apparatus"]}))
        );
    }

    #[test]
    fn new_empty_scope_is_preserved() {
        let after = BTreeMap::from([("new".into(), BTreeMap::<String, String>::new())]);
        assert_eq!(
            nested_patch(&BTreeMap::new(), &after),
            Some(json!({
                "scopes": {"new": {"upsert": {}, "remove": []}}, "remove": [],
            }))
        );
    }
}
