use super::*;
use crate::core::production_map::ProductionMapStorePort;

async fn operational_state(store: &MemoryProductionMapStore, order: &str) -> serde_json::Value {
    serde_json::json!({
        "queue": store.apparatus_queue_states().await.unwrap(),
        "sequence": store.apparatus_sequences().await.unwrap(),
        "sessions": store.order_run_sessions_for_order(order).await.unwrap(),
        "wip": store.progress_batches_for_order(order).await.unwrap(),
        "logs": store.queue_action_logs_for_orders(&[order.to_string()]).await.unwrap(),
        "controls": store.order_control_states().await.unwrap(),
    })
}

#[tokio::test]
async fn bosma_astatka_http_reports_without_changing_running_paused_or_completed_work() {
    let mut state = test_state();
    let store = Arc::new(MemoryProductionMapStore::new());
    state.production_maps = production_map_service_with_store(&state, store.clone());
    let prints = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: prints.clone(),
        fail: false,
    }));
    let station = "apparatus:default:bosma_8";
    let order = "zakaz-bosma-astatka";
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "astatka-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![station.into()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .unwrap();
    let admin = session(&state, PrincipalRole::Admin).await;
    let worker = session_for(&state, PrincipalRole::Aparatchi, "astatka-worker").await;
    let outsider = session_for(&state, PrincipalRole::Aparatchi, "unassigned-worker").await;
    let router = build_router(state);
    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin,
            &pechat_order_map_json(order, "Bosma astatka", "BAST", station),
        ))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK);
    provision_test_qolip(&router, &admin, order).await;
    let payload = serde_json::json!({
        "apparatus": station, "order_id": order, "total_waste": 0,
        "finished_goods_meter": 80, "finished_goods_kg": 12, "bobina_kg": 1,
        "description": "Astatka hisoboti",
        "returned_paint_items": [
            {"usage":"rasxot","category":"colors","name":"Oq","values":{"Mix":9,"Oq":0,"Qora":0}},
            {"usage":"astatka","category":"colors","name":"Oq","values":{"Mix":1,"Oq":0,"Qora":0}}
        ]
    });
    let endpoint = "/v1/mobile/admin/production-maps/bosma-astatka";
    let pending = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            endpoint,
            &worker,
            &payload.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(pending.status(), StatusCode::CONFLICT);
    let forbidden = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            endpoint,
            &outsider,
            &payload.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    // Build the wrong-operation request explicitly; the admin bypasses assignment only.
    let mut wrong_apparatus = payload.clone();
    wrong_apparatus["apparatus"] = serde_json::json!("apparatus:default:asset-007");
    let rejected = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            endpoint,
            &admin,
            &wrong_apparatus.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    assert!(
        store
            .bosma_astatka_reports_for_order(order)
            .await
            .unwrap()
            .is_empty()
    );

    let mut previous_to = None;
    for (action, expected) in [
        ("start", "in_progress"),
        ("pause", "paused"),
        ("complete", "completed"),
    ] {
        if action == "complete" {
            let resumed = router
                .clone()
                .oneshot(request_with_body(
                    "POST",
                    "/v1/mobile/admin/production-maps/queue-action",
                    &worker,
                    &serde_json::json!({"apparatus":station,"order_id":order,"action":"resume"})
                        .to_string(),
                ))
                .await
                .unwrap();
            assert_eq!(resumed.status(), StatusCode::OK);
        }
        let mut command = payload.clone();
        command["action"] = serde_json::json!(action);
        command["total_waste"] = serde_json::json!(1);
        command["description"] = serde_json::json!("");
        if action != "complete" {
            command
                .as_object_mut()
                .unwrap()
                .remove("returned_paint_items");
        }
        let command = if action == "start" {
            with_test_qolip(&command.to_string(), order)
        } else {
            command.to_string()
        };
        let changed = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/admin/production-maps/queue-action",
                &worker,
                &command,
            ))
            .await
            .unwrap();
        let status = changed.status();
        let changed = json_body(changed).await;
        assert_eq!(status, StatusCode::OK, "{action}: {changed}");
        assert_eq!(changed["states"][order], expected);
        let expected_prints = match action {
            "pause" => 1,
            "complete" => 2,
            _ => 0,
        };
        wait_for_progress_print_request_count(&prints, expected_prints).await;
        let before = operational_state(&store, order).await;
        let print_count = prints.lock().await.len();
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                endpoint,
                &worker,
                &payload.to_string(),
            ))
            .await
            .unwrap();
        let status = response.status();
        let response = json_body(response).await;
        assert_eq!(status, StatusCode::OK, "{response}");
        let report = &response["report"];
        assert_eq!(report["total_waste"], 0.0);
        assert_eq!(report["returned_paint"]["sender_ref"], "astatka-worker");
        assert_eq!(report["returned_paint"]["items"][1]["values"]["Mix"], "1");
        if let Some(previous) = previous_to {
            assert_eq!(report["from_at_unix"], previous);
        }
        previous_to = Some(report["to_at_unix"].clone());
        assert_eq!(operational_state(&store, order).await, before);
        assert_eq!(prints.lock().await.len(), print_count);
    }
    assert_eq!(
        store
            .bosma_astatka_reports_for_order(order)
            .await
            .unwrap()
            .len(),
        3
    );
    for field in [
        "total_waste",
        "finished_goods_meter",
        "finished_goods_kg",
        "bobina_kg",
    ] {
        let mut invalid = payload.clone();
        invalid[field] = serde_json::json!(-1);
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                endpoint,
                &worker,
                &invalid.to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{field}");
    }
    let mut missing_paint = payload.clone();
    missing_paint["returned_paint_items"] = serde_json::json!([]);
    let response = router
        .oneshot(request_with_body(
            "POST",
            endpoint,
            &worker,
            &missing_paint.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        store
            .bosma_astatka_reports_for_order(order)
            .await
            .unwrap()
            .len(),
        3
    );
}
