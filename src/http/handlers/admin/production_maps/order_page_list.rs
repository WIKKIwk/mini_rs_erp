//! Presentation-only queue rows. All authority remains in the domain snapshot.
//! Keep this projection identical for HTTP bootstrap and page-window deltas.
use super::*;
use crate::core::production_map::{
    OrderControlState, PrintPreflightStatus, queue_state::ApparatusQueueOrderState,
};

pub(super) fn project(
    snapshot: &ProductionMapLiveSnapshot,
    ids: &[String],
    apparatus: &str,
    search: &str,
    customers: &BTreeMap<String, String>,
) -> serde_json::Value {
    let free_pick = snapshot.queue_policies.iter().any(|policy| {
        policy.apparatus_id.as_str() == apparatus && policy.policy == ApparatusQueuePolicy::FreePick
    });
    let reorder_enabled = !free_pick && search.is_empty();
    let by_id = snapshot
        .maps
        .iter()
        .map(|saved| (saved.map.id.as_str(), saved))
        .collect::<BTreeMap<_, _>>();
    let rows = ids.iter().filter_map(|id| {
        let saved = by_id.get(id.as_str())?;
        let state = card_state(snapshot, saved, apparatus, !reorder_enabled);
        let order_control = snapshot.order_controls.get(id).map(|c| c.state).unwrap_or(OrderControlState::Active);
        // Flutter's reorder guard also treats a paused machine as active.
        // Preserve its exact semantics rather than the execution state's
        // narrower is_active() helper (which intentionally excludes pause).
        let can_reorder = reorder_enabled && !matches!(state,
            ApparatusQueueOrderState::InProgress | ApparatusQueueOrderState::PrintPreflight |
            ApparatusQueueOrderState::Paused | ApparatusQueueOrderState::Frozen)
            && order_control != OrderControlState::Frozen;
        // These are resolved row labels, not a replacement domain map. In
        // particular customer_name is the visible subtitle and may fall back
        // to product/end-node text when no customer is known. Detail loading
        // always replaces it with the complete canonical map/customer lookup.
        Some((id.clone(), serde_json::json!({
            "id":id, "code":display_code(&saved.map), "title":display_title(&saved.map),
            "customer_name":display_subtitle(&saved.map, customers.get(id).map(String::as_str)),
            "image_id":saved.map.image_id, "queue_state":state.as_str(),
            "display_state":display_state(snapshot, id, apparatus, state),
            "frozen":order_control == OrderControlState::Frozen || state == ApparatusQueueOrderState::Frozen,
            "can_reorder":can_reorder,
        })))
    }).collect::<BTreeMap<_, _>>();
    serde_json::json!({
        "projection":SEQUENCE_LIST_PROJECTION,
        "rows":rows,
        "sequence_versions": {apparatus:snapshot.sequence_versions.get(apparatus)},
        "sequence_revisions": {apparatus:snapshot.sequence_revisions.get(apparatus).copied().unwrap_or(0)},
        "queue_policies":snapshot.queue_policies.iter()
            .filter(|p| p.apparatus_id.as_str() == apparatus).collect::<Vec<_>>(),
    })
}

pub(super) fn project_worker(
    snapshot: &ProductionMapLiveSnapshot,
    customers: &BTreeMap<String, String>,
) -> serde_json::Value {
    let rows = snapshot.maps.iter().map(|saved| {
        let id = &saved.map.id;
        (id.clone(), serde_json::json!({"id":id,"code":display_code(&saved.map),"title":display_title(&saved.map),
            "customer_name":display_subtitle(&saved.map,customers.get(id).map(String::as_str)),"image_id":saved.map.image_id}))
    }).collect::<BTreeMap<_,_>>();
    let states = snapshot
        .queue_states
        .iter()
        .map(|(apparatus, states)| {
            (
                apparatus.clone(),
                states
                    .iter()
                    .map(|(id, raw)| {
                        let state = ApparatusQueueOrderState::parse(raw)
                            .unwrap_or(ApparatusQueueOrderState::Pending);
                        (id.clone(), display_state(snapshot, id, apparatus, state))
                    })
                    .collect::<BTreeMap<_, _>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let prompts = snapshot.queue_action_controls.iter().map(|(apparatus,controls)| {
        let values = controls.iter().filter_map(|(id,control)| {
            let work = control.stage_work.as_ref()?;
            if !work.upstream_closed || !work.astatka_available || !work.astatka_required || work.report_session_id.is_empty()
                || snapshot.order_controls.get(id).is_some_and(|control| control.state == OrderControlState::Frozen)
                || snapshot.order_statuses.get(id).is_some_and(|status| status.order_status.trim().eq_ignore_ascii_case("frozen"))
                || control.state == ApparatusQueueOrderState::Frozen { return None; }
            Some((id.clone(),serde_json::json!({"report_session_id":work.report_session_id,"upstream_title":work.upstream_title})))
        }).collect::<BTreeMap<_,_>>();
        (apparatus.clone(),values)
    }).collect::<BTreeMap<_,_>>();
    let mut order_controls = snapshot
        .order_controls
        .iter()
        .map(|(id, control)| (id.clone(), serde_json::json!({"state":control.state})))
        .collect::<BTreeMap<_, _>>();
    // The legacy worker frozen guard also reads the global operational status.
    // Encode that one presentation bit without sending its entire status record.
    for (id, status) in &snapshot.order_statuses {
        if status.order_status.trim().eq_ignore_ascii_case("frozen") {
            order_controls.insert(id.clone(), serde_json::json!({"state":"frozen"}));
        }
    }
    serde_json::json!({"worker_list_rows":rows,"worker_display_states":states,"worker_report_prompts":prompts,"order_controls":order_controls})
}

pub(super) fn worker_pending_report_ids(
    snapshot: &ProductionMapLiveSnapshot,
    completed: &[crate::core::production_map::CompletedQueueOrder],
) -> BTreeMap<String, Vec<String>> {
    use crate::core::production_map::ApparatusQueueInteractionMode;
    let mut history = completed.iter().collect::<Vec<_>>();
    history.sort_by_key(|entry| std::cmp::Reverse(entry.completed_at_unix));
    let available = snapshot
        .maps
        .iter()
        .map(|saved| saved.map.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut pending = BTreeMap::<String, Vec<String>>::new();
    for entry in history {
        let id = &entry.order_id;
        if !available.contains(id.as_str())
            || snapshot
                .order_controls
                .get(id)
                .is_some_and(|control| control.state == OrderControlState::Frozen)
            || snapshot
                .order_statuses
                .get(id)
                .is_some_and(|status| status.order_status.trim().eq_ignore_ascii_case("frozen"))
        {
            continue;
        }
        let Some(control) = snapshot
            .queue_action_controls
            .get(&entry.apparatus)
            .and_then(|controls| controls.get(id))
        else {
            continue;
        };
        if !control
            .stage_work
            .as_ref()
            .is_some_and(|work| work.astatka_required)
            || !matches!(
                control.interaction.mode,
                ApparatusQueueInteractionMode::Paused | ApparatusQueueInteractionMode::Completed
            )
        {
            continue;
        }
        let ids = pending.entry(entry.apparatus.clone()).or_default();
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    pending
}

fn display_code(map: &ProductionMapDefinition) -> &str {
    if !map.code.trim().is_empty() {
        return map.code.trim();
    }
    if !map.order_number.trim().is_empty() {
        return map.order_number.trim();
    }
    map.id
        .trim()
        .strip_prefix("zakaz-")
        .map(str::trim)
        .unwrap_or("")
}

fn product_title(map: &ProductionMapDefinition) -> &str {
    map.nodes
        .iter()
        .filter(|node| node.kind == ProductionMapNodeKind::End)
        .map(|node| node.title.trim())
        .find(|title| !title.is_empty() && *title != map.title.trim())
        .unwrap_or("")
}

fn display_title(map: &ProductionMapDefinition) -> &str {
    if !map.title.trim().is_empty() {
        map.title.trim()
    } else {
        product_title(map)
    }
}

fn display_subtitle<'a>(map: &'a ProductionMapDefinition, customer: Option<&'a str>) -> &'a str {
    if let Some(customer) = customer.map(str::trim).filter(|value| !value.is_empty()) {
        return customer;
    }
    if !map.customer_name.trim().is_empty() {
        return map.customer_name.trim();
    }
    if !map.product_code.trim().is_empty() {
        return map.product_code.trim();
    }
    product_title(map)
}

fn card_state(
    snapshot: &ProductionMapLiveSnapshot,
    saved: &ProductionMapSaved,
    apparatus: &str,
    normalize_occurrences: bool,
) -> ApparatusQueueOrderState {
    let id = &saved.map.id;
    let state = snapshot
        .queue_states
        .get(apparatus)
        .and_then(|states| states.get(id))
        .and_then(|state| ApparatusQueueOrderState::parse(state))
        .unwrap_or(ApparatusQueueOrderState::Pending);
    // Strict admin queues retain raw execution state. Free-pick/search
    // cards normalize a completion belonging to an earlier machine occurrence.
    if !normalize_occurrences || state != ApparatusQueueOrderState::Completed {
        return state;
    }
    let occurrences = crate::core::production_map::chain::linear_work_stages(&saved.map)
        .into_iter()
        .filter(|stage| stage.apparatus_id.as_deref() == Some(apparatus))
        .collect::<Vec<_>>();
    if occurrences.len() < 2 {
        return state;
    }
    let control = snapshot
        .queue_action_controls
        .get(apparatus)
        .and_then(|values| values.get(id));
    let unfinished = occurrences.iter().any(|stage| {
        let stage_state = snapshot
            .stage_states
            .get(id)
            .and_then(|states| states.get(&stage.node_id))
            .map(String::as_str);
        let completed = stage_state == Some("completed")
            || control.is_some_and(|control| {
                control.stage_node_id == stage.node_id
                    && control
                        .stage_work
                        .as_ref()
                        .is_some_and(|work| work.local_completed)
            });
        matches!(
            stage_state,
            Some("pending" | "print_preflight" | "in_progress" | "paused" | "frozen")
        ) && !completed
    });
    if unfinished {
        ApparatusQueueOrderState::Pending
    } else {
        state
    }
}

fn display_state(
    snapshot: &ProductionMapLiveSnapshot,
    id: &str,
    apparatus: &str,
    state: ApparatusQueueOrderState,
) -> &'static str {
    // Exact selected-apparatus card precedence: only global safety warnings
    // override this machine's state. Another machine's activity cannot color it.
    if snapshot.order_statuses.get(id).is_some_and(|status| {
        status
            .order_status
            .trim()
            .eq_ignore_ascii_case("completed_with_issue")
            || status.completed_with_issue_count > 0
    }) {
        return "issue";
    }
    if snapshot
        .order_controls
        .get(id)
        .is_some_and(|control| control.state != OrderControlState::Active)
        || snapshot
            .order_statuses
            .get(id)
            .is_some_and(|status| status.order_status.trim().eq_ignore_ascii_case("frozen"))
    {
        return "frozen";
    }
    if state == ApparatusQueueOrderState::PrintPreflight
        && snapshot
            .queue_action_controls
            .get(apparatus)
            .and_then(|values| values.get(id))
            .is_some_and(|control| {
                control.state == ApparatusQueueOrderState::PrintPreflight
                    && control.print_preflight.as_ref().is_some_and(|hold| {
                        hold.status == PrintPreflightStatus::Passed
                            && hold.order_id.trim() == id
                            && hold.apparatus.trim() == apparatus
                    })
            })
    {
        return "print_preflight_passed";
    }
    state.as_str()
}

/// Only the fields actually displayed by the all-orders list, including its
/// localized activity watermarks. Maps, programs and action authority remain
/// demand-loaded detail even for pending Telegram intake.
pub(super) fn project_orders(
    snapshot: &ProductionMapLiveSnapshot,
    ids: &[String],
    pending: &[&crate::core::pending_orders::PendingOrder],
    customers: &BTreeMap<String, String>,
) -> serde_json::Value {
    let rows = snapshot.maps.iter().filter(|saved| ids.contains(&saved.map.id)).map(|saved| {
        let id = &saved.map.id;
        let state = order_display_state(snapshot, id);
        (id.clone(), serde_json::json!({
            "id":id, "code":display_code(&saved.map), "title":display_title(&saved.map),
            "customer_name":display_subtitle(&saved.map, customers.get(id).map(String::as_str)),
            "image_id":saved.map.image_id, "display_state":state,
            "watermarks":order_watermarks(snapshot, &saved.map, state),
        }))
    }).collect::<BTreeMap<_, _>>();
    let pending_rows = pending
        .iter()
        .map(|order| {
            let template = &order.template;
            (
                order.id.clone(),
                serde_json::json!({
                    "id":order.id,"code":template.order_number,"title":template.product,
                    "customer_name":template.customer,"image_id":template.image_id,"kg":template.kg,
                }),
            )
        })
        .collect::<BTreeMap<_, _>>();
    serde_json::json!({"projection":ORDERS_LIST_PROJECTION,"rows":rows,
        "pending_rows":pending_rows,"pending_ids":pending.iter().map(|order| &order.id).collect::<Vec<_>>()})
}

fn queue_state<'a>(snapshot: &'a ProductionMapLiveSnapshot, apparatus: &str, id: &str) -> &'a str {
    snapshot
        .queue_states
        .get(apparatus)
        .and_then(|states| states.get(id))
        .map(String::as_str)
        .unwrap_or("pending")
}

fn order_display_state(snapshot: &ProductionMapLiveSnapshot, id: &str) -> &'static str {
    let status = snapshot.order_statuses.get(id);
    let raw = status
        .map(|status| status.order_status.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if raw == "completed_with_issue"
        || status.is_some_and(|status| status.completed_with_issue_count > 0)
    {
        return "issue";
    }
    if raw == "frozen"
        || snapshot
            .order_controls
            .get(id)
            .is_some_and(|control| control.state != OrderControlState::Active)
    {
        return "frozen";
    }
    let preflight = || {
        let stages = snapshot
            .queue_states
            .iter()
            .filter(|(_, states)| {
                states
                    .get(id)
                    .is_some_and(|value| value.trim() == "print_preflight")
            })
            .map(|(apparatus, _)| apparatus)
            .collect::<Vec<_>>();
        let passed = !stages.is_empty()
            && stages.iter().all(|apparatus| {
                snapshot
                    .queue_action_controls
                    .get(*apparatus)
                    .and_then(|controls| controls.get(id))
                    .is_some_and(|control| {
                        control.state == ApparatusQueueOrderState::PrintPreflight
                            && control.print_preflight.as_ref().is_some_and(|hold| {
                                hold.status == PrintPreflightStatus::Passed
                                    && hold.order_id.trim() == id
                                    && hold.apparatus.trim() == apparatus.as_str()
                            })
                    })
            });
        if passed {
            "print_preflight_passed"
        } else {
            "print_preflight"
        }
    };
    match raw.as_str() {
        "print_preflight" => return preflight(),
        "paused" => return "paused",
        "in_progress" => return "in_progress",
        "waiting_next_stage" | "partially_completed" => return "waiting_next_stage",
        "completed" => return "completed",
        _ => {}
    }
    if status.is_some_and(|status| {
        matches!(
            status.lifecycle_status.as_str(),
            "production_completed" | "closed"
        )
    }) {
        return "completed";
    }
    // Mirror the legacy list's aggregate queue activity fallback, including a
    // visible-but-unstated queue member being treated as pending.
    let states = snapshot
        .queue_states
        .iter()
        .filter_map(|(_, states)| states.get(id).map(String::as_str))
        .collect::<Vec<_>>();
    let pending = states.contains(&"pending")
        || snapshot.visible_order_ids.iter().any(|(apparatus, ids)| {
            ids.iter().any(|member| member == id)
                && snapshot
                    .queue_states
                    .get(apparatus)
                    .is_none_or(|states| !states.contains_key(id))
        });
    if states.contains(&"frozen") {
        "frozen"
    } else if states.contains(&"print_preflight") {
        preflight()
    } else if states.contains(&"paused") {
        "paused"
    } else if states.contains(&"in_progress") {
        "in_progress"
    } else if states.contains(&"completed") && pending {
        "waiting_next_stage"
    } else if states.contains(&"completed") {
        "completed"
    } else {
        "pending"
    }
}

fn order_watermarks(
    snapshot: &ProductionMapLiveSnapshot,
    map: &ProductionMapDefinition,
    state: &str,
) -> Vec<serde_json::Value> {
    use crate::core::production_map::chain::linear_work_stages;
    let id = &map.id;
    let descriptor =
        |apparatus: &str, kind: &str| serde_json::json!({"apparatus_id":apparatus,"kind":kind});
    if matches!(
        state,
        "pending" | "completed" | "print_preflight" | "print_preflight_passed"
    ) {
        return Vec::new();
    }
    let stages = linear_work_stages(map);
    if state == "waiting_next_stage" {
        let mut completed_seen = false;
        for stage in &stages {
            let Some(apparatus) = stage.apparatus_id.as_deref() else {
                continue;
            };
            let Some(node) = map.nodes.iter().find(|node| node.id == stage.node_id) else {
                continue;
            };
            let station = if !node.alternative_group_id.trim().is_empty()
                || node.alternative_assigned_apparatus_id.trim().is_empty()
            {
                node.apparatus_id.trim()
            } else {
                node.alternative_assigned_apparatus_id.trim()
            };
            let current = snapshot
                .stage_states
                .get(id)
                .and_then(|states| states.get(&stage.node_id))
                .filter(|state| !state.trim().is_empty())
                .map(String::as_str)
                .unwrap_or_else(|| queue_state(snapshot, station, id));
            if current == "completed" {
                completed_seen = true;
            } else if completed_seen && current == "pending" {
                return vec![descriptor(apparatus, "waiting")];
            }
        }
        return vec![descriptor("", "waiting")];
    }
    let mut seen = BTreeSet::new();
    let apparatuses = stages
        .iter()
        .filter_map(|stage| stage.apparatus_id.as_deref())
        .filter(|apparatus| seen.insert(*apparatus))
        .collect::<Vec<_>>();
    if state == "in_progress" {
        return apparatuses
            .into_iter()
            .filter(|apparatus| queue_state(snapshot, apparatus, id) == "in_progress")
            .map(|apparatus| descriptor(apparatus, "active"))
            .collect();
    }
    let (target, kind) = match state {
        "paused" => ("paused", "paused"),
        "frozen" => ("frozen", "frozen"),
        "issue" => ("frozen", "issue"),
        _ => return Vec::new(),
    };
    let matching = apparatuses
        .iter()
        .copied()
        .filter(|apparatus| queue_state(snapshot, apparatus, id) == target)
        .collect::<Vec<_>>();
    if !matching.is_empty() {
        return matching
            .into_iter()
            .map(|apparatus| descriptor(apparatus, kind))
            .collect();
    }
    let fallback = apparatuses
        .iter()
        .copied()
        .find(|apparatus| queue_state(snapshot, apparatus, id) != "completed")
        .or_else(|| apparatuses.first().copied())
        .unwrap_or("");
    vec![descriptor(fallback, kind)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::production_map::{
        ApparatusQueueOrderActionControl, ProductionOrderStatusDetail,
    };
    const APPARATUS: &str = "apparatus:default:asset-010";
    const ID: &str = "zakaz-test";

    fn control(state: ApparatusQueueOrderState) -> ApparatusQueueOrderActionControl {
        ApparatusQueueOrderActionControl {
            stage_work: None,
            work_activity: None,
            last_worked_at_unix: 0,
            re_pause: None,
            reclose: None,
            state,
            allowed_actions: Vec::new(),
            interaction: Default::default(),
            previous_stage: String::new(),
            stage_node_id: String::new(),
            previous_stage_ready: false,
            rezka_output_kadr_counts: Vec::new(),
            rezka_output_report: serde_json::Value::Null,
            rezka_input_lineage: Vec::new(),
            rezka_active_partial_rolls: Vec::new(),
            complete_requires_full_report: false,
            closing_output_batch_id: String::new(),
            complete_requires_rezka_total_waste_only: false,
            rezka_total_waste_only: false,
            freeze_request: None,
            stage_control_state: String::new(),
            print_preflight_allowed: false,
            print_preflight: None,
        }
    }

    fn map() -> ProductionMapSaved {
        serde_json::from_value(serde_json::json!({
            "map":{"id":ID,"product_code":"item","title":"Order","code":"001",
                "nodes":[{"id":"start","kind":"start","title":"Start"},
                    {"id":"first","kind":"apparatus","title":"First","apparatus_id":APPARATUS},
                    {"id":"second","kind":"apparatus","title":"Second","apparatus_id":APPARATUS},
                    {"id":"end","kind":"end","title":"End"}],
                "edges":[{"from":"start","to":"first"},{"from":"first","to":"second"},{"from":"second","to":"end"}]},
            "program":{"map_id":ID,"product_code":"item","operations":[]}
        })).unwrap()
    }

    #[test]
    fn worker_pending_report_handoff_keeps_upstream_open_reports_in_newest_history_order() {
        use crate::core::production_map::{
            ApparatusQueueInteractionMode, CompletedQueueOrder, CompletedQueueOrderStatus,
        };
        let mut snapshot = ProductionMapLiveSnapshot::default();
        let mut completed = Vec::new();
        for (id, time) in [("old", 1), ("latest", 3), ("middle", 2), ("frozen", 4)] {
            let mut saved = map();
            saved.map.id = id.into();
            snapshot.maps.push(saved);
            let mut value = control(ApparatusQueueOrderState::Paused);
            value.interaction.mode = ApparatusQueueInteractionMode::Paused;
            value.stage_work = Some(crate::core::production_map::StageWorkControl {
                astatka_required: true,
                upstream_closed: false,
                astatka_available: false,
                report_session_id: format!("s-{id}"),
                ..Default::default()
            });
            snapshot
                .queue_action_controls
                .entry(APPARATUS.into())
                .or_default()
                .insert(id.into(), value);
            completed.push(CompletedQueueOrder {
                apparatus: APPARATUS.into(),
                order_id: id.into(),
                completed_at_unix: time,
                status: CompletedQueueOrderStatus::InProgress,
                issue_note: String::new(),
            });
        }
        snapshot.order_statuses.insert(
            "frozen".into(),
            ProductionOrderStatusDetail {
                order_status: "frozen".into(),
                ..Default::default()
            },
        );
        assert_eq!(
            worker_pending_report_ids(&snapshot, &completed)[APPARATUS],
            vec!["latest", "middle", "old"]
        );
        // Current-order/QR handoff is broader than automatic upstream-closed
        // prompting. It must not accidentally create an unsolicited dialog.
        assert_eq!(
            project_worker(&snapshot, &BTreeMap::new())["worker_report_prompts"][APPARATUS],
            serde_json::json!({})
        );
    }

    #[test]
    fn worker_list_keeps_report_prompts_and_freeze_guard_without_full_control_records() {
        let mut snapshot = ProductionMapLiveSnapshot::default();
        snapshot.maps.push(map());
        let mut value = control(ApparatusQueueOrderState::InProgress);
        value.stage_work = Some(crate::core::production_map::StageWorkControl {
            upstream_closed: true,
            astatka_available: true,
            astatka_required: true,
            report_session_id: "session".into(),
            upstream_title: "Upstream".into(),
            ..Default::default()
        });
        snapshot
            .queue_action_controls
            .insert(APPARATUS.into(), BTreeMap::from([(ID.into(), value)]));
        snapshot.queue_states.insert(
            APPARATUS.into(),
            BTreeMap::from([(ID.into(), "in_progress".into())]),
        );
        let row = project_worker(&snapshot, &BTreeMap::new());
        assert_eq!(
            row["worker_report_prompts"][APPARATUS][ID],
            serde_json::json!({"report_session_id":"session","upstream_title":"Upstream"})
        );
        assert_eq!(row["worker_display_states"][APPARATUS][ID], "in_progress");
        assert!(row.get("queue_action_controls").is_none());
        assert!(row.get("maps").is_none());
        snapshot.order_statuses.insert(
            ID.into(),
            ProductionOrderStatusDetail {
                order_status: "frozen".into(),
                ..Default::default()
            },
        );
        let row = project_worker(&snapshot, &BTreeMap::new());
        assert_eq!(
            row["worker_report_prompts"][APPARATUS],
            serde_json::json!({})
        );
        assert_eq!(
            row["order_controls"][ID],
            serde_json::json!({"state":"frozen"})
        );
    }

    #[test]
    fn worker_list_preflight_colours_preserve_passed_current_hold_and_global_issue_precedence() {
        let mut snapshot = ProductionMapLiveSnapshot::default();
        snapshot.maps.push(map());
        snapshot.queue_states.insert(
            APPARATUS.into(),
            BTreeMap::from([(ID.into(), "print_preflight".into())]),
        );
        let mut value = control(ApparatusQueueOrderState::PrintPreflight);
        value.print_preflight=Some(serde_json::from_value(serde_json::json!({"hold_id":"h","idempotency_key":"k","order_id":ID,"apparatus":APPARATUS,"status":"passed","actor":{"role":"admin"},"created_at_unix":0,"updated_at_unix":0,"expires_at_unix":0})).unwrap());
        snapshot
            .queue_action_controls
            .insert(APPARATUS.into(), BTreeMap::from([(ID.into(), value)]));
        let fields = project_worker(&snapshot, &BTreeMap::new());
        assert_eq!(
            fields["worker_display_states"][APPARATUS][ID],
            "print_preflight_passed"
        );
        assert!(!fields.to_string().contains("hold_id"));
        snapshot.order_statuses.insert(
            ID.into(),
            ProductionOrderStatusDetail {
                completed_with_issue_count: 1,
                ..Default::default()
            },
        );
        assert_eq!(
            project_worker(&snapshot, &BTreeMap::new())["worker_display_states"][APPARATUS][ID],
            "issue"
        );
    }

    #[test]
    fn all_order_colour_precedence_matches_legacy_global_and_queue_fallbacks() {
        let mut snapshot = ProductionMapLiveSnapshot::default();
        snapshot.maps.push(map());
        snapshot.queue_states.insert(
            APPARATUS.into(),
            BTreeMap::from([(ID.into(), "in_progress".into())]),
        );
        snapshot.queue_states.insert(
            "apparatus:default:other".into(),
            BTreeMap::from([(ID.into(), "paused".into())]),
        );
        assert_eq!(order_display_state(&snapshot, ID), "paused");
        snapshot.order_statuses.insert(
            ID.into(),
            ProductionOrderStatusDetail {
                order_status: "in_progress".into(),
                ..Default::default()
            },
        );
        assert_eq!(order_display_state(&snapshot, ID), "in_progress");
        snapshot.order_controls.insert(ID.into(), serde_json::from_value(serde_json::json!({
            "order_id":ID,"state":"freeze_requested","actor":{"role":"admin"},"requested_at_unix":1
        })).unwrap());
        assert_eq!(order_display_state(&snapshot, ID), "frozen");
        snapshot
            .order_statuses
            .get_mut(ID)
            .unwrap()
            .completed_with_issue_count = 1;
        assert_eq!(order_display_state(&snapshot, ID), "issue");
        snapshot.order_statuses.clear();
        snapshot.order_controls.clear();
        snapshot.queue_states.clear();
        snapshot.queue_states.insert(
            APPARATUS.into(),
            BTreeMap::from([(ID.into(), "completed".into())]),
        );
        snapshot
            .visible_order_ids
            .insert("apparatus:default:next".into(), vec![ID.into()]);
        assert_eq!(order_display_state(&snapshot, ID), "waiting_next_stage");
    }

    #[test]
    fn all_order_watermarks_preserve_occurrences_and_status_fallbacks_without_graph_on_wire() {
        let saved = map();
        let mut snapshot = ProductionMapLiveSnapshot::default();
        snapshot.queue_states.insert(
            APPARATUS.into(),
            BTreeMap::from([(ID.into(), "in_progress".into())]),
        );
        assert_eq!(
            order_watermarks(&snapshot, &saved.map, "in_progress"),
            vec![serde_json::json!({"apparatus_id":APPARATUS,"kind":"active"})]
        );
        snapshot.stage_states.insert(
            ID.into(),
            BTreeMap::from([
                ("first".into(), "completed".into()),
                ("second".into(), "pending".into()),
            ]),
        );
        // A prior completed occurrence must not hide the second pending use
        // of the same physical machine.
        assert_eq!(
            order_watermarks(&snapshot, &saved.map, "waiting_next_stage"),
            vec![serde_json::json!({"apparatus_id":APPARATUS,"kind":"waiting"})]
        );
        for kind in ["paused", "frozen", "issue"] {
            assert_eq!(
                order_watermarks(&snapshot, &saved.map, kind),
                vec![serde_json::json!({"apparatus_id":APPARATUS,"kind":kind})]
            );
        }
        for kind in [
            "pending",
            "completed",
            "print_preflight",
            "print_preflight_passed",
        ] {
            assert!(order_watermarks(&snapshot, &saved.map, kind).is_empty());
        }
        let mut empty = saved.map;
        empty.nodes.clear();
        empty.edges.clear();
        assert_eq!(
            order_watermarks(&snapshot, &empty, "waiting_next_stage"),
            vec![serde_json::json!({"apparatus_id":"","kind":"waiting"})]
        );
        assert_eq!(
            order_watermarks(&snapshot, &empty, "issue"),
            vec![serde_json::json!({"apparatus_id":"","kind":"issue"})]
        );
    }

    #[test]
    fn legacy_row_labels_preserve_trimmed_code_title_and_subtitle_fallbacks() {
        let mut value = map().map;
        value.code = "  code-1  ".into();
        value.order_number = "  number-2  ".into();
        assert_eq!(display_code(&value), "code-1");
        value.code = "  ".into();
        assert_eq!(display_code(&value), "number-2");
        value.order_number.clear();
        value.id = " zakaz-  003  ".into();
        assert_eq!(display_code(&value), "003");
        value.id = "training-zakaz-004".into();
        assert_eq!(display_code(&value), "");

        value.title = "  Root title  ".into();
        value.nodes.last_mut().unwrap().title = "  Finished product  ".into();
        assert_eq!(display_title(&value), "Root title");
        value.title = "  ".into();
        assert_eq!(display_title(&value), "Finished product");
        value.customer_name = "  Map customer  ".into();
        value.product_code = "  Product code  ".into();
        assert_eq!(
            display_subtitle(&value, Some("  Snapshot customer  ")),
            "Snapshot customer"
        );
        assert_eq!(display_subtitle(&value, Some("  ")), "Map customer");
        value.customer_name.clear();
        assert_eq!(display_subtitle(&value, None), "Product code");
        value.product_code.clear();
        assert_eq!(display_subtitle(&value, None), "Finished product");
        value.title = " Finished product ".into();
        assert_eq!(product_title(&value), "");
        value.title.clear();
        value.nodes.clear();
        assert_eq!(display_title(&value), ""); // The client localizes its default.
        assert_eq!(display_subtitle(&value, None), "");
    }

    #[test]
    fn frozen_membership_survives_issue_colour_and_freeze_requested_stays_distinct() {
        let mut snapshot = ProductionMapLiveSnapshot::default();
        snapshot.maps.push(map());
        snapshot.queue_states.insert(
            APPARATUS.into(),
            BTreeMap::from([(ID.into(), "pending".into())]),
        );
        snapshot.order_controls.insert(
            ID.into(),
            serde_json::from_value(serde_json::json!({
                "order_id":ID,"state":"frozen","actor":{"role":"admin"},"requested_at_unix":1
            }))
            .unwrap(),
        );
        snapshot.order_statuses.insert(
            ID.into(),
            ProductionOrderStatusDetail {
                completed_with_issue_count: 1,
                ..Default::default()
            },
        );
        let rows = |snapshot: &ProductionMapLiveSnapshot| {
            project(snapshot, &[ID.into()], APPARATUS, "", &BTreeMap::new())
        };
        let row = rows(&snapshot)["rows"][ID].clone();
        assert_eq!(row["queue_state"], "pending");
        assert_eq!(row["display_state"], "issue");
        assert_eq!(row["frozen"], true);
        assert_eq!(row["can_reorder"], false);
        snapshot.order_controls.get_mut(ID).unwrap().state = OrderControlState::FreezeRequested;
        let row = rows(&snapshot)["rows"][ID].clone();
        assert_eq!(row["frozen"], false);
        assert_eq!(row["can_reorder"], true); // Exact pre-existing .isFrozen guard.
        snapshot.order_controls.get_mut(ID).unwrap().state = OrderControlState::Active;
        snapshot
            .queue_states
            .get_mut(APPARATUS)
            .unwrap()
            .insert(ID.into(), "frozen".into());
        let row = rows(&snapshot)["rows"][ID].clone();
        assert_eq!(row["frozen"], true);
        assert_eq!(row["can_reorder"], false);
    }

    #[test]
    fn selected_machine_colours_preserve_global_safety_precedence() {
        let mut snapshot = ProductionMapLiveSnapshot::default();
        for global in [
            "paused",
            "in_progress",
            "print_preflight",
            "completed",
            "waiting_next_stage",
        ] {
            snapshot.order_statuses.insert(
                ID.into(),
                ProductionOrderStatusDetail {
                    order_status: global.into(),
                    ..Default::default()
                },
            );
            assert_eq!(
                display_state(&snapshot, ID, APPARATUS, ApparatusQueueOrderState::Pending),
                "pending"
            );
        }
        snapshot.order_controls.insert(ID.into(),serde_json::from_value(serde_json::json!({
            "order_id":ID,"state":"freeze_requested","actor":{"role":"admin"},"requested_at_unix":1
        })).unwrap());
        assert_eq!(
            display_state(
                &snapshot,
                ID,
                APPARATUS,
                ApparatusQueueOrderState::InProgress
            ),
            "frozen"
        );
        snapshot
            .order_statuses
            .get_mut(ID)
            .unwrap()
            .completed_with_issue_count = 1;
        assert_eq!(
            display_state(
                &snapshot,
                ID,
                APPARATUS,
                ApparatusQueueOrderState::InProgress
            ),
            "issue"
        );
        snapshot.order_controls.clear();
        snapshot
            .order_statuses
            .get_mut(ID)
            .unwrap()
            .completed_with_issue_count = 0;
        snapshot.order_statuses.get_mut(ID).unwrap().order_status = "frozen".into();
        assert_eq!(
            display_state(&snapshot, ID, APPARATUS, ApparatusQueueOrderState::Paused),
            "frozen"
        );
    }

    #[test]
    fn passed_preflight_requires_current_matching_hold_and_execution_state() {
        let mut snapshot = ProductionMapLiveSnapshot::default();
        let mut value = control(ApparatusQueueOrderState::PrintPreflight);
        value.print_preflight = Some(
            serde_json::from_value(serde_json::json!({
                "hold_id":"hold","idempotency_key":"key","order_id":ID,"apparatus":APPARATUS,
                "status":"passed","actor":{"role":"admin"},"created_at_unix":0,
                "updated_at_unix":0,"expires_at_unix":0
            }))
            .unwrap(),
        );
        snapshot
            .queue_action_controls
            .entry(APPARATUS.into())
            .or_default()
            .insert(ID.into(), value);
        let resolve = |s: &ProductionMapLiveSnapshot, state| display_state(s, ID, APPARATUS, state);
        assert_eq!(
            resolve(&snapshot, ApparatusQueueOrderState::PrintPreflight),
            "print_preflight_passed"
        );
        assert_eq!(
            resolve(&snapshot, ApparatusQueueOrderState::InProgress),
            "in_progress"
        );
        for status in [
            PrintPreflightStatus::Held,
            PrintPreflightStatus::Running,
            PrintPreflightStatus::Failed,
            PrintPreflightStatus::Cancelled,
            PrintPreflightStatus::Consumed,
        ] {
            snapshot
                .queue_action_controls
                .get_mut(APPARATUS)
                .unwrap()
                .get_mut(ID)
                .unwrap()
                .print_preflight
                .as_mut()
                .unwrap()
                .status = status;
            assert_eq!(
                resolve(&snapshot, ApparatusQueueOrderState::PrintPreflight),
                "print_preflight"
            );
        }
        let control = snapshot
            .queue_action_controls
            .get_mut(APPARATUS)
            .unwrap()
            .get_mut(ID)
            .unwrap();
        control.print_preflight.as_mut().unwrap().status = PrintPreflightStatus::Passed;
        control.print_preflight.as_mut().unwrap().apparatus = "apparatus:default:asset-007".into();
        assert_eq!(
            resolve(&snapshot, ApparatusQueueOrderState::PrintPreflight),
            "print_preflight"
        );
    }

    #[test]
    fn free_pick_completed_occurrences_normalize_only_with_explicit_remaining_work() {
        let saved = map();
        let mut snapshot = ProductionMapLiveSnapshot::default();
        snapshot.queue_states.insert(
            APPARATUS.into(),
            BTreeMap::from([(ID.into(), "completed".into())]),
        );
        snapshot.stage_states.insert(
            ID.into(),
            BTreeMap::from([
                ("first".into(), "completed".into()),
                ("second".into(), "pending".into()),
            ]),
        );
        assert_eq!(
            card_state(&snapshot, &saved, APPARATUS, true),
            ApparatusQueueOrderState::Pending
        );
        assert_eq!(
            card_state(&snapshot, &saved, APPARATUS, false),
            ApparatusQueueOrderState::Completed
        );
        snapshot.stage_states.get_mut(ID).unwrap().remove("second");
        assert_eq!(
            card_state(&snapshot, &saved, APPARATUS, true),
            ApparatusQueueOrderState::Completed
        );
        snapshot
            .stage_states
            .get_mut(ID)
            .unwrap()
            .insert("second".into(), "paused".into());
        let mut value = control(ApparatusQueueOrderState::Completed);
        value.stage_node_id = "second".into();
        value.stage_work = Some(
            serde_json::from_value(serde_json::json!({
                "completed":false,"local_completed":true,"upstream_closed":false,
                "astatka_available":false,"astatka_required":false,"report_session_id":"report",
                "upstream_title":"","last_apparatus":"","last_worker_ref":""
            }))
            .unwrap(),
        );
        snapshot
            .queue_action_controls
            .insert(APPARATUS.into(), BTreeMap::from([(ID.into(), value)]));
        assert_eq!(
            card_state(&snapshot, &saved, APPARATUS, true),
            ApparatusQueueOrderState::Completed
        );
        snapshot
            .queue_states
            .get_mut(APPARATUS)
            .unwrap()
            .insert(ID.into(), "in_progress".into());
        assert_eq!(
            card_state(&snapshot, &saved, APPARATUS, true),
            ApparatusQueueOrderState::InProgress
        );
    }
}
