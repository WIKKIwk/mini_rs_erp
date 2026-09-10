use super::*;

fn map() -> ProductionMapDefinition {
    serde_json::from_value(serde_json::json!({"id":"order", "product_code":"P", "title":"Shared operation",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
        {"id":"source","kind":"apparatus","title":"Source","apparatus_id":"apparatus:test:source"},
        {"id":"a","kind":"apparatus","title":"A","apparatus_id":"apparatus:test:a","alternative_group_id":"operation","alternative_assigned_apparatus_id":"apparatus:test:a"},
        {"id":"b","kind":"apparatus","title":"B","apparatus_id":"apparatus:test:b","alternative_group_id":"operation","alternative_assigned_apparatus_id":"apparatus:test:a"},
        {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"source"},{"from":"source","to":"a"},{"from":"a","to":"end"}]})).unwrap()
}

fn session(node: &str, status: OrderRunStatus, seq: Option<u64>) -> OrderRunSession {
    let mut s = OrderRunSession {
        session_id: format!("session-{node}"),
        apparatus: format!("apparatus:test:{node}"),
        order_id: "order".into(),
        stage_node_id: node.into(),
        status,
        worker_role: "aparatchi".into(),
        worker_ref: format!("worker-{node}"),
        worker_display_name: node.into(),
        started_at_unix: 1,
        updated_at_unix: 2,
        payload_json: serde_json::json!({WORK_PROTOCOL:1}),
    };
    if let Some(seq) = seq {
        stamp_work_report(
            &mut s,
            &format!("report-{node}"),
            seq,
            &QueueActionActor {
                ref_: format!("worker-{node}"),
                ..Default::default()
            },
            10,
        );
    }
    s
}

fn statuses(sessions: &[OrderRunSession], inputs: &[StageWorkInput]) -> Vec<StageWorkStatus> {
    stage_work_statuses(&map(), sessions, inputs, &BTreeMap::new(), &[])
}

fn shared(status: &[StageWorkStatus]) -> &StageWorkStatus {
    status.iter().find(|s| s.stage_node_id == "a").unwrap()
}

#[test]
fn alternatives_remain_visible_even_with_old_assignment_and_representative_edges() {
    let m = map();
    assert!(chain::map_has_work_stage_for_station(
        &m,
        "apparatus:test:b"
    ));
    let b = chain::work_stage_for_station(&m, "apparatus:test:b", "a").unwrap();
    assert_eq!(b.node_id, "b");
    assert_eq!(
        chain::previous_work_stage_for_node(&m, "b")
            .unwrap()
            .node_id,
        "source"
    );
    assert!(chain::stage_node_ids_match_for_map(&m, "a", "b"));
}

#[test]
fn unused_candidates_are_not_required() {
    let s = vec![
        session("source", OrderRunStatus::Completed, Some(1)),
        session("b", OrderRunStatus::Completed, Some(2)),
    ];
    let result = statuses(&s, &[]);
    assert!(shared(&result).completed);
    assert_eq!(shared(&result).last_apparatus, "apparatus:test:b");
}

#[test]
fn first_finisher_report_does_not_close_an_active_peer() {
    let s = vec![
        session("source", OrderRunStatus::Completed, Some(1)),
        session("a", OrderRunStatus::Active, None),
        session("b", OrderRunStatus::Completed, Some(2)),
    ];
    let result = statuses(&s, &[]);
    assert!(!shared(&result).completed);
    assert!(shared(&result).astatka_required_apparatuses.is_empty());
}

#[test]
fn late_upstream_closing_requests_only_missing_reports() {
    let mut s = vec![
        session("source", OrderRunStatus::RollDetached, None),
        session("a", OrderRunStatus::Completed, Some(1)),
        session("b", OrderRunStatus::Completed, None),
    ];
    assert!(
        shared(&statuses(&s, &[]))
            .astatka_required_apparatuses
            .is_empty()
    );
    s[0] = session("source", OrderRunStatus::Completed, Some(2));
    let result = statuses(&s, &[]);
    assert_eq!(
        shared(&result).astatka_required_apparatuses,
        vec!["apparatus:test:b"]
    );
    assert!(!shared(&result).completed);
}

#[test]
fn late_upstream_without_new_output_closes_already_reported_downstream() {
    let mut s = vec![
        session("source", OrderRunStatus::RollDetached, None),
        session("a", OrderRunStatus::Completed, Some(2)),
        session("b", OrderRunStatus::Completed, Some(1)),
    ];
    assert!(!shared(&statuses(&s, &[])).completed);
    s[0] = session("source", OrderRunStatus::Completed, Some(3));
    let result = statuses(&s, &[]);
    assert!(shared(&result).completed);
    assert_eq!(shared(&result).last_apparatus, "apparatus:test:a");
    assert_eq!(shared(&result).last_worker_ref, "worker-a");
    assert!(shared(&result).astatka_required_apparatuses.is_empty());
}

#[test]
fn waiting_or_in_use_input_prevents_whole_stage_completion() {
    let s = vec![
        session("source", OrderRunStatus::Completed, Some(1)),
        session("a", OrderRunStatus::Completed, Some(2)),
    ];
    let input = StageWorkInput {
        target_node: "b".into(),
        outstanding: true,
        ..Default::default()
    };
    assert!(!shared(&statuses(&s, &[input])).completed);
}

#[test]
fn unclaimed_input_defers_notification_but_not_manual_accounting() {
    let sessions = vec![
        session("source", OrderRunStatus::Completed, Some(1)),
        session("a", OrderRunStatus::Completed, None),
    ];
    let input = StageWorkInput {
        target_node: "b".into(),
        outstanding: true,
        available: true,
        ..Default::default()
    };
    let status = statuses(&sessions, &[input]);
    assert!(shared(&status).astatka_required_apparatuses.is_empty());
    let control = work_control(&map(), "apparatus:test:a", "a", &status, &sessions).unwrap();
    assert!(control.astatka_available);
    assert!(!control.completed);
    assert_eq!(
        shared(&statuses(&sessions, &[])).astatka_required_apparatuses,
        vec!["apparatus:test:a"]
    );
}

#[test]
fn new_work_invalidates_older_report_but_does_not_erase_it() {
    let mut s = vec![
        session("source", OrderRunStatus::Completed, Some(1)),
        session("a", OrderRunStatus::Completed, Some(2)),
    ];
    let mut later = session("a", OrderRunStatus::Completed, None);
    later.started_at_unix = 3;
    later.session_id = "later".into();
    s.push(later);
    let result = statuses(&s, &[]);
    assert!(!shared(&result).completed);
    assert_eq!(
        shared(&result).astatka_required_apparatuses,
        vec!["apparatus:test:a"]
    );
    assert!(work_report(&s[1]).is_some());
    assert!(!work_control(&map(), "apparatus:test:a", "a", &result, &s).unwrap().local_completed);
}

#[test]
fn local_completion_ignores_active_peer_and_open_upstream_but_not_available_input() {
    for source in [OrderRunStatus::RollDetached, OrderRunStatus::Completed] {
        for peer in [OrderRunStatus::Active, OrderRunStatus::Completed] {
            for available in [false, true] {
                let s = vec![session("source", source, Some(1)),
                    session("a", OrderRunStatus::Completed, Some(2)), session("b", peer, None)];
                let result = statuses(&s, &[StageWorkInput {
                    target_node: "b".into(), outstanding: true, available,
                    ..Default::default()
                }]);
                let own = work_control(&map(), "apparatus:test:a", "a", &result, &s).unwrap();
                assert_eq!(own.local_completed, !available);
                assert!(!own.completed, "shared completion still waits for the peer and all input");
                assert!(!work_control(&map(), "apparatus:test:b", "b", &result, &s).unwrap().local_completed);
            }
        }
    }
}

#[test]
fn local_completion_requires_own_finished_execution_and_its_accepted_report() {
    let mut s = vec![session("source", OrderRunStatus::Completed, Some(1)),
        session("a", OrderRunStatus::Active, None), session("b", OrderRunStatus::Completed, Some(2))];
    for state in [OrderRunStatus::Active, OrderRunStatus::Paused, OrderRunStatus::RollDetached, OrderRunStatus::Completed] {
        s[1].status = state;
        let result = statuses(&s, &[]);
        assert!(!work_control(&map(), "apparatus:test:a", "a", &result, &s).unwrap().local_completed);
    }
    s.remove(1);
    let result = statuses(&s, &[]);
    assert!(!work_control(&map(), "apparatus:test:a", "a", &result, &s).unwrap().local_completed,
        "an unused candidate must not inherit a peer's report");
}

#[test]
fn retry_does_not_steal_last_report_position() {
    let mut a = session("a", OrderRunStatus::Completed, Some(1));
    stamp_work_report(&mut a, "retry", 99, &QueueActionActor::default(), 100);
    assert_eq!(work_report(&a).unwrap().sequence, 1);
    let result = statuses(
        &[
            session("source", OrderRunStatus::Completed, Some(3)),
            a,
            session("b", OrderRunStatus::Completed, Some(2)),
        ],
        &[],
    );
    assert_eq!(shared(&result).last_apparatus, "apparatus:test:b");
}

#[test]
fn audit_during_active_work_does_not_finish_it() {
    let mut a = session("a", OrderRunStatus::Active, None);
    stamp_work_report(&mut a, "audit", 1, &QueueActionActor::default(), 10);
    assert!(work_report(&a).is_none());
    assert_eq!(a.status, OrderRunStatus::Active);
}
