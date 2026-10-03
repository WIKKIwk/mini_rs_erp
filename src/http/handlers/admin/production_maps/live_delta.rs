use super::*;
use crate::core::production_map::{
    CompletedQueueOrder, CompletionRequestDecisionNotification, CompletionRequestNotification,
    ProductionMapLiveEvent, ProductionMapLiveSnapshot,
};
use axum::extract::ws::{Message, WebSocket};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::time::{Duration, timeout};

// A slow client gets a compressed HTTP resync, never an unbounded WS frame.
const MAX_STATE_DELTA_BYTES: usize = 64 * 1024;

struct LiveView {
    snapshot: Arc<ProductionMapLiveSnapshot>,
    revision: u64,
    epoch: String,
    scope: String,
    customers: BTreeMap<String, String>,
    completed: Vec<CompletedQueueOrder>,
    requests: Vec<CompletionRequestNotification>,
    decisions: Vec<CompletionRequestDecisionNotification>,
}

fn colour_history_unchanged(before_revision: u64, revision: u64, event_revision: u64) -> bool {
    event_revision == revision && before_revision.checked_add(1) == Some(revision)
}

async fn read_view(
    state: &AppState,
    principal: &Principal,
    include_requests: bool,
    worker_scope: bool,
    reuse_history: Option<(&LiveView, u64)>,
) -> Result<LiveView, AdminError> {
    let service = &state.production_maps;
    let (snapshot, revision) = service
        .live_snapshot_shared_with_revision()
        .await
        .map_err(production_map_error)?;
    let snapshot =
        super::super::training::merge_worker_training_snapshot_shared(state, principal, snapshot)
            .await
            .map_err(super::super::training::training_workspace_error)?;
    let actor = queue_action_actor(principal);
    // Colour trials do not write completion history/request events. Reuse
    // those lists only for this exact next revision, not coalesced writes.
    let reusable = reuse_history.filter(|(before, expected)| {
        colour_history_unchanged(before.revision, revision, *expected)
    });
    let (completed, requests, decisions) = if let Some((before, _)) = reusable {
        (
            before.completed.clone(),
            before.requests.clone(),
            before.decisions.clone(),
        )
    } else {
        tokio::try_join!(
            service.completed_queue_orders_for_actor(&actor.ref_, 200),
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
    let (snapshot, scope) = if worker_scope {
        super::worker_snapshot::project_for_principal(
            state,
            principal,
            snapshot,
            completed
                .iter()
                .map(|order| order.order_id.clone())
                .chain(decisions.iter().map(|decision| decision.order_id.clone())),
        )
        .await?
    } else {
        (snapshot, String::new())
    };
    let customers = production_map_order_customers(state, &snapshot.maps).await;
    Ok(LiveView {
        snapshot,
        revision,
        epoch: service.snapshot_epoch().to_string(),
        scope,
        customers,
        completed,
        requests,
        decisions,
    })
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

fn view_patch(before: &LiveView, after: &LiveView) -> Map<String, Value> {
    let mut patch = Map::new();
    let old = &before.snapshot;
    let new = &after.snapshot;
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

async fn send(socket: &mut WebSocket, value: Value) -> bool {
    match serde_json::to_string(&value) {
        Ok(json) => send_text(socket, json).await,
        Err(_) => false,
    }
}

async fn send_text(socket: &mut WebSocket, json: String) -> bool {
    timeout(Duration::from_secs(15), socket.send(Message::Text(json.into())))
        .await.is_ok_and(|result| result.is_ok())
}

pub(super) fn production_map_state_live_socket(
    state: AppState,
    mut socket: WebSocket,
    principal: Principal,
    include_requests: bool,
    client_epoch: String,
    client_revision: Option<u64>,
    worker_scope: bool,
    client_scope: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async move {
        // Subscribe before reading: updates during bootstrap must not be lost.
        let mut events = state.production_maps.subscribe_live();
        let mut baseline =
            match read_view(&state, &principal, include_requests, worker_scope, None).await {
                Ok(view) => view,
                Err(error) => {
                    let _ = send(&mut socket, json!(error.1.0)).await;
                    return;
                }
            };
        // Workers have already loaded the gzip HTTP snapshot. No duplicate
        // multi-megabyte initial frame; cursor mismatch requests HTTP resync.
        if !send(
            &mut socket,
            json!({
            "ok": true, "type": "state_ready", "epoch": baseline.epoch, "rev": baseline.revision,
            "scope": baseline.scope,
            "resync": client_epoch != baseline.epoch || client_revision != Some(baseline.revision) || client_scope != baseline.scope,
            }),
        )
        .await
        {
            return;
        }
        let mut heartbeat = tokio::time::interval(Duration::from_secs(25));
        loop {
            tokio::select! {
                inbound = socket.recv() => match inbound {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                },
                received = events.recv() => {
                    let (lagged, colour_revision) = match received {
                        Ok(ProductionMapLiveEvent::Invalidate | ProductionMapLiveEvent::Delta(_)) => (false, None),
                        Ok(ProductionMapLiveEvent::PrintPreflight { revision }) => (false, Some(revision)),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => (true, None),
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    let next = match read_view(&state, &principal, include_requests, worker_scope,
                        colour_revision.map(|revision| (&baseline, revision))).await {
                        Ok(view) => view,
                        Err(error) => { let _ = send(&mut socket, json!(error.1.0)).await; break; }
                    };
                    let patch = view_patch(&baseline, &next);
                    if !lagged && patch.is_empty() && baseline.revision == next.revision { continue; }
                    let scope_changed = baseline.scope != next.scope;
                    let payload = json!({
                        "ok": true, "type": "state_delta", "epoch": next.epoch,
                        "base_rev": baseline.revision, "rev": next.revision, "patch": patch,
                        "scope": next.scope,
                    });
                    let serialized = serde_json::to_string(&payload).ok();
                    let too_large = serialized.as_ref().is_none_or(|json| json.len() > MAX_STATE_DELTA_BYTES);
                    if lagged || too_large || scope_changed {
                        if !send(&mut socket, json!({"ok": true, "type": "state_resync", "epoch": next.epoch, "rev": next.revision, "scope": next.scope})).await { break; }
                    } else if !send_text(&mut socket, serialized.expect("bounded serialized patch")).await { break; }
                    baseline = next;
                },
                _ = heartbeat.tick() => {
                    if worker_scope {
                        match super::worker_snapshot::scope_token(&state, &principal).await {
                            Ok((_, scope)) if scope != baseline.scope => {
                                let next = match read_view(&state, &principal, include_requests, worker_scope, None).await {
                                    Ok(view) => view,
                                    Err(error) => { let _ = send(&mut socket, json!(error.1.0)).await; break; }
                                };
                                if !send(&mut socket, json!({"ok": true, "type": "state_resync", "epoch": next.epoch, "rev": next.revision, "scope": next.scope})).await { break; }
                                baseline = next;
                            }
                            Err(_) => { let _ = send(&mut socket, json!({"ok": false, "error": "forbidden"})).await; break; }
                            _ => {}
                        }
                    }
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

    #[test]
    fn colour_state_patch_does_not_repeat_285_unchanged_maps() {
        use crate::core::production_map::{ProductionMapDefinition, ProductionMapSaved, compile_map};
        let maps = (0..285).map(|id| {
            let map: ProductionMapDefinition = serde_json::from_value(json!({
                "id": format!("order-{id}"), "product_code": format!("PRODUCT-{id}"),
                "title": "Production order",
                "nodes": [{"id":"start", "kind":"start", "title":"Start"},
                    {"id":"work", "kind":"task", "title":"Work"},
                    {"id":"end", "kind":"end", "title":"End"}],
                "edges": [{"from":"start", "to":"work"}, {"from":"work", "to":"end"}],
            })).unwrap();
            let program = compile_map(&map).unwrap();
            ProductionMapSaved { map, program }
        }).collect();
        let snapshot = ProductionMapLiveSnapshot {
            maps, sequences: BTreeMap::new(), sequence_versions: BTreeMap::new(),
            sequence_revisions: BTreeMap::new(), visible_order_ids: BTreeMap::new(),
            queue_states: BTreeMap::new(), stage_states: BTreeMap::new(),
            queue_policies: vec![], queue_action_controls: BTreeMap::new(),
            order_statuses: BTreeMap::new(), order_controls: BTreeMap::new(),
            frozen_orders_by_apparatus: BTreeMap::new(),
        };
        let full_bytes = serde_json::to_vec(&snapshot).unwrap().len();
        let view = |snapshot, revision| LiveView {
            snapshot: Arc::new(snapshot), revision, epoch: "epoch".into(),
            scope: "scope".into(), customers: BTreeMap::new(), completed: vec![],
            requests: vec![], decisions: vec![],
        };
        let before = view(snapshot.clone(), 8);
        let mut changed = snapshot;
        changed.queue_states.insert("apparatus".into(),
            BTreeMap::from([("order-0".into(), "print_preflight".into())]));
        changed.stage_states.insert("order-0".into(),
            BTreeMap::from([("print".into(), "print_preflight".into())]));
        let after = view(changed, 9);
        let patch = view_patch(&before, &after);
        assert!(!patch.contains_key("maps"));
        assert!(!patch.contains_key("map_order"));
        let delta_bytes = serde_json::to_vec(&patch).unwrap().len();
        assert!(delta_bytes < 1024);
        assert!(full_bytes > delta_bytes * 100);
        eprintln!("synthetic 285-map snapshot: {full_bytes} bytes; colour state patch: {delta_bytes} bytes");
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
