use super::*;
use crate::core::production_map::{
    CompletedQueueOrder, CompletionRequestDecisionNotification, CompletionRequestNotification,
    ProductionMapLiveEvent, ProductionMapLiveSnapshot,
};
use crate::http::json_research;
use crate::http::ws::{Message, Utf8Bytes, WebSocket};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::time::{Duration, timeout};

#[path = "worker_list.rs"]
mod worker_list;
pub(super) fn worker_list_payload(view: &LiveView, show_all: bool) -> Value {
    worker_list::payload(view, show_all)
}

// A slow client gets a compressed HTTP resync, never an unbounded WS frame.
const MAX_STATE_DELTA_BYTES: usize = 64 * 1024;

pub(super) struct LiveView {
    pub snapshot: Arc<ProductionMapLiveSnapshot>,
    // Full authorized candidate routes before any UI queue window is applied.
    dependency_snapshot: Arc<ProductionMapLiveSnapshot>,
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
    timeout(
        Duration::from_secs(15),
        read_view_consistent(
            state,
            principal,
            include_requests,
            worker_scope,
            reuse_history,
            window,
        ),
    )
    .await
    .map_err(|_| conflict("production_state_read_timeout"))?
}

async fn read_view_consistent(
    state: &AppState,
    principal: &Principal,
    include_requests: bool,
    worker_scope: bool,
    reuse_history: Option<(&LiveView, u64)>,
    window: &super::worker_snapshot::WorkerQueueWindow,
) -> Result<LiveView, AdminError> {
    let service = &state.production_maps;
    for _ in 0..3 {
        let started_revision = service.snapshot_revision();
        let actor = queue_action_actor(principal);
        let worker_assignment = if principal.role.is_apparatus_operator() {
            Some(super::worker_snapshot::scope_token(state, principal).await?)
        } else { None };
        // Reuse history only for an exact colour-only revision. Retry when a
        // concurrent commit moves the revision while history/snapshot is read.
        let reusable = reuse_history.filter(|(before, expected)| {
            colour_history_unchanged(before.revision, started_revision, *expected)
                && worker_assignment.as_ref().is_none_or(|(_, assignment_scope)| {
                    worker_scope && before.scope == window.scope(assignment_scope)
                })
        });
        let (completed, requests, decisions) = if let Some((before, _)) = reusable {
            (
                before.completed.clone(),
                before.requests.clone(),
                before.decisions.clone(),
            )
        } else {
            tokio::try_join!(
                async {
                    if let Some((assigned, _)) = &worker_assignment {
                        service.completed_queue_orders_for_apparatuses(assigned, window.history_limit()).await
                    } else {
                        service.completed_queue_orders_for_actor(&actor.ref_, window.history_limit()).await
                    }
                },
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
        let (snapshot, revision, scope, dependency_snapshot) = if worker_scope {
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
            (snapshot.clone(), revision, String::new(), snapshot)
        };
        if revision != started_revision {
            continue;
        }
        let customers = production_map_order_customers(state, &snapshot.maps).await;
        if service.snapshot_revision() != revision {
            continue;
        }
        if let Some((_, assignment_scope)) = &worker_assignment {
            let (_, current_scope) = super::worker_snapshot::scope_token(state, principal).await?;
            if current_scope != *assignment_scope { continue; }
        }
        return Ok(LiveView {
            snapshot,
            dependency_snapshot,
            revision,
            epoch: service.snapshot_epoch().to_string(),
            scope,
            customers,
            completed,
            requests,
            decisions,
        });
    }
    Err(conflict("production_state_changed_during_read"))
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

#[cfg(test)]
pub(super) fn snapshot_patch(
    old: &ProductionMapLiveSnapshot,
    new: &ProductionMapLiveSnapshot,
) -> Map<String, Value> {
    snapshot_patch_for_protocol(old, new, false)
}

pub(super) fn snapshot_patch_for_protocol(
    old: &ProductionMapLiveSnapshot,
    new: &ProductionMapLiveSnapshot,
    compact: bool,
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
    if compact {
        for (name, before, after) in [
            ("sequences", &old.sequences, &new.sequences),
            (
                "visible_order_ids",
                &old.visible_order_ids,
                &new.visible_order_ids,
            ),
        ] {
            if let Some(value) = sequence_map_patch(before, after) {
                patch.insert(name.into(), value);
            }
        }
    } else {
        field!(sequences);
        field!(visible_order_ids);
    }
    field!(sequence_versions);
    field!(sequence_revisions);
    field!(order_statuses);
    field!(order_controls);
    if old.stage_order_controls != new.stage_order_controls {
        patch.insert("stage_order_controls".into(), serde_json::to_value(&new.stage_order_controls).expect("serializable controls"));
    }
    field!(frozen_orders_by_apparatus);
    nested!(sequence_positions_by_apparatus);
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
        let before = old
            .maps
            .iter()
            .map(|saved| saved.map.id.clone())
            .collect::<Vec<_>>();
        let after = new
            .maps
            .iter()
            .map(|saved| saved.map.id.clone())
            .collect::<Vec<_>>();
        if let Some(edit) = compact
            .then(|| super::sequence_delta::sequence_edit(&before, &after))
            .flatten()
        {
            patch.insert("map_order_edit".into(), edit);
        } else {
            patch.insert("map_order".into(), json!(after));
        }
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

fn sequence_map_patch(
    before: &BTreeMap<String, Vec<String>>,
    after: &BTreeMap<String, Vec<String>>,
) -> Option<Value> {
    let mut upsert = Map::new();
    let mut edit = Map::new();
    for (key, values) in after {
        if before.get(key) == Some(values) {
            continue;
        }
        if let Some(delta) = before
            .get(key)
            .and_then(|previous| super::sequence_delta::sequence_edit(previous, values))
        {
            edit.insert(key.clone(), delta);
        } else {
            upsert.insert(key.clone(), json!(values));
        }
    }
    let remove = before
        .keys()
        .filter(|key| !after.contains_key(*key))
        .collect::<Vec<_>>();
    (!upsert.is_empty() || !edit.is_empty() || !remove.is_empty())
        .then(|| json!({"upsert": upsert, "remove": remove, "edit": edit}))
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
    match json_research::to_string(&value) {
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

/// Print-preflight changes can also affect another order's permissions on
/// the same machine. Keep the full route and alternative-machine dependency
/// closure, not just the currently displayed apparatus or target order.
fn preflight_outside_view(
    snapshot: &ProductionMapLiveSnapshot,
    order_id: &str,
    apparatus: &str,
) -> bool {
    if order_id.is_empty() || !queue_state::is_canonical_apparatus_id(apparatus) {
        return false;
    }
    let same = |id: &str| queue_state::apparatus_ids_match(id, apparatus);
    !snapshot.maps.iter().any(|saved| {
        saved.map.id == order_id
            || saved.map.nodes.iter().any(|node| {
                same(&node.apparatus_id) || same(&node.alternative_assigned_apparatus_id)
            })
    }) && !snapshot.sequences.keys().any(|id| same(id))
        && !snapshot.visible_order_ids.keys().any(|id| same(id))
        && !snapshot
            .queue_action_controls
            .iter()
            .any(|(id, controls)| same(id) && !controls.is_empty())
        && !snapshot.order_controls.contains_key(order_id)
        && !snapshot.order_statuses.contains_key(order_id)
        && !snapshot.stage_states.contains_key(order_id)
}

include!("live_delta_parts/delivery.rs");

#[cfg(test)]
#[path = "live_delta_tests.rs"]
mod tests;
