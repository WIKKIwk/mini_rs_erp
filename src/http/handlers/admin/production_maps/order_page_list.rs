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
        // Paused work keeps its session/resources but does not occupy the
        // apparatus. Only running work, colour trials and frozen rows lock it.
        let can_reorder = reorder_enabled && !matches!(state,
            ApparatusQueueOrderState::InProgress | ApparatusQueueOrderState::PrintPreflight |
            ApparatusQueueOrderState::Frozen)
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
        let mut row = serde_json::json!({"id":id,"code":display_code(&saved.map),"title":display_title(&saved.map),
            "customer_name":display_subtitle(&saved.map,customers.get(id).map(String::as_str)),"image_id":saved.map.image_id});
        let paused = snapshot.queue_action_controls.iter().filter_map(|(apparatus, controls)| {
            controls.get(id).filter(|control| control.temporarily_paused
                && control.state == ApparatusQueueOrderState::Paused).map(|_| apparatus)
        }).collect::<Vec<_>>();
        if !paused.is_empty() {
            row["temporarily_paused_apparatus"] = serde_json::json!(paused);
        }
        (id.clone(), row)
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

include!("order_page_list_parts/order_watermarks.rs");

#[cfg(test)]
#[path = "order_page_list_tests.rs"]
mod tests;
