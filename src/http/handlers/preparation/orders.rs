//! Preparation list/detail boundary. List rows are presentation data; order
//! actions continue to load and validate their existing canonical authority.
use super::*;
use crate::core::apparatus_standard::{QueueDiscipline, RuntimeApparatusConfiguration};
use crate::core::production_map::{
    OrderControlState, ProductionMapDefinition, ProductionMapLiveSnapshot, ProductionMapService,
};
use crate::http::handlers::admin::production_maps::order_page;
use crate::http::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::time::{Duration, timeout};

const PROJECTION: &str = "preparation_order_list_v1";
const READ_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Default, Deserialize)]
pub struct OrderListQuery {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    search: String,
    #[serde(default)]
    cursor: String,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    token: String,
    #[serde(default)]
    epoch: String,
    #[serde(default)]
    rev: Option<u64>,
    #[serde(default)]
    scope: String,
}

fn default_limit() -> usize {
    30
}

impl OrderListQuery {
    fn normalized(mut self) -> Result<Self, ApiError> {
        self.apparatus = self.apparatus.trim().to_string();
        self.search = self.search.trim().to_lowercase();
        if !(1..=50).contains(&self.limit)
            || self.search.chars().count() > 120
            || self.cursor.len() > 256
            || self.scope.len() > 256
            || self.epoch.len() > 256
            || (!self.apparatus.is_empty()
                && !crate::core::apparatus_standard::ApparatusId::is_valid(&self.apparatus))
        {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "preparation_orders_invalid",
            ));
        }
        Ok(self)
    }
}

fn api_error(status: StatusCode, code: &str) -> ApiError {
    (status, Json(json!({"ok":false,"error":code,"code":code})))
}

fn digest(value: impl Serialize) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&value).expect("order list serialization"))
    )
}

fn responsibilities(value: &Value) -> Vec<(String, String)> {
    let mut values = value
        .get("materials")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|v| {
            (
                v["material_id"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_lowercase(),
                v["material_name"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_lowercase(),
            )
        })
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

async fn authorize_orders(state: &AppState, headers: &HeaderMap) -> Result<Principal, ApiError> {
    let principal = authorize(state, headers).await?;
    if !state
        .admin
        .principal_is_active(&principal)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "preparation_orders_scope",
            )
        })?
    {
        return Err(api_error(StatusCode::FORBIDDEN, "forbidden"));
    }
    Ok(principal)
}

struct ReadScope {
    key: String,
    apparatus_id: String,
    apparatus: Value,
    selected: Option<RuntimeApparatusConfiguration>,
    responsibilities: Vec<(String, String)>,
}

async fn read_scope(
    state: &AppState,
    principal: &Principal,
    query: &OrderListQuery,
) -> Result<ReadScope, ApiError> {
    if !state
        .admin
        .principal_is_active(principal)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "preparation_orders_scope",
            )
        })?
        || !state
            .admin
            .principal_has_capability(principal, Capability::PreparationAccess)
            .await
    {
        return Err(api_error(StatusCode::FORBIDDEN, "forbidden"));
    }
    let assignments = store(state)?
        .list_responsibilities(&principal.ref_)
        .await
        .map_err(error)?;
    let assignments = responsibilities(&assignments);
    let mut configurations = state
        .apparatus
        .list_runtime_configurations()
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "preparation_orders_apparatus",
            )
        })?;
    configurations.retain(|item| item.is_active());
    let selected = configurations
        .iter()
        .find(|item| item.runtime.apparatus_id.as_str() == query.apparatus)
        .or_else(|| configurations.first())
        .cloned();
    let apparatus_id = selected
        .as_ref()
        .map(|c| c.runtime.apparatus_id.to_string())
        .unwrap_or_default();
    let apparatus = json!(
        configurations
            .iter()
            .map(|c| json!({
                "id":c.runtime.apparatus_id,"name":c.runtime.display.display_name,
            }))
            .collect::<Vec<_>>()
    );
    let picker_scope = configurations
        .iter()
        .map(|c| {
            (
                c.runtime.apparatus_id.as_str(),
                c.runtime.source_revision,
                c.runtime.display.display_name.as_str(),
                c.queue.discipline,
            )
        })
        .collect::<Vec<_>>();
    let key = digest((
        PROJECTION,
        &principal.role,
        &principal.ref_,
        &apparatus_id,
        &query.search,
        query.limit,
        &assignments,
        &picker_scope,
    ));
    Ok(ReadScope {
        key,
        apparatus_id,
        apparatus,
        selected,
        responsibilities: assignments,
    })
}

#[derive(Clone, Serialize)]
struct OrderRow {
    id: String,
    code: String,
    order_number: String,
    title: String,
    customer_name: String,
    image_id: String,
    position: usize,
}

struct CompactRows {
    rows: Vec<OrderRow>,
    version: String,
    built_at: Instant,
}

type RowCacheEntry = Arc<Mutex<Option<CompactRows>>>;
type RowCache = Mutex<BTreeMap<String, (Instant, RowCacheEntry)>>;
static ROW_CACHE: OnceLock<RowCache> = OnceLock::new();

async fn row_cache_entry(key: String) -> RowCacheEntry {
    let mut entries = ROW_CACHE.get_or_init(Default::default).lock().await;
    if let Some((touched, entry)) = entries.get_mut(&key) {
        *touched = Instant::now();
        return entry.clone();
    }
    if entries.len() >= 32 {
        let oldest = entries
            .iter()
            .filter(|(_, (_, entry))| Arc::strong_count(entry) == 1)
            .min_by_key(|(_, (touched, _))| *touched)
            .map(|(key, _)| key.clone());
        if let Some(key) = oldest {
            entries.remove(&key);
        }
    }
    let entry = Arc::new(Mutex::new(None));
    if entries.len() < 32 {
        entries.insert(key, (Instant::now(), entry.clone()));
    }
    entry
}

fn row(map: &ProductionMapDefinition, position: usize) -> OrderRow {
    let code = if !map.code.trim().is_empty() {
        map.code.trim()
    } else if !map.order_number.trim().is_empty() {
        map.order_number.trim()
    } else {
        map.id.strip_prefix("zakaz-").unwrap_or(&map.id)
    };
    let product = map
        .nodes
        .iter()
        .filter(|n| n.kind == crate::core::production_map::ProductionMapNodeKind::End)
        .map(|n| n.title.trim())
        .find(|v| !v.is_empty() && *v != map.title.trim())
        .unwrap_or("");
    let title = if map.title.trim().is_empty() {
        product
    } else {
        map.title.trim()
    };
    OrderRow {
        id: map.id.clone(),
        code: code.to_string(),
        order_number: map.order_number.clone(),
        title: title.to_string(),
        customer_name: map.customer_name.trim().to_string(),
        image_id: map.image_id.clone(),
        position,
    }
}

fn offset(cursor: &str, version: &str) -> Result<usize, ApiError> {
    if cursor.is_empty() {
        return Ok(0);
    }
    let parsed = URL_SAFE_NO_PAD
        .decode(cursor)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<(String, usize)>(&bytes).ok())
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "preparation_orders_cursor"))?;
    if parsed.0 != version {
        return Err(api_error(
            StatusCode::CONFLICT,
            "preparation_orders_changed",
        ));
    }
    Ok(parsed.1)
}

async fn read_list(
    state: &AppState,
    principal: &Principal,
    query: &OrderListQuery,
) -> Result<Value, ApiError> {
    let requested_at = Instant::now();
    for _ in 0..3 {
        let revision = state.production_maps.snapshot_revision();
        let scope = read_scope(state, principal, query).await?;
        let epoch = state.production_maps.snapshot_epoch();
        let entry = row_cache_entry(digest((&scope.key, epoch, revision))).await;
        // One build per exact actor/query/revision. Only compact rows survive
        // the build; topology and fallback canonical snapshots are not cached here.
        let mut cached = entry.lock().await;
        // A new first-page read is also an explicit refresh. Labels can change
        // independently of the queue revision. Concurrent first readers which
        // started before this build still share it; cursor pages reuse its rows.
        if query.cursor.is_empty() && cached.as_ref().is_some_and(|v| v.built_at < requested_at) {
            *cached = None;
        }
        if cached.is_none() {
            let catalog = store(state)?
                .order_list_catalog(&principal.ref_, &scope.apparatus_id)
                .await
                .map_err(error)?;
            if responsibilities(&catalog.responsibilities) != scope.responsibilities {
                continue;
            }
            let maps = catalog
                .candidates
                .iter()
                .map(|c| c.map.clone())
                .collect::<Vec<_>>();
            let mut ids = scope
                .selected
                .as_ref()
                .map(|selected| {
                    ProductionMapService::preparation_catalog_sequence(
                        &maps,
                        &catalog.sequence,
                        selected,
                        &BTreeSet::new(),
                    )
                })
                .unwrap_or_default();
            let affected = catalog
                .candidates
                .iter()
                .filter(|c| c.needs_authority && ids.contains(&c.map.id))
                .map(|c| c.map.id.clone())
                .collect::<Vec<_>>();
            let mut priority = BTreeMap::<String, (u8, i64)>::new();
            let mut hidden = BTreeSet::new();
            let mut frozen_sequence_ids = BTreeSet::new();
            // Only executed/cooperative orders expand into the existing authority.
            // Untouched rows never build controls, programs, histories or WIP.
            // The expansion is bounded to order IDs, never a global snapshot build.
            let mut changed = false;
            for chunk in affected.chunks(50) {
                let (snapshot, read_revision) = state
                    .production_maps
                    .order_snapshot_shared_with_revision(chunk)
                    .await
                    .map_err(|_| {
                        api_error(StatusCode::INTERNAL_SERVER_ERROR, "preparation_orders_read")
                    })?;
                if read_revision != revision {
                    changed = true;
                    break;
                }
                let visible = order_page::ordered_ids(&snapshot, &scope.apparatus_id)
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                for id in chunk {
                    let control = snapshot
                        .queue_action_controls
                        .get(&scope.apparatus_id)
                        .and_then(|v| v.get(id));
                    let raw = snapshot
                        .queue_states
                        .get(&scope.apparatus_id)
                        .and_then(|v| v.get(id))
                        .map(String::as_str)
                        .or_else(|| control.map(|c| c.state.as_str()))
                        .unwrap_or("pending");
                    let global = snapshot.order_controls.get(id);
                    let status = snapshot.order_statuses.get(id);
                    if !visible.contains(id) || !snapshot.maps.iter().any(|m| &m.map.id == id) {
                        hidden.insert(id.clone());
                    }
                    if raw == "frozen"
                        || global.is_some_and(|c| c.state == OrderControlState::Frozen)
                    {
                        frozen_sequence_ids.insert(id.clone());
                    }
                    let red = global.is_some_and(|c| c.state != OrderControlState::Active)
                        || status.is_some_and(|s| {
                            matches!(s.order_status.as_str(), "frozen" | "completed_with_issue")
                                || s.completed_with_issue_count > 0
                        })
                        || raw == "frozen";
                    let rank = if red {
                        3
                    } else if matches!(raw, "in_progress" | "print_preflight") {
                        0
                    } else if raw == "paused" {
                        1
                    } else {
                        2
                    };
                    priority.insert(
                        id.clone(),
                        (rank, control.map(|c| c.last_worked_at_unix).unwrap_or(0)),
                    );
                }
            }
            if changed || state.production_maps.snapshot_revision() != revision {
                continue;
            }
            if !frozen_sequence_ids.is_empty() {
                ids = scope
                    .selected
                    .as_ref()
                    .map(|selected| {
                        ProductionMapService::preparation_catalog_sequence(
                            &maps,
                            &catalog.sequence,
                            selected,
                            &frozen_sequence_ids,
                        )
                    })
                    .unwrap_or_default();
            }
            ids.retain(|id| !hidden.contains(id));
            if scope
                .selected
                .as_ref()
                .is_some_and(|c| c.queue.discipline == QueueDiscipline::FreePick)
            {
                ids.sort_by_key(|id| {
                    let (priority, last) = priority.get(id).copied().unwrap_or((2, 0));
                    (priority, std::cmp::Reverse(last))
                });
            }
            let by_id = catalog
                .candidates
                .iter()
                .map(|c| (c.map.id.as_str(), c))
                .collect::<BTreeMap<_, _>>();
            let rows = ids
                .iter()
                .enumerate()
                .filter_map(|(index, id)| {
                    let candidate = by_id.get(id.as_str())?;
                    if !candidate.allowed {
                        return None;
                    }
                    let row = row(&candidate.map, index + 1);
                    (query.search.is_empty()
                        || [
                            &row.id,
                            &row.code,
                            &row.order_number,
                            &row.title,
                            &row.customer_name,
                            &candidate.map.product_code,
                        ]
                        .into_iter()
                        .map(|s| s.to_lowercase())
                        .collect::<Vec<_>>()
                        .join(" ")
                        .contains(&query.search))
                    .then_some(row)
                })
                .collect::<Vec<_>>();
            let version = digest((&scope.key, epoch, revision, &rows));
            *cached = Some(CompactRows {
                rows,
                version,
                built_at: Instant::now(),
            });
        }
        let value = cached.as_ref().expect("compact rows populated");
        let rows = &value.rows;
        let version = &value.version;
        let start = offset(&query.cursor, version)?;
        if start > rows.len() {
            return Err(api_error(
                StatusCode::CONFLICT,
                "preparation_orders_changed",
            ));
        }
        let end = start.saturating_add(query.limit).min(rows.len());
        let next_cursor = (end < rows.len()).then(|| {
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&(&version, end)).expect("order list cursor"))
        });
        // Material responsibility changes have their own authority and need not
        // advance the production revision. Recheck before releasing the result.
        let latest = read_scope(state, principal, query).await?;
        if latest.key != scope.key || state.production_maps.snapshot_revision() != revision {
            continue;
        }
        return Ok(
            json!({"ok":true,"projection":PROJECTION,"scope":scope.key,"epoch":epoch,
            "rev":revision,"version":version,"apparatus_id":scope.apparatus_id,
            "apparatus":scope.apparatus,"rows":&rows[start..end],"total":rows.len(),
            "next_cursor":next_cursor}),
        );
    }
    Err(api_error(
        StatusCode::CONFLICT,
        "preparation_orders_changed",
    ))
}

pub async fn order_list(
    State(state): State<AppState>,
    Query(query): Query<OrderListQuery>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let query = query.normalized()?;
    let principal = authorize_orders(&state, &headers).await?;
    timeout(READ_TIMEOUT, read_list(&state, &principal, &query))
        .await
        .map_err(|_| api_error(StatusCode::CONFLICT, "preparation_orders_timeout"))?
        .map(Json)
}

pub async fn order_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let principal = authorize_orders(&state, &headers).await?;
    if id.len() > 256 || !id.starts_with("zakaz-") {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "preparation_order_invalid",
        ));
    }
    if !store(&state)?
        .order_list_detail_allowed(&principal.ref_, &id)
        .await
        .map_err(error)?
    {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "preparation_order_not_available",
        ));
    }
    let payload = timeout(READ_TIMEOUT, async {
        for _ in 0..3 {
            let revision = state.production_maps.snapshot_revision();
            let mut saved = state
                .production_maps
                .map(&id)
                .await
                .map_err(|_| {
                    api_error(StatusCode::INTERNAL_SERVER_ERROR, "preparation_order_read")
                })?
                .ok_or_else(|| {
                    api_error(StatusCode::NOT_FOUND, "preparation_order_not_available")
                })?;
            store(&state)?
                .fill_order_customer(&mut saved.map)
                .await
                .map_err(error)?;
            let templates =
                order_page::page_calculation_templates(&state, std::slice::from_ref(&saved)).await;
            if state.production_maps.snapshot_revision() != revision {
                continue;
            }
            let customers = json!({&id:&saved.map.customer_name});
            // The master sheet is a summary: execution authority, progress and
            // cross-machine controls are not visible and are not built here.
            let mut payload = serde_json::to_value(ProductionMapLiveSnapshot {
                maps: vec![saved],
                ..Default::default()
            })
            .expect("preparation order detail");
            payload["ok"] = true.into();
            payload["projection"] = "order_detail_v1".into();
            payload["order_id"] = id.clone().into();
            payload["apparatus"] = "".into();
            payload["epoch"] = state.production_maps.snapshot_epoch().into();
            payload["rev"] = revision.into();
            payload["scope"] = digest((
                &principal.role,
                &principal.ref_,
                "preparation_order_detail_v1",
                &id,
            ))
            .into();
            payload["order_customers"] = customers;
            payload["calculation_templates"] = json!(templates);
            return Ok(payload);
        }
        Err(api_error(
            StatusCode::CONFLICT,
            "preparation_orders_changed",
        ))
    })
    .await
    .map_err(|_| api_error(StatusCode::CONFLICT, "preparation_orders_timeout"))??;
    authorize_orders(&state, &headers).await?;
    if !store(&state)?
        .order_list_detail_allowed(&principal.ref_, &id)
        .await
        .map_err(error)?
    {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "preparation_order_not_available",
        ));
    }
    Ok(Json(payload).into_response())
}

pub async fn order_list_live(
    State(state): State<AppState>,
    Query(query): Query<OrderListQuery>,
    mut headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let query = query.normalized()?;
    let token = if query.token.trim().is_empty() {
        super::super::auth::bearer_token(&headers).unwrap_or_default()
    } else {
        query.token.clone()
    };
    headers.insert(
        axum::http::header::AUTHORIZATION,
        format!("Bearer {token}")
            .parse()
            .map_err(|_| api_error(StatusCode::UNAUTHORIZED, "unauthorized"))?,
    );
    let principal = authorize_orders(&state, &headers).await?;
    read_scope(&state, &principal, &query).await?;
    Ok(ws
        .on_upgrade(move |socket| socket_loop(state, socket, headers, principal, query))
        .into_response())
}

async fn send(socket: &mut WebSocket, value: Value) -> bool {
    timeout(
        READ_TIMEOUT,
        socket.send(Message::Text(value.to_string().into())),
    )
    .await
    .is_ok_and(|result| result.is_ok())
}

async fn socket_loop(
    state: AppState,
    mut socket: WebSocket,
    headers: HeaderMap,
    principal: Principal,
    query: OrderListQuery,
) {
    let mut events = state.production_maps.subscribe_live();
    let mut cursor = (
        query.epoch.clone(),
        query.rev.unwrap_or(u64::MAX),
        query.scope.clone(),
    );
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    loop {
        let current = match authorize_orders(&state, &headers).await {
            Ok(value) if value.role == principal.role && value.ref_ == principal.ref_ => value,
            Ok(_) => break,
            Err((status, Json(value))) => {
                let _ = send(
                    &mut socket,
                    json!({"type":"error","status":status.as_u16(),"error":value}),
                )
                .await;
                break;
            }
        };
        let scope = match read_scope(&state, &current, &query).await {
            Ok(value) => value,
            Err((status, Json(value))) => {
                let _ = send(
                    &mut socket,
                    json!({"type":"error","status":status.as_u16(),"error":value}),
                )
                .await;
                break;
            }
        };
        let next = (
            state.production_maps.snapshot_epoch().to_string(),
            state.production_maps.snapshot_revision(),
            scope.key,
        );
        let kind = if next != cursor {
            "invalidate"
        } else {
            "ready"
        };
        if !send(
            &mut socket,
            json!({"type":kind,"epoch":next.0,"rev":next.1,"scope":next.2}),
        )
        .await
        {
            break;
        }
        cursor = next;
        tokio::select! {
            event=events.recv()=>{if matches!(event,Err(tokio::sync::broadcast::error::RecvError::Closed)){break;}},
            _=heartbeat.tick()=>{},
            message=socket.recv()=>{
                match message {
                    Some(Ok(Message::Ping(bytes)))=>{if socket.send(Message::Pong(bytes)).await.is_err(){break;}},
                    Some(Ok(Message::Close(_)))|Some(Err(_))|None=>break,
                    _=>{},
                }
            }
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_order_row_does_not_replace_missing_customer_with_product_code() {
        let map: ProductionMapDefinition = serde_json::from_value(json!({
            "id":"zakaz-row","title":"Order","product_code":"PRODUCT-42",
            "nodes":[{"id":"end","kind":"end","title":"Product title"}],"edges":[]
        }))
        .unwrap();
        let value = row(&map, 7);
        assert!(value.customer_name.is_empty());
        assert_eq!(value.title, "Order");
        assert_eq!(value.position, 7);
    }

    #[test]
    fn preparation_order_cursor_rejects_scope_and_revision_reuse() {
        let cursor = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&("version-a", 30usize)).unwrap());
        assert_eq!(offset(&cursor, "version-a").unwrap(), 30);
        assert_eq!(
            offset(&cursor, "version-b").unwrap_err().0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            offset("invalid", "v").unwrap_err().0,
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn preparation_order_query_is_bounded_and_responsibility_scope_is_stable() {
        let q = OrderListQuery {
            limit: 30,
            search: " Film ".into(),
            ..Default::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(q.search, "film");
        assert!(
            OrderListQuery {
                limit: 51,
                ..Default::default()
            }
            .normalized()
            .is_err()
        );
        let a = responsibilities(
            &json!({"materials":[{"material_id":"B","material_name":"Film"},{"material_id":"A","material_name":"Glue"}]}),
        );
        let b = responsibilities(
            &json!({"materials":[{"material_id":"a","material_name":"glue"},{"material_id":"b","material_name":"film"}]}),
        );
        assert_eq!(a, b);
        assert_ne!(digest(&a), digest(&a[..1]));
    }
}
