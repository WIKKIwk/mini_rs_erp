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
        temporary_pause: None,
        cancel_start: None,
        temporarily_paused: false,
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
fn sequence_rows_allow_paused_reordering_but_keep_running_trial_and_frozen_guards() {
    for (state, can_reorder) in [
        ("pending", true),
        ("paused", true),
        ("completed", true),
        ("in_progress", false),
        ("print_preflight", false),
        ("frozen", false),
    ] {
        let mut snapshot = ProductionMapLiveSnapshot::default();
        snapshot.maps.push(map());
        snapshot.queue_states.insert(
            APPARATUS.into(),
            BTreeMap::from([(ID.into(), state.into())]),
        );
        let rows = project(&snapshot, &[ID.into()], APPARATUS, "", &BTreeMap::new());
        assert_eq!(rows["rows"][ID]["queue_state"], state);
        assert_eq!(rows["rows"][ID]["can_reorder"], can_reorder, "{state}");
        let searched = project(&snapshot, &[ID.into()], APPARATUS, "order", &BTreeMap::new());
        assert_eq!(searched["rows"][ID]["can_reorder"], false, "search: {state}");
    }
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
