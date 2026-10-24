//! Read-only task windows. Physical inventory, full maps and mutation authority
//! are demand-loaded separately; neither cached rows nor a delta authorize writes.
use super::*;
use crate::core::apparatus_standard::{EquipmentCapabilityCode, ExecutionOperation};
use crate::http::ws::{Message, WebSocket, WebSocketUpgrade};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{Duration, timeout};

const PROJECTION: &str = "supply_tasks_v1";
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const HEARTBEAT: Duration = Duration::from_secs(25);
const MAX_FEEDS: usize = 64;

#[derive(Clone, Default, Deserialize)]
pub struct TasksQuery {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    apparatus: String,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    search: String,
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
    10
}

impl TasksQuery {
    fn normalized(mut self) -> Result<Self, AdminError> {
        self.apparatus = self.apparatus.trim().to_string();
        self.search = self.search.trim().to_lowercase();
        if !matches!(self.kind.as_str(), "qolip" | "material")
            || !matches!(self.limit, 5 | 10 | 20 | 50)
            || self.search.chars().count() > 120
            || (!self.apparatus.is_empty()
                && !queue_state::is_canonical_apparatus_id(&self.apparatus))
            || self.scope.len() > 256
            || self.epoch.len() > 256
        {
            return Err(bad_request("invalid_supply_tasks_scope"));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct TaskRow {
    id: String,
    code: String,
    order_number: String,
    title: String,
    product_code: String,
    customer_name: String,
    image_id: String,
    /// Index in the checked canonical prefix, before task/search filtering.
    position: usize,
    apparatus_count: usize,
    task_state: &'static str,
}

#[derive(Clone, Serialize)]
struct TaskWindow {
    ok: bool,
    projection: &'static str,
    kind: String,
    scope: String,
    epoch: String,
    rev: u64,
    version: String,
    apparatus_id: String,
    limit: usize,
    search: String,
    checked_count: usize,
    missing_count: usize,
    rows: Vec<TaskRow>,
    apparatus: Value,
}

type Feed = Arc<AsyncMutex<Option<Arc<TaskWindow>>>>;

/// A bounded single-flight cache shared by HTTP and live connections. Each
/// key includes the actor, effective assignments, picker and exact query.
#[derive(Default)]
pub(crate) struct SupplyTasksHub {
    feeds: Mutex<VecDeque<(String, Feed)>>,
}

impl SupplyTasksHub {
    fn feed(&self, scope: &str) -> Feed {
        let mut feeds = self.feeds.lock().expect("supply tasks cache poisoned");
        if let Some((_, feed)) = feeds.iter().find(|(key, _)| key == scope) {
            return feed.clone();
        }
        while feeds.len() >= MAX_FEEDS {
            feeds.pop_front();
        }
        let feed = Arc::new(AsyncMutex::new(None));
        feeds.push_back((scope.to_string(), feed.clone()));
        feed
    }
}

struct ReadScope {
    scope: String,
    apparatus_id: String,
    apparatus: Value,
    assignments: Vec<String>,
}

fn digest(value: impl Serialize) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&value).expect("task projection serialization"))
    )
}

async fn read_scope(
    state: &AppState,
    principal: &Principal,
    query: &TasksQuery,
) -> Result<ReadScope, AdminError> {
    let capability = match (query.kind.as_str(), &principal.role) {
        ("qolip", PrincipalRole::Qolipchi) => Capability::QolipManage,
        ("material", PrincipalRole::MaterialTaminotchi) => Capability::RawMaterialAssign,
        _ => return Err(forbidden()),
    };
    if !state
        .admin
        .principal_is_active(principal)
        .await
        .map_err(|_| server_error("supply task principal scope failed"))?
    {
        return Err(forbidden());
    }
    if !state
        .admin
        .principal_has_capability(principal, capability)
        .await
    {
        return Err(forbidden());
    }
    let mut assignments = if query.kind == "qolip" {
        state
            .qolip
            .warehouses_for_principal(principal, false)
            .await
            .map_err(|_| server_error("supply task warehouse scope failed"))?
    } else {
        state
            .admin
            .principal_assigned_item_group_scope(principal)
            .await
            .map_err(|_| server_error("supply task material scope failed"))?
    };
    assignments = assignments
        .into_iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect();
    assignments.sort();
    assignments.dedup();
    let mut apparatus = state
        .apparatus
        .list_runtime_projections()
        .await
        .map_err(|_| server_error("supply task apparatus failed"))?;
    if query.kind == "qolip" {
        apparatus.retain(|item| {
            item.execution_profile.operation == ExecutionOperation::Print
                && item
                    .capabilities
                    .contains_key(&EquipmentCapabilityCode::Tooling)
        });
    }
    let apparatus_id = if query.apparatus.is_empty() {
        apparatus
            .first()
            .map(|item| item.apparatus_id.as_str())
            .unwrap_or("")
            .to_string()
    } else if apparatus
        .iter()
        .any(|item| item.apparatus_id.as_str() == query.apparatus)
    {
        query.apparatus.clone()
    } else {
        // Persisted selections can disappear or lose the tooling capability.
        // Normalize to the same first supported picker option as the client.
        apparatus
            .first()
            .map(|item| item.apparatus_id.as_str())
            .unwrap_or("")
            .to_string()
    };
    let apparatus = serde_json::to_value(apparatus).expect("canonical apparatus serialization");
    let scope = digest((
        PROJECTION,
        &principal.role,
        &principal.ref_,
        &query.kind,
        &apparatus_id,
        query.limit,
        &query.search,
        &assignments,
        &apparatus,
    ));
    Ok(ReadScope {
        scope,
        apparatus_id,
        apparatus,
        assignments,
    })
}

pub async fn production_tasks_window(
    State(state): State<AppState>,
    Query(query): Query<TasksQuery>,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let query = query.normalized()?;
    let principal = completion::authenticated_principal_for_live(&state, &headers, "").await?;
    let window = read_window(&state, &principal, &query).await?;
    let mut response = json_response(&*window);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Authorization"));
    Ok(response)
}

async fn read_window(
    state: &AppState,
    principal: &Principal,
    query: &TasksQuery,
) -> Result<Arc<TaskWindow>, AdminError> {
    timeout(READ_TIMEOUT, async {
        for _ in 0..3 {
            let scope = read_scope(state, principal, query).await?;
            let feed = state.production_tasks_live.feed(&scope.scope);
            let mut current = feed.lock().await;
            if let Some(window) = current.as_ref() {
                if window.rev == state.production_maps.snapshot_revision()
                    && window.epoch == state.production_maps.snapshot_epoch()
                {
                    return Ok(window.clone());
                }
            }
            let started_revision = state.production_maps.snapshot_revision();
            let (rows, checked_count, missing_count, revision) = if scope.apparatus_id.is_empty() {
                (Vec::new(), 0, 0, started_revision)
            } else {
                // Reuse the existing authoritative scoped read and occurrence-aware
                // ordering. Never load the global snapshot or actor histories.
                let (snapshot, revision) = state
                    .production_maps
                    .worker_snapshot_shared_with_revision(&[scope.apparatus_id.clone()], &[])
                    .await
                    .map_err(production_map_error)?;
                let by_id = snapshot
                    .maps
                    .iter()
                    .map(|saved| (saved.map.id.as_str(), saved))
                    .collect::<BTreeMap<_, _>>();
                let selected = order_page::ordered_ids(&snapshot, &scope.apparatus_id)
                    .into_iter()
                    .filter(|id| {
                        (id.starts_with("zakaz-") || id.starts_with("training-zakaz-"))
                            && by_id.contains_key(id.as_str())
                    })
                    .take(query.limit)
                    .collect::<Vec<_>>();
                let codes = selected
                    .iter()
                    .filter_map(|id| by_id.get(id.as_str()))
                    .map(|saved| saved.map.product_code.trim().to_string())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                let mut missing = BTreeMap::<String, &'static str>::new();
                if query.kind == "qolip" {
                    let readiness = state
                        .qolip
                        .order_product_readiness(&codes, &scope.assignments)
                        .await
                        .map_err(|_| server_error("supply task mold readiness failed"))?;
                    for id in &selected {
                        let code = by_id[id.as_str()].map.product_code.trim().to_lowercase();
                        if readiness.get(&code) != Some(&true) {
                            missing.insert(id.clone(), "missing");
                        }
                    }
                } else if !selected.is_empty() && !scope.assignments.is_empty() {
                    let materials = state
                        .production_orders
                        .order_material_tasks(&selected, &scope.assignments)
                        .await
                        .map_err(|_| server_error("supply task material readiness failed"))?;
                    for id in &selected {
                        let values = materials
                            .iter()
                            .filter(|material| material.order_id == *id)
                            .collect::<Vec<_>>();
                        if values.iter().any(|value| !value.assigned) {
                            missing.insert(
                                id.clone(),
                                if values.iter().any(|value| value.assigned) {
                                    "partial"
                                } else {
                                    "missing"
                                },
                            );
                        }
                    }
                }
                // Only selected task rows can need a legacy customer fallback.
                let missing_maps = selected
                    .iter()
                    .filter(|id| missing.contains_key(*id))
                    .map(|id| &by_id[id.as_str()].map)
                    .collect::<Vec<_>>();
                let customers = production_map_definition_customers(state, &missing_maps).await;
                let rows = selected
                    .iter()
                    .enumerate()
                    .filter_map(|(position, id)| {
                        let task_state = *missing.get(id)?;
                        let map = &by_id[id.as_str()].map;
                        if !matches_search(map, &query.search, &customers) {
                            return None;
                        }
                        Some(row(
                            map,
                            position,
                            task_state,
                            customers.get(id).map(String::as_str),
                        ))
                    })
                    .collect::<Vec<_>>();
                (rows, selected.len(), missing.len(), revision)
            };
            // Database NOTIFY is commit-bound. Never stamp an earlier predicate
            // read with a later revision, or reuse changed assignment scopes.
            if revision != started_revision
                || state.production_maps.snapshot_revision() != revision
                || read_scope(state, principal, query).await?.scope != scope.scope
            {
                continue;
            }
            let epoch = state.production_maps.snapshot_epoch().to_string();
            let version = digest((&scope.scope, &epoch, checked_count, missing_count, &rows));
            let window = Arc::new(TaskWindow {
                ok: true,
                projection: PROJECTION,
                kind: query.kind.clone(),
                scope: scope.scope,
                epoch,
                rev: revision,
                version,
                apparatus_id: scope.apparatus_id,
                limit: query.limit,
                search: query.search.clone(),
                checked_count,
                missing_count,
                rows,
                apparatus: scope.apparatus,
            });
            *current = Some(window.clone());
            return Ok(window);
        }
        Err(conflict("supply_tasks_changed_during_read"))
    })
    .await
    .map_err(|_| conflict("supply_tasks_read_timeout"))?
}

fn product_title(map: &ProductionMapDefinition) -> &str {
    map.nodes
        .iter()
        .filter(|node| node.kind == ProductionMapNodeKind::End)
        .map(|node| node.title.trim())
        .find(|title| !title.is_empty() && *title != map.title.trim())
        .unwrap_or("")
}

fn row(
    map: &ProductionMapDefinition,
    position: usize,
    task_state: &'static str,
    customer: Option<&str>,
) -> TaskRow {
    let code = if !map.code.trim().is_empty() {
        map.code.trim()
    } else if !map.order_number.trim().is_empty() {
        map.order_number.trim()
    } else {
        map.id.strip_prefix("zakaz-").unwrap_or("").trim()
    };
    let customer_name = customer
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| (!map.customer_name.trim().is_empty()).then_some(map.customer_name.trim()))
        .or_else(|| (!map.product_code.trim().is_empty()).then_some(map.product_code.trim()))
        .unwrap_or_else(|| product_title(map));
    TaskRow {
        id: map.id.clone(),
        code: code.to_string(),
        order_number: map.order_number.clone(),
        title: if map.title.trim().is_empty() {
            product_title(map).to_string()
        } else {
            map.title.trim().to_string()
        },
        product_code: map.product_code.clone(),
        customer_name: customer_name.to_string(),
        image_id: map.image_id.clone(),
        position,
        apparatus_count: map
            .nodes
            .iter()
            .filter(|node| node.kind == ProductionMapNodeKind::Apparatus)
            .count(),
        task_state,
    }
}

fn matches_search(
    map: &ProductionMapDefinition,
    search: &str,
    customers: &BTreeMap<String, String>,
) -> bool {
    search.is_empty()
        || [
            map.code.as_str(),
            &map.order_number,
            &map.title,
            &map.product_code,
            &map.customer_name,
            customers.get(&map.id).map(String::as_str).unwrap_or(""),
            map.id.strip_prefix("zakaz-").unwrap_or(""),
        ]
        .into_iter()
        .chain(map.nodes.iter().map(|node| node.title.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .contains(search)
}

pub async fn production_tasks_live(
    State(state): State<AppState>,
    Query(query): Query<TasksQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, AdminError> {
    let query = query.normalized()?;
    let token = if query.token.trim().is_empty() {
        bearer_token(&headers).ok_or_else(unauthorized)?
    } else {
        query.token.clone()
    };
    let principal = completion::authenticated_principal_for_live(&state, &headers, &token).await?;
    read_scope(&state, &principal, &query).await?;
    Ok(ws
        .on_upgrade(move |socket| socket_loop(state, socket, principal, token, query))
        .into_response())
}

fn delta(before: &TaskWindow, after: &TaskWindow) -> Value {
    let previous = before
        .rows
        .iter()
        .map(|row| (&row.id, row))
        .collect::<BTreeMap<_, _>>();
    let next = after
        .rows
        .iter()
        .map(|row| (&row.id, row))
        .collect::<BTreeMap<_, _>>();
    let upsert = after
        .rows
        .iter()
        .filter(|row| previous.get(&row.id).copied() != Some(*row))
        .collect::<Vec<_>>();
    let remove = before
        .rows
        .iter()
        .filter(|row| !next.contains_key(&row.id))
        .map(|row| &row.id)
        .collect::<Vec<_>>();
    json!({"ok":true,"type":"tasks_delta","epoch":after.epoch,"scope":after.scope,
        "base_rev":before.rev,"rev":after.rev,"version":after.version,
        "checked_count":after.checked_count,"missing_count":after.missing_count,
        "order_ids":after.rows.iter().map(|row| &row.id).collect::<Vec<_>>(),"upsert":upsert,"remove":remove})
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
    principal: Principal,
    token: String,
    query: TasksQuery,
) {
    // Subscribe before reading; an intervening commit remains queued.
    let mut events = state.production_maps.subscribe_live();
    let mut previous = match read_window(&state, &principal, &query).await {
        Ok(window) => window,
        Err(error) => {
            let _ = send_error(&mut socket, &error).await;
            return;
        }
    };
    let initial = if query.epoch == previous.epoch
        && query.scope == previous.scope
        && query.rev == Some(previous.rev)
    {
        json!({"ok":true,"type":"tasks_ready","epoch":previous.epoch,"scope":previous.scope,
            "rev":previous.rev,"version":previous.version,"resync":false})
    } else {
        json!({"ok":true,"type":"tasks_snapshot","window":&*previous})
    };
    if !send(&mut socket, initial).await {
        return;
    }
    let mut heartbeat = tokio::time::interval(HEARTBEAT);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    loop {
        let recheck = tokio::select! {
            event = events.recv() => match event { Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => true, Err(_) => break },
            _ = heartbeat.tick() => false,
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Ping(bytes))) => { if socket.send(Message::Pong(bytes)).await.is_err() { break; } },
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => {},
                }
                continue;
            }
        };
        // A long-lived socket must not retain revoked capabilities or assignments.
        let current_principal = match state.sessions.get(&token).await {
            Ok(value) => value,
            Err(_) => {
                let _ = send_error(&mut socket, &unauthorized()).await;
                break;
            }
        };
        if current_principal.role != principal.role || current_principal.ref_ != principal.ref_ {
            let _ = send_error(&mut socket, &unauthorized()).await;
            break;
        }
        let scope = match read_scope(&state, &current_principal, &query).await {
            Ok(value) => value,
            Err(error) => {
                let _ = send_error(&mut socket, &error).await;
                break;
            }
        };
        if !recheck
            && scope.scope == previous.scope
            && state.production_maps.snapshot_revision() == previous.rev
        {
            if !send(
                &mut socket,
                json!({"ok":true,"type":"tasks_ready","epoch":previous.epoch,"scope":previous.scope,
                "rev":previous.rev,"version":previous.version,"resync":false}),
            )
            .await
            {
                break;
            }
            continue;
        }
        let next = match read_window(&state, &current_principal, &query).await {
            Ok(window) => window,
            Err(error) => {
                let _ = send_error(&mut socket, &error).await;
                break;
            }
        };
        if next.scope != previous.scope
            || next.epoch != previous.epoch
            || next.apparatus != previous.apparatus
        {
            if !send(
                &mut socket,
                json!({"ok":true,"type":"tasks_snapshot","window":&*next}),
            )
            .await
            {
                break;
            }
        } else if next.rev != previous.rev || next.version != previous.version {
            // Empty task changes still advance this socket's exact cursor.
            if !send(&mut socket, delta(&previous, &next)).await {
                break;
            }
        }
        previous = next;
    }
    let _ = socket.send(Message::Close(None)).await;
}

async fn send_error(socket: &mut WebSocket, error: &AdminError) -> bool {
    let terminal = matches!(error.0, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN);
    send(socket, json!({"ok":false,"type":if terminal { "tasks_error" } else { "tasks_resync" },
        "error":if error.0 == StatusCode::UNAUTHORIZED { "unauthorized" } else if terminal { "forbidden" } else { "supply_tasks_resync" },
        "status":error.0.as_u16(),"resync":!terminal})).await
}
