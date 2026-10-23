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
        Some((id.clone(), serde_json::json!({
            "id":id, "code":saved.map.code, "title":saved.map.title,
            "customer_name":customers.get(id).map(String::as_str).unwrap_or(&saved.map.customer_name),
            "image_id":saved.map.image_id, "queue_state":state.as_str(),
            "display_state":display_state(snapshot, id, apparatus, state),
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
