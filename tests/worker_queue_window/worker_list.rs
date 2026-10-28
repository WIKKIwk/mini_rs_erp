use super::*;
const BASE: &str = "worker_scope=true&queue_limit=5&queue_positions=true";
const PROJECTION: &str = "worker_list_v1";

fn slim_query(base: &str) -> String {
    format!("{base}&projection={PROJECTION}")
}
fn cursor(value: &Value) -> String {
    format!(
        "if_epoch={}&if_rev={}&if_scope={}",
        value["epoch"].as_str().unwrap(),
        value["rev"],
        value["scope"].as_str().unwrap()
    )
}
fn live_query(value: &Value, projection: &str) -> String {
    format!(
        "protocol=state_delta_v2&worker_scope=true&queue_limit=5&queue_positions=true&projection={projection}&epoch={}&rev={}&scope={}",
        value["epoch"].as_str().unwrap(),
        value["rev"],
        value["scope"].as_str().unwrap()
    )
}
fn assert_slim(value: &Value) {
    assert_eq!(value["projection"], PROJECTION);
    assert!(value["worker_pending_report_ids"].is_object());
    for field in [
        "maps",
        "map_order",
        "queue_action_controls",
        "stage_states",
        "stage_order_controls",
        "order_statuses",
        "order_customers",
        "frozen_orders_by_apparatus",
        "calculation_templates",
    ] {
        assert!(value.get(field).is_none(), "heavy field {field}");
    }
    for row in value["worker_list_rows"].as_object().unwrap().values() {
        assert_eq!(row.as_object().unwrap().len(), 5 + usize::from(row.get("temporarily_paused_apparatus").is_some()));
        assert!(row.get("program").is_none());
    }
    for control in value["order_controls"].as_object().unwrap().values() {
        assert_eq!(control.as_object().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn worker_list_preserves_temporary_pause_display_through_http_and_live_resume() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let mut session = OrderRunSession {
        session_id: "unfinished-roll".into(), apparatus: CUT.into(), order_id: "zakaz-000".into(),
        stage_node_id: "work".into(), status: OrderRunStatus::Paused,
        worker_role: "aparatchi".into(), worker_ref: "window-worker".into(), worker_display_name: "Worker".into(),
        started_at_unix: 1, updated_at_unix: 2,
        payload_json: json!({"_temporary_pause_revision":1,"_temporary_pause":{"revision":1,"paused_at_unix":2}}),
    };
    f.store.put_order_run_session(session.clone()).await.unwrap();
    f.store.put_apparatus_queue_states(CUT, BTreeMap::from([("zakaz-000".into(), "paused".into())])).await.unwrap();
    f.state.production_maps.notify_live();
    let (status, before) = f.get(&slim_query(BASE)).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    assert_slim(&before);
    assert_eq!(before["worker_list_rows"]["zakaz-000"]["temporarily_paused_apparatus"], json!([CUT]));
    assert!(before["worker_list_rows"]["zakaz-001"].get("temporarily_paused_apparatus").is_none());
    assert!(!before.to_string().contains("unfinished-roll"), "display rows must not expose session authority");

    let (address, server) = start(&f).await;
    let (mut socket, upgrade) = connect(address, &f.token, &live_query(&before, PROJECTION)).await;
    assert!(upgrade.starts_with("HTTP/1.1 101"), "{upgrade}");
    assert_eq!(ws_json(&mut socket).await["resync"], false);
    session.status = OrderRunStatus::Active;
    session.updated_at_unix = 3;
    session.payload_json.as_object_mut().unwrap().remove("_temporary_pause");
    f.store.put_order_run_session(session).await.unwrap();
    f.store.put_apparatus_queue_states(CUT, BTreeMap::from([("zakaz-000".into(), "in_progress".into())])).await.unwrap();
    f.state.production_maps.notify_live();
    let delta = ws_json(&mut socket).await;
    let resumed = &delta["patch"]["worker_list_rows"]["upsert"]["zakaz-000"];
    assert_eq!(resumed["id"], "zakaz-000", "{delta}");
    assert!(resumed.get("temporarily_paused_apparatus").is_none(), "{delta}");
    assert_eq!(delta["patch"]["worker_display_states"]["scopes"][CUT]["upsert"]["zakaz-000"], "in_progress");
    drop(socket);
    server.abort();
}

#[tokio::test]
async fn worker_list_http_matches_existing_windows_ranks_and_cursors_without_domain_records() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    for limit in [0, 5, 10, 20, 50] {
        let base = format!("worker_scope=true&queue_limit={limit}&queue_positions=true");
        let (status, full) = f.get(&base).await;
        assert_eq!(status, StatusCode::OK, "{full}");
        let (status, slim) = f.get(&slim_query(&base)).await;
        assert_eq!(status, StatusCode::OK, "{slim}");
        assert_slim(&slim);
        for key in [
            "epoch",
            "rev",
            "sequences",
            "visible_order_ids",
            "sequence_versions",
            "sequence_revisions",
            "sequence_positions_by_apparatus",
            "queue_policies",
            "queue_states",
            "completed_orders",
            "completion_requests",
            "completion_request_decisions",
        ] {
            assert_eq!(slim[key], full[key], "{key}");
        }
        assert_ne!(slim["scope"], full["scope"]);
        assert_eq!(
            slim["worker_list_rows"].as_object().unwrap().len(),
            full["maps"].as_array().unwrap().len()
        );
        for saved in full["maps"].as_array().unwrap() {
            let id = saved["map"]["id"].as_str().unwrap();
            assert_eq!(slim["worker_list_rows"][id]["title"], saved["map"]["title"]);
            assert_eq!(
                slim["worker_display_states"][CUT][id],
                full["queue_states"][CUT][id]
            );
        }
        assert_eq!(
            f.get(&format!("{}&{}", slim_query(&base), cursor(&slim)))
                .await
                .0,
            StatusCode::NOT_MODIFIED
        );
        assert_eq!(
            f.get(&format!("{base}&{}", cursor(&slim))).await.0,
            StatusCode::OK
        );
        assert_eq!(
            f.get(&format!("{}&{}", slim_query(&base), cursor(&full)))
                .await
                .0,
            StatusCode::OK
        );
    }
    let (_, searched) = f
        .get(&slim_query(&format!("{BASE}&queue_search=zakaz-059")))
        .await;
    assert_eq!(searched["visible_order_ids"][CUT], json!(["zakaz-059"]));
    assert_eq!(
        searched["sequence_positions_by_apparatus"][CUT]["zakaz-059"],
        60
    );
    for bad in [
        "projection=worker_list_v1",
        "worker_scope=true&projection=unknown",
        "worker_scope=true&projection=sequence_list_v1",
    ] {
        assert_eq!(f.get(bad).await.0, StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[tokio::test]
async fn worker_list_live_state_and_graph_label_deltas_are_small_scoped_and_replayable() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let (_, before) = f.get(&slim_query(BASE)).await;
    let (address, server) = start(&f).await;
    let (mut socket, upgrade) = connect(address, &f.token, &live_query(&before, PROJECTION)).await;
    assert!(upgrade.starts_with("HTTP/1.1 101"), "{upgrade}");
    let ready = ws_json(&mut socket).await;
    assert_eq!(ready["resync"], false, "{ready}");
    assert_eq!(ready["scope"], before["scope"]);
    f.store
        .put_apparatus_queue_states(CUT, BTreeMap::from([("zakaz-000".into(), "paused".into())]))
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    let delta = ws_json(&mut socket).await;
    assert_eq!(delta["type"], "state_delta", "{delta}");
    assert_eq!(
        delta["patch"]["worker_display_states"]["scopes"][CUT]["upsert"]["zakaz-000"],
        "paused"
    );
    for field in [
        "maps",
        "queue_action_controls",
        "stage_states",
        "order_statuses",
    ] {
        assert!(delta["patch"].get(field).is_none(), "{delta}");
    }
    let mut changed = map("zakaz-000", CUT);
    changed.title = "Changed title".into();
    f.state.production_maps.upsert_map(changed).await.unwrap();
    let delta = ws_json(&mut socket).await;
    assert_eq!(
        delta["patch"]["worker_list_rows"]["upsert"]["zakaz-000"]["title"], "Changed title",
        "{delta}"
    );
    assert!(!delta.to_string().contains("\"program\""));
    let (_, after) = f.get(&slim_query(BASE)).await;
    drop(socket);
    let (mut resumed, _) = connect(address, &f.token, &live_query(&after, PROJECTION)).await;
    assert_eq!(ws_json(&mut resumed).await["resync"], false);
    // A legacy full-state cursor is not a summary baseline.
    let (_, full) = f.get(BASE).await;
    let (mut crossed, _) = connect(address, &f.token, &live_query(&full, PROJECTION)).await;
    let ready = ws_json(&mut crossed).await;
    assert_eq!(ready["resync"], true, "{ready}");
    assert_eq!(ready["scope"], after["scope"]);
    drop(crossed);
    drop(resumed);
    server.abort();
}

#[tokio::test]
async fn worker_detail_checks_assignment_actual_order_membership_and_returns_one_canonical_order() {
    let _guard = FIXTURE_LOCK.lock().await;
    let mut f = Fixture::new().await;
    let query = format!("worker_scope=true&detail=true&apparatus={CUT}&order_id=zakaz-000");
    let (status, detail) = f.get(&query).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["projection"], "worker_detail_v1");
    assert_eq!(detail["maps"].as_array().unwrap().len(), 1);
    assert_eq!(detail["maps"][0]["map"]["id"], "zakaz-000");
    assert!(detail["queue_action_controls"][CUT]["zakaz-000"].is_object());
    assert_eq!(detail["apparatus"], CUT);
    assert_eq!(detail["order_id"], "zakaz-000");
    assert!(!detail["scope"].as_str().unwrap().is_empty());
    assert_eq!(
        f.get(&format!(
            "worker_scope=true&detail=true&apparatus={OTHER}&order_id=zakaz-unrelated"
        ))
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.get(&format!(
            "worker_scope=true&detail=true&apparatus={CUT}&order_id=zakaz-unrelated"
        ))
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.get("worker_scope=true&detail=true&order_id=zakaz-000")
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    f.state.worker_show_all_apparatus_tabs = true;
    f.router = build_router(f.state.clone());
    let (status, watch) = f
        .get(&format!(
            "worker_scope=true&detail=true&apparatus={OTHER}&order_id=zakaz-unrelated"
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "{watch}");
    assert_eq!(watch["maps"].as_array().unwrap().len(), 1);
    assert_eq!(watch["maps"][0]["map"]["id"], "zakaz-unrelated");
    assert_eq!(
        f.get(&format!(
            "worker_scope=true&detail=true&apparatus={CUT}&order_id=absent"
        ))
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn worker_detail_history_fallback_requires_persisted_assigned_apparatus_history() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let query = format!("worker_scope=true&detail=true&apparatus={CUT}&order_id=zakaz-unrelated");
    let record = |event: &str, actor: &str, apparatus: &str| ApparatusQueueActionEvent {
        event_id: event.into(),
        apparatus: apparatus.into(),
        order_id: "zakaz-unrelated".into(),
        stage_node_id: "work".into(),
        action: queue_state::ApparatusQueueAction::Pause,
        from_state: queue_state::ApparatusQueueOrderState::InProgress,
        to_state: queue_state::ApparatusQueueOrderState::Paused,
        policy: ApparatusQueuePolicy::StrictSequence,
        actor: QueueActionActor {
            role: "aparatchi".into(),
            ref_: actor.into(),
            display_name: actor.into(),
        },
        assigned_apparatus: vec![apparatus.into()],
        payload_json: json!({}),
    };
    f.store
        .append_apparatus_queue_action_event(record("someone-else", "other-worker", OTHER))
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    assert_eq!(f.get(&query).await.0, StatusCode::NOT_FOUND);
    f.store
        .append_apparatus_queue_action_event(record("own-history", "window-worker", OTHER))
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    // Own history on an unassigned apparatus cannot authorize this CUT detail.
    assert_eq!(f.get(&query).await.0, StatusCode::NOT_FOUND);
    f.store
        .append_apparatus_queue_action_event(record("assigned-history", "previous-shift", CUT))
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    let (status, detail) = f.get(&query).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["maps"].as_array().unwrap().len(), 1);
    assert_eq!(detail["maps"][0]["map"]["id"], "zakaz-unrelated");
    let (_, full) = f.get(BASE).await;
    let (_, slim) = f.get(&slim_query(BASE)).await;
    assert_eq!(slim["completed_orders"], full["completed_orders"]);
    assert!(
        slim["worker_list_rows"]["zakaz-unrelated"].is_object(),
        "history labels stay available without loading its map"
    );
    assert!(
        !slim["visible_order_ids"][CUT]
            .as_array()
            .unwrap()
            .contains(&json!("zakaz-unrelated"))
    );
}

#[tokio::test]
async fn worker_list_pins_active_order_and_preserves_free_pick_search_semantics() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    f.store
        .put_apparatus_queue_states(
            CUT,
            BTreeMap::from([("zakaz-059".into(), "in_progress".into())]),
        )
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    let (_, full) = f.get(BASE).await;
    let (_, slim) = f.get(&slim_query(BASE)).await;
    assert_eq!(slim["visible_order_ids"], full["visible_order_ids"]);
    assert!(
        slim["visible_order_ids"][CUT]
            .as_array()
            .unwrap()
            .contains(&json!("zakaz-059"))
    );
    assert_eq!(
        slim["worker_display_states"][CUT]["zakaz-059"],
        "in_progress"
    );
    f.free_pick().await;
    let (_, full) = f.get(BASE).await;
    let (_, slim) = f.get(&slim_query(BASE)).await;
    assert_eq!(slim["visible_order_ids"], full["visible_order_ids"]);
    assert_eq!(
        slim["sequence_positions_by_apparatus"],
        full["sequence_positions_by_apparatus"]
    );
}

#[tokio::test]
async fn worker_list_wire_size_comparison_uses_same_twenty_orders() {
    use std::io::Write;
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let base = "worker_scope=true&queue_limit=20&queue_positions=true";
    let (_, full) = f.get(base).await;
    let (_, slim) = f.get(&slim_query(base)).await;
    let size = |value: &Value| {
        let raw = serde_json::to_vec(value).unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(8));
        encoder.write_all(&raw).unwrap();
        (raw.len(), encoder.finish().unwrap().len())
    };
    let (full_raw, full_gzip) = size(&full);
    let (slim_raw, slim_gzip) = size(&slim);
    eprintln!(
        "worker_list_v1 fixture=60_orders page=20 full_raw={full_raw} slim_raw={slim_raw} full_gzip8={full_gzip} slim_gzip8={slim_gzip}"
    );
    assert!(slim_raw < full_raw / 2);
    assert!(slim_gzip < full_gzip);
}
