use super::*;

fn replay_view(revision: u64, scope: &str) -> LiveView {
    LiveView {
        dependency_snapshot: Arc::new(ProductionMapLiveSnapshot::default()),
        snapshot: Arc::new(ProductionMapLiveSnapshot::default()),
        revision,
        epoch: "epoch".into(),
        scope: scope.into(),
        customers: BTreeMap::new(),
        completed: Vec::new(),
        requests: Vec::new(),
        decisions: Vec::new(),
    }
}

#[tokio::test]
async fn same_revision_assignment_roundtrip_cannot_reuse_or_replay_other_scope() {
    use crate::config::AppConfig;
    use crate::core::admin::service::AdminService;
    use crate::core::authz::RoleAssignmentUpsert;
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        default_target_warehouse: String::new(),
        http_timeout: Duration::from_secs(2),
        session_store_path: dir.path().join("sessions.json"),
        profile_store_path: dir.path().join("profiles.json"),
        push_token_store_path: dir.path().join("push.json"),
        session_ttl_seconds: Some(3600),
        supplier_prefix: "10".into(),
        werka_prefix: "20".into(),
        werka_code: String::new(),
        werka_name: String::new(),
        werka_phone: String::new(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: String::new(),
        material_taminotchi_phone: String::new(),
        admin_phone: String::new(),
        admin_name: String::new(),
        admin_code: String::new(),
    });
    state.admin = AdminService::new(&state.config);
    state.worker_show_all_apparatus_tabs = false;
    let principal = Principal {
        role: PrincipalRole::Aparatchi,
        ref_: "scope-roundtrip".into(),
        display_name: "Worker".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let assign = |apparatus: &str| RoleAssignmentUpsert {
        principal_role: principal.role,
        principal_ref: principal.ref_.clone(),
        role_id: "aparatchi".into(),
        assigned_apparatus: vec![apparatus.into()],
        assigned_item_groups: Vec::new(),
    };
    let apparatus_a = "apparatus:default:asset-010";
    let apparatus_b = "apparatus:default:asset-007";
    let window = super::super::worker_snapshot::WorkerQueueWindow::new(
        Some(5), "", true,
    ).unwrap();
    let feed = super::super::live_hub::StateFeed::default();
    state.production_maps.notify_live();
    let revision = state.production_maps.snapshot_revision();
    let epoch = state.production_maps.snapshot_epoch().to_string();
    state.admin.upsert_role_assignment(assign(apparatus_a)).await.unwrap();
    refresh_feed(&state, &feed, &principal, false, true, None, &window, false)
        .await.unwrap();
    let scope_a = feed.data.lock().await.view.as_ref().unwrap().scope.clone();

    // The existing A-key socket follows reassignment to B without changing
    // its registry entry, exactly as its heartbeat does in production.
    state.admin.upsert_role_assignment(assign(apparatus_b)).await.unwrap();
    refresh_feed(&state, &feed, &principal, false, true, None, &window, true)
        .await.unwrap();
    let scope_b = {
        let mut data = feed.data.lock().await;
        let view = data.view.as_mut().unwrap();
        view.customers.insert("private-b-order".into(), "B".into());
        let scope = view.scope.clone();
        data.history.push(super::super::live_feed::PreparedDelta::new(
            &epoch, &scope, revision - 1, revision,
            Arc::from(r#"{"order_customers":{"upsert":{"private-b-order":"B"},"remove":[]}}"#),
        ));
        scope
    };
    assert_ne!(scope_a, scope_b);
    assert!(!delivery(&feed, &epoch, Some(revision - 1), &scope_b, true).await.resync);

    // A reconnect can now reuse the original A-key feed while the
    // production revision has not changed. Both current data and B replay
    // history must be discarded before certifying any client cursor.
    state.admin.upsert_role_assignment(assign(apparatus_a)).await.unwrap();
    assert_eq!(state.production_maps.snapshot_revision(), revision);
    refresh_feed(&state, &feed, &principal, false, true, None, &window, false)
        .await.unwrap();
    {
        let mut data = feed.data.lock().await;
        assert_eq!(data.view.as_ref().unwrap().scope, scope_a);
        assert!(!data.view.as_ref().unwrap().customers.contains_key("private-b-order"));
        assert!(data.history.recover(revision - 1, revision).is_none());
    }
    assert!(delivery(&feed, &epoch, Some(revision - 1), &scope_b, true).await.resync);
    assert!(!delivery(&feed, &epoch, Some(revision), &scope_a, true).await.resync);

    state.admin.delete_role_assignment(&principal.role, &principal.ref_).await.unwrap();
    assert!(refresh_feed(&state, &feed, &principal, false, true, None, &window, false)
        .await.is_err(), "An unassigned worker must not receive an old cached scope");
}

#[tokio::test]
async fn live_noop_is_silent_but_next_material_delta_rebases_over_proven_steps() {
    let feed = super::super::live_hub::StateFeed::default();
    {
        let mut data = feed.data.lock().await;
        data.replace(replay_view(1, "scope"), None);
        data.replace(replay_view(2, "scope"), Some(Arc::from("{}")));
    }
    let quiet = delivery(&feed, "epoch", Some(1), "scope", false).await;
    assert!(!quiet.resync);
    assert!(quiet.frames.is_empty());
    assert_eq!(quiet.revision, 1);
    {
        let mut data = feed.data.lock().await;
        data.replace(
            replay_view(4, "scope"),
            Some(Arc::from(
                r#"{"sequences":{"upsert":{"a":["order"]},"remove":[]}}"#,
            )),
        );
    }
    let recovered = delivery(&feed, "epoch", Some(1), "scope", true).await;
    assert!(!recovered.resync);
    assert_eq!(recovered.frames.len(), 1);
    let frame: Value = serde_json::from_str(recovered.frames[0].as_str()).unwrap();
    assert_eq!(frame["base_rev"], 1);
    assert_eq!(frame["rev"], 4);
    assert_eq!(frame["patch"]["sequences"]["upsert"]["a"], json!(["order"]));
    // No view was observed at revision 3, so an HTTP cursor there cannot
    // be falsely accepted as an equivalent predecessor.
    assert!(
        delivery(&feed, "epoch", Some(3), "scope", true)
            .await
            .resync
    );
}

#[tokio::test]
async fn heartbeat_checkpoint_advances_noop_without_changing_any_fields() {
    let feed = super::super::live_hub::StateFeed::default();
    {
        let mut data = feed.data.lock().await;
        data.replace(replay_view(8, "scope"), None);
        data.replace(replay_view(11, "scope"), Some(Arc::from("{}")));
    }
    let checkpoint = delivery(&feed, "epoch", Some(8), "scope", true).await;
    assert!(!checkpoint.resync);
    assert_eq!(checkpoint.revision, 11);
    let frame: Value = serde_json::from_str(checkpoint.frames[0].as_str()).unwrap();
    assert_eq!(frame["base_rev"], 8);
    assert_eq!(frame["patch"], json!({}));
    assert!(delivery(&feed, "old", Some(8), "scope", true).await.resync);
    assert!(
        delivery(&feed, "epoch", Some(8), "other-worker", true)
            .await
            .resync
    );
}

#[tokio::test]
async fn scope_change_and_oversized_delta_require_canonical_resync() {
    let feed = super::super::live_hub::StateFeed::default();
    {
        let mut data = feed.data.lock().await;
        data.replace(replay_view(1, "old-assignment"), None);
        data.replace(replay_view(2, "new-assignment"), Some(Arc::from("{}")));
    }
    assert!(
        delivery(&feed, "epoch", Some(1), "old-assignment", true)
            .await
            .resync
    );
    assert!(
        delivery(&feed, "epoch", Some(1), "new-assignment", true)
            .await
            .resync
    );
    assert!(
        !delivery(&feed, "epoch", Some(2), "new-assignment", true)
            .await
            .resync
    );
    {
        let mut data = feed.data.lock().await;
        // None is the bounded-frame fallback used for an oversized patch.
        data.replace(replay_view(3, "new-assignment"), None);
    }
    assert!(
        delivery(&feed, "epoch", Some(2), "new-assignment", true)
            .await
            .resync
    );
}

#[test]
fn preflight_relevance_preserves_shared_machine_and_cross_stage_dependencies() {
    use crate::core::production_map::{
        ProductionMapDefinition, ProductionMapSaved, compile_map,
    };
    let a = "apparatus:default:bosma_9";
    let b = "apparatus:default:asset-008";
    let c = "apparatus:default:asset-009";
    let map: ProductionMapDefinition = serde_json::from_value(json!({
        "id":"order-a", "product_code":"A", "title":"Order A",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"work","kind":"task","title":"Work", "apparatus_id":a,
             "alternative_assigned_apparatus_id":b},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"work"},{"from":"work","to":"end"}],
    }))
    .unwrap();
    let program = compile_map(&map).unwrap();
    let mut snapshot = ProductionMapLiveSnapshot::default();
    snapshot.maps.push(ProductionMapSaved { map, program });
    assert!(!preflight_outside_view(&snapshot, "order-a", c));
    assert!(!preflight_outside_view(&snapshot, "another-order", a));
    assert!(!preflight_outside_view(&snapshot, "another-order", b));
    assert!(preflight_outside_view(&snapshot, "another-order", c));
    assert!(!preflight_outside_view(
        &snapshot,
        "another-order",
        "unknown"
    ));
    snapshot.sequences.insert(c.into(), Vec::new());
    // An off-window job starting on our own machine may enter the window.
    assert!(!preflight_outside_view(&snapshot, "off-window-order", c));
}

#[test]
fn unloaded_order_routes_remain_dependencies_of_a_bounded_queue_window() {
    use crate::core::production_map::{ProductionMapDefinition, ProductionMapSaved, compile_map};
    let assigned = "apparatus:default:bosma_9";
    let upstream = "apparatus:default:asset-008";
    let mut source = ProductionMapLiveSnapshot::default();
    for index in 0..6 {
        let map: ProductionMapDefinition = serde_json::from_value(json!({
            "id":format!("order-{index}"), "product_code":"A", "title":"Order",
            "nodes":[{"id":"start","kind":"start","title":"Start"},
                {"id":"work","kind":"task","title":"Work", "apparatus_id":assigned,
                 "alternative_assigned_apparatus_id": if index == 5 {upstream} else {""}},
                {"id":"end","kind":"end","title":"End"}],
            "edges":[{"from":"start","to":"work"},{"from":"work","to":"end"}],
        })).unwrap();
        let program = compile_map(&map).unwrap();
        source.maps.push(ProductionMapSaved { map, program });
    }
    let ids = (0..6).map(|index| format!("order-{index}")).collect::<Vec<_>>();
    source.sequences.insert(assigned.into(), ids.clone());
    source.visible_order_ids.insert(assigned.into(), ids);
    let window = super::super::worker_snapshot::WorkerQueueWindow::new(Some(5), "", true).unwrap();
    let projected = window.project(&source, &[assigned.into()], &[], &BTreeMap::new());
    assert_eq!(projected.maps.len(), 5);
    assert!(preflight_outside_view(&projected, "order-5", upstream));
    // refresh_feed uses the retained pre-window source, so neither this
    // hidden target nor a competing job on its upstream machine is skipped.
    assert!(!preflight_outside_view(&source, "order-5", upstream));
    assert!(!preflight_outside_view(&source, "competing-order", upstream));
}

#[test]
fn colour_state_patch_does_not_repeat_285_unchanged_maps() {
    use crate::core::production_map::{
        ProductionMapDefinition, ProductionMapSaved, compile_map,
    };
    let maps = (0..285)
        .map(|id| {
            let map: ProductionMapDefinition = serde_json::from_value(json!({
                "id": format!("order-{id}"), "product_code": format!("PRODUCT-{id}"),
                "title": "Production order",
                "nodes": [{"id":"start", "kind":"start", "title":"Start"},
                    {"id":"work", "kind":"task", "title":"Work"},
                    {"id":"end", "kind":"end", "title":"End"}],
                "edges": [{"from":"start", "to":"work"}, {"from":"work", "to":"end"}],
            }))
            .unwrap();
            let program = compile_map(&map).unwrap();
            ProductionMapSaved { map, program }
        })
        .collect();
    let snapshot = ProductionMapLiveSnapshot {
        maps,
        sequences: BTreeMap::new(),
        sequence_versions: BTreeMap::new(),
        sequence_revisions: BTreeMap::new(),
        visible_order_ids: BTreeMap::new(),
        sequence_positions_by_apparatus: BTreeMap::new(),
        queue_states: BTreeMap::new(),
        stage_states: BTreeMap::new(),
        queue_policies: vec![],
        queue_action_controls: BTreeMap::new(),
        order_statuses: BTreeMap::new(),
        order_controls: BTreeMap::new(),
        stage_order_controls: vec![],
        frozen_orders_by_apparatus: BTreeMap::new(),
    };
    let full_bytes = serde_json::to_vec(&snapshot).unwrap().len();
    let view = |snapshot, revision| LiveView {
        dependency_snapshot: Arc::new(ProductionMapLiveSnapshot::default()),
        snapshot: Arc::new(snapshot),
        revision,
        epoch: "epoch".into(),
        scope: "scope".into(),
        customers: BTreeMap::new(),
        completed: vec![],
        requests: vec![],
        decisions: vec![],
    };
    let before = view(snapshot.clone(), 8);
    let mut changed = snapshot;
    changed.queue_states.insert(
        "apparatus".into(),
        BTreeMap::from([("order-0".into(), "print_preflight".into())]),
    );
    changed.stage_states.insert(
        "order-0".into(),
        BTreeMap::from([("print".into(), "print_preflight".into())]),
    );
    let after = view(changed, 9);
    let patch = view_patch(&before, &after);
    assert!(!patch.contains_key("maps"));
    assert!(!patch.contains_key("map_order"));
    let delta_bytes = serde_json::to_vec(&patch).unwrap().len();
    assert!(delta_bytes < 1024);
    assert!(full_bytes > delta_bytes * 100);
    eprintln!(
        "synthetic 285-map snapshot: {full_bytes} bytes; colour state patch: {delta_bytes} bytes"
    );
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
