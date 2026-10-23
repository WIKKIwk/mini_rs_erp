//! Opt-in worker list wire contract. Canonical domain snapshots remain private
//! authority; list records never pretend to be maps or command controls.
use super::*;

fn fields(view: &LiveView) -> Value {
    let snapshot = &view.snapshot;
    let mut value = super::super::order_page::worker_list_fields(snapshot, &view.customers);
    value["worker_pending_report_ids"] = json!(
        super::super::order_page::worker_pending_report_ids(snapshot, &view.completed)
    );
    for (name, field) in [
        ("sequences", json!(snapshot.sequences)),
        ("visible_order_ids", json!(snapshot.visible_order_ids)),
        ("sequence_versions", json!(snapshot.sequence_versions)),
        ("sequence_revisions", json!(snapshot.sequence_revisions)),
        (
            "sequence_positions_by_apparatus",
            json!(snapshot.sequence_positions_by_apparatus),
        ),
        ("queue_states", json!(snapshot.queue_states)),
        ("queue_policies", json!(snapshot.queue_policies)),
        ("completed_orders", json!(view.completed)),
        ("completion_requests", json!(view.requests)),
        ("completion_request_decisions", json!(view.decisions)),
    ] {
        value[name] = field;
    }
    value
}

pub(super) fn payload(view: &LiveView, show_all: bool) -> Value {
    let mut value = fields(view);
    value["ok"] = true.into();
    value["projection"] = "worker_list_v1".into();
    value["epoch"] = view.epoch.clone().into();
    value["scope"] = view.scope.clone().into();
    value["rev"] = view.revision.into();
    value["worker_show_all_apparatus_tabs"] = show_all.into();
    value
}

fn object(value: &Value) -> BTreeMap<String, Value> {
    value
        .as_object()
        .into_iter()
        .flatten()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}
fn nested(value: &Value) -> BTreeMap<String, BTreeMap<String, Value>> {
    object(value)
        .into_iter()
        .map(|(key, value)| (key, object(&value)))
        .collect()
}

pub(super) fn patch(before: &LiveView, after: &LiveView) -> Map<String, Value> {
    let (before_fields, after_fields) = (fields(before), fields(after));
    let mut patch = Map::new();
    for key in [
        "worker_list_rows",
        "order_controls",
        "worker_pending_report_ids",
    ] {
        if let Some(value) = map_patch(&object(&before_fields[key]), &object(&after_fields[key])) {
            patch.insert(key.into(), value);
        }
    }
    for key in ["worker_display_states", "worker_report_prompts"] {
        if let Some(value) = nested_patch(&nested(&before_fields[key]), &nested(&after_fields[key]))
        {
            patch.insert(key.into(), value);
        }
    }
    // Existing typed patch shapes, without ever serializing maps/programs,
    // action controls, graph states or full order-control records.
    for (key, old, new) in [
        (
            "sequences",
            &before.snapshot.sequences,
            &after.snapshot.sequences,
        ),
        (
            "visible_order_ids",
            &before.snapshot.visible_order_ids,
            &after.snapshot.visible_order_ids,
        ),
    ] {
        if let Some(value) = sequence_map_patch(old, new) {
            patch.insert(key.into(), value);
        }
    }
    if let Some(value) = map_patch(
        &before.snapshot.sequence_versions,
        &after.snapshot.sequence_versions,
    ) {
        patch.insert("sequence_versions".into(), value);
    }
    if let Some(value) = map_patch(
        &before.snapshot.sequence_revisions,
        &after.snapshot.sequence_revisions,
    ) {
        patch.insert("sequence_revisions".into(), value);
    }
    if let Some(value) = nested_patch(&before.snapshot.queue_states, &after.snapshot.queue_states) {
        patch.insert("queue_states".into(), value);
    }
    if let Some(value) = nested_patch(
        &before.snapshot.sequence_positions_by_apparatus,
        &after.snapshot.sequence_positions_by_apparatus,
    ) {
        patch.insert("sequence_positions_by_apparatus".into(), value);
    }
    let policy_map = |snapshot: &ProductionMapLiveSnapshot| {
        snapshot
            .queue_policies
            .iter()
            .map(|p| (p.apparatus_id.to_string(), p.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    if let Some(value) = map_patch(&policy_map(&before.snapshot), &policy_map(&after.snapshot)) {
        patch.insert("queue_policies".into(), value);
    }
    for key in [
        "completed_orders",
        "completion_requests",
        "completion_request_decisions",
    ] {
        if before_fields[key] != after_fields[key] {
            patch.insert(key.into(), after_fields[key].clone());
        }
    }
    patch
}
