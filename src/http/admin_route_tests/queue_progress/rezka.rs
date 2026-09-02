use super::*;

async fn queue_action_json(
    router: &axum::Router,
    token: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            token,
            &body.to_string(),
        ))
        .await
        .expect("queue action");
    let status = response.status();
    (status, json_body(response).await)
}

#[tokio::test]
async fn rezka_complete_requires_or_persists_progress_metrics() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests.clone(),
        fail: false,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-rezka-complete".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:asset-010".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-rezka-complete").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-rezka-complete",
                "Rezka complete order",
                "9325",
                "apparatus:default:asset-010",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-complete",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let missing_metrics = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-complete",
                "action":"complete",
                "produced_qty":32,
                "gross_qty":32,
                "uom":"kg"
            }"#,
        ))
        .await
        .expect("complete without rezka metrics");
    assert_eq!(missing_metrics.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_metrics).await["error"],
        "rezka_progress_metrics_required"
    );

    let missing_quantity = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-complete",
                "action":"complete",
                "produced_qty":32,
                "total_waste":1,
                "uom":"kg"
            }"#,
        ))
        .await
        .expect("complete without rezka weight");
    assert_eq!(missing_quantity.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_quantity).await["error"],
        "rezka_progress_metrics_required"
    );

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-complete",
                "action":"complete",
                "produced_qty":32,
                "gross_qty":32,
                "uom":"kg",
                "rezka_bosma_waste":1.25,
                "rezka_lamination_waste":2.5,
                "rezka_edge_waste":0.75,
                "diameter":45.5,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("complete with rezka metrics");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body:?}");
    assert_eq!(
        completed_body["states"]["zakaz-rezka-complete"],
        "completed"
    );
    assert_eq!(completed_body["progress_batch"]["rezka_bosma_waste"], 1.25);
    assert_eq!(
        completed_body["progress_batch"]["rezka_lamination_waste"],
        2.5
    );
    assert_eq!(completed_body["progress_batch"]["rezka_edge_waste"], 0.75);
    assert_eq!(completed_body["progress_batch"]["diameter"], 45.5);
    assert_eq!(completed_body["progress_event"]["rezka_edge_waste"], 0.75);
    assert_eq!(completed_body["progress_event"]["diameter"], 45.5);
    assert_eq!(
        completed_body["progress_batches"].as_array().unwrap().len(),
        4
    );
    assert_eq!(completed_body["prints"].as_array().unwrap().len(), 4);
    wait_for_progress_print_request_count(&print_requests, 4).await;
    let printed = print_requests.lock().await;
    assert_eq!(printed.len(), 4);
    assert_eq!(printed[0].gross_qty, 32.0);
}

#[tokio::test]
async fn rezka_explicit_frame_metrics_are_persisted_and_printed_individually() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests.clone(),
        fail: false,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-rezka-frame-values".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:asset-010".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(
        &state,
        PrincipalRole::Aparatchi,
        "worker-rezka-frame-values",
    )
    .await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-rezka-frame-values",
                "Rezka frame values",
                "9328",
                "apparatus:default:asset-010",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-frame-values",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let mismatched_frames = serde_json::json!({
        "apparatus": "apparatus:default:asset-010",
        "order_id": "zakaz-rezka-frame-values",
        "action": "complete",
        "rezka_frames": [
            {"produced_qty": 90.0, "gross_qty": 11.0, "diameter": 45.1},
            {"produced_qty": 80.0, "gross_qty": 10.0, "diameter": 44.8},
            {"produced_qty": 70.0, "gross_qty": 9.0, "diameter": 44.2}
        ]
    })
    .to_string();
    let mismatch = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &mismatched_frames,
        ))
        .await
        .expect("reject mismatched frame count");
    assert_eq!(mismatch.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(mismatch).await["error"],
        "rezka_frame_count_mismatch"
    );

    let frame_request = serde_json::json!({
        "apparatus": "apparatus:default:asset-010",
        "order_id": "zakaz-rezka-frame-values",
        "action": "complete",
        "printer": "zebra",
        "print_mode": "rfid",
        "description": "Kadrlar bo'yicha yakuniy o'lchovlar",
        "rezka_bosma_waste": 5.4,
        "total_waste": 5.4,
        "rezka_frames": [
            {
                "produced_qty": 90.0,
                "gross_qty": 11.0,
                "finished_goods_kg": 10.6,
                "finished_goods_meter": 89.0,
                "bobina_kg": 0.3,
                "diameter": 45.1
            },
            {
                "produced_qty": 80.0,
                "gross_qty": 10.0,
                "finished_goods_kg": 9.2,
                "finished_goods_meter": 78.0,
                "bobina_kg": 0.2,
                "diameter": 44.8
            },
            {
                "produced_qty": 70.0,
                "gross_qty": 9.0,
                "finished_goods_kg": 8.1,
                "finished_goods_meter": 68.0,
                "bobina_kg": 0.4,
                "diameter": 44.2
            },
            {
                "produced_qty": 60.0,
                "gross_qty": 8.0,
                "finished_goods_kg": 7.1,
                "finished_goods_meter": 57.0,
                "bobina_kg": 0.1,
                "diameter": 43.9
            }
        ]
    })
    .to_string();
    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &frame_request,
        ))
        .await
        .expect("complete with explicit frame metrics");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body:?}");
    assert!(completed_body["completion_request"].is_null());

    let expected = [
        (90.0, 11.0, 10.6, 89.0, 0.3, 45.1),
        (80.0, 10.0, 9.2, 78.0, 0.2, 44.8),
        (70.0, 9.0, 8.1, 68.0, 0.4, 44.2),
        (60.0, 8.0, 7.1, 57.0, 0.1, 43.9),
    ];
    let batches = completed_body["progress_batches"]
        .as_array()
        .expect("frame batches");
    assert_eq!(batches.len(), expected.len());
    assert_eq!(
        completed_body["prints"].as_array().unwrap().len(),
        expected.len()
    );
    for (index, batch) in batches.iter().enumerate() {
        let (produced_qty, gross_qty, finished_goods_kg, finished_goods_meter, bobina_kg, diameter) =
            expected[index];
        assert_eq!(batch["payload_json"]["rezka_frame_index"], index as u64 + 1);
        assert_eq!(batch["payload_json"]["rezka_frame_count"], 4);
        assert_eq!(batch["payload_json"]["rezka_metrics_owner"], true);
        assert_eq!(batch["produced_qty"], produced_qty);
        assert_eq!(batch["finished_goods_kg"], finished_goods_kg);
        assert_eq!(batch["finished_goods_meter"], finished_goods_meter);
        assert_eq!(batch["bobina_kg"], bobina_kg);
        assert_eq!(batch["diameter"], diameter);
        if index == 0 {
            assert_eq!(batch["rezka_bosma_waste"], 5.4);
            assert_eq!(batch["total_waste"], 5.4);
        } else {
            assert!(batch["rezka_bosma_waste"].is_null());
            assert!(batch["total_waste"].is_null());
        }
        assert_eq!(batch["payload_json"]["gross_qty"], gross_qty);
    }

    wait_for_progress_print_request_count(&print_requests, expected.len()).await;
    let printed = print_requests.lock().await;
    assert_eq!(printed.len(), expected.len());
    for batch in batches {
        let qr_payload = batch["qr_payload"].as_str().expect("frame qr");
        let request = printed
            .iter()
            .find(|request| request.epc == qr_payload)
            .expect("print request for frame qr");
        let index = batches
            .iter()
            .position(|candidate| candidate["qr_payload"] == batch["qr_payload"])
            .expect("frame index");
        assert_eq!(request.gross_qty, expected[index].1);
        assert_eq!(request.qty, Some(expected[index].3));
        assert_eq!(request.progress_unit, "m");
        assert_eq!(request.print_count, 1);
    }

    drop(printed);

    let persisted = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/wip-batches?apparatus=apparatus%3Adefault%3Aasset-010&status=all&order_id=zakaz-rezka-frame-values",
            &admin_token,
        ))
        .await
        .expect("read persisted frame batches");
    assert_eq!(persisted.status(), StatusCode::OK);
    let persisted_body = json_body(persisted).await;
    let persisted_batches = persisted_body["batches"]
        .as_array()
        .expect("persisted batches");
    for batch in batches {
        let batch_id = batch["batch_id"].as_str().expect("batch id");
        let stored = persisted_batches
            .iter()
            .find(|candidate| candidate["batch_id"] == batch_id)
            .expect("stored frame batch");
        assert_eq!(stored["diameter"], batch["diameter"]);
        assert_eq!(stored["bobina_kg"], batch["bobina_kg"]);
        assert_eq!(stored["produced_qty"], batch["produced_qty"]);
    }
}

#[tokio::test]
async fn rezka_roll_complete_skips_issue_frame_qr_and_persists_issue() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests.clone(),
        fail: false,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-rezka-frame-issue".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:asset-010".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "worker-rezka-frame-issue").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-rezka-frame-issue",
                "Rezka frame issue",
                "9329",
                "apparatus:default:asset-010",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-frame-issue",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let roll_completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-frame-issue",
                "action":"roll_complete",
                "rezka_frames":[
                    {"produced_qty":90,"gross_qty":11,"diameter":45.1},
                    {"produced_qty":80,"gross_qty":10,"diameter":44.8},
                    {"issue_note":"Rezka valida muammo, uchinchi kadr chiqarilmadi"},
                    {"produced_qty":70,"gross_qty":9,"diameter":44.2}
                ],
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("roll complete with one issue frame");
    let status = roll_completed.status();
    let body = json_body(roll_completed).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["states"]["zakaz-rezka-frame-issue"], "in_progress");
    assert_eq!(body["progress_batches"].as_array().unwrap().len(), 3);
    assert_eq!(body["prints"].as_array().unwrap().len(), 3);
    assert_eq!(
        body["progress_event"]["payload_json"]["rezka_frame_issues"][0]["frame_index"],
        3
    );
    assert_eq!(
        body["progress_event"]["payload_json"]["rezka_frame_issues"][0]["issue_note"],
        "Rezka valida muammo, uchinchi kadr chiqarilmadi"
    );
    let frame_indexes = body["progress_batches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|batch| batch["payload_json"]["rezka_frame_index"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(frame_indexes, [1, 2, 4]);
    wait_for_progress_print_request_count(&print_requests, 3).await;
    assert_eq!(print_requests.lock().await.len(), 3);

    let all_issue = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-frame-issue",
                "action":"roll_complete",
                "rezka_frames":[
                    {"issue_note":"Birinchi kadr ham brak"},
                    {"issue_note":"Ikkinchi kadr ham brak"},
                    {"issue_note":"Uchinchi kadr ham brak"},
                    {"issue_note":"To‘rtinchi kadr ham brak"}
                ]
            }"#,
        ))
        .await
        .expect("roll complete with all issue frames");
    let all_issue_status = all_issue.status();
    let all_issue_body = json_body(all_issue).await;
    assert_eq!(all_issue_status, StatusCode::OK, "{all_issue_body:?}");
    assert_eq!(
        all_issue_body["states"]["zakaz-rezka-frame-issue"],
        "in_progress"
    );
    assert!(all_issue_body["progress_batch"].is_null());
    assert_eq!(
        all_issue_body["progress_batches"].as_array().unwrap().len(),
        0
    );
    assert_eq!(all_issue_body["prints"].as_array().unwrap().len(), 0);
    assert_eq!(
        all_issue_body["progress_event"]["payload_json"]["rezka_frame_issues"]
            .as_array()
            .unwrap()
            .len(),
        4
    );

    let final_issue = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-frame-issue",
                "action":"complete",
                "description":"Rezka rulonini chiqarib bo‘lmadi",
                "rezka_frames":[
                    {"issue_note":"Birinchi kadr brak"},
                    {"issue_note":"Ikkinchi kadr brak"},
                    {"issue_note":"Uchinchi kadr brak"},
                    {"issue_note":"To‘rtinchi kadr brak"}
                ]
            }"#,
        ))
        .await
        .expect("complete with all issue frames");
    let final_issue_status = final_issue.status();
    let final_issue_body = json_body(final_issue).await;
    assert_eq!(final_issue_status, StatusCode::OK, "{final_issue_body:?}");
    assert_eq!(
        final_issue_body["states"]["zakaz-rezka-frame-issue"],
        "completed"
    );
    assert_eq!(
        final_issue_body["progress_batches"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(final_issue_body["prints"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn rezka_pause_records_quantities_without_waste_and_fans_out_frames() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests.clone(),
        fail: false,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-rezka-pause".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:asset-010".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-rezka-pause").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-rezka-pause",
                "Rezka pause order",
                "9326",
                "apparatus:default:asset-010",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-pause",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let missing_metrics = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-pause",
                "action":"pause",
                "produced_qty":18,
                "uom":"kg"
            }"#,
        ))
        .await
        .expect("pause without rezka metrics");
    assert_eq!(missing_metrics.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_metrics).await["error"],
        "rezka_progress_metrics_required"
    );

    let missing_quantity = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-pause",
                "action":"pause",
                "produced_qty":18,
                "rezka_bosma_waste":1,
                "uom":"kg"
            }"#,
        ))
        .await
        .expect("pause without rezka weight");
    assert_eq!(missing_quantity.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_quantity).await["error"],
        "rezka_progress_metrics_required"
    );

    let missing_diameter = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-pause",
                "action":"pause",
                "produced_qty":18,
                "gross_qty":18,
                "rezka_bosma_waste":1,
                "uom":"kg"
            }"#,
        ))
        .await
        .expect("pause without diameter");
    assert_eq!(missing_diameter.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_diameter).await["error"],
        "rezka_progress_metrics_required"
    );

    let paused = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-pause",
                "action":"pause",
                "produced_qty":18,
                "gross_qty":18,
                "diameter":45.5,
                "uom":"kg",
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("pause with rezka metrics");
    let paused_status = paused.status();
    let paused_body = json_body(paused).await;
    assert_eq!(paused_status, StatusCode::OK, "{paused_body:?}");
    assert_eq!(paused_body["progress_batch"]["status"], "roll_detached");
    assert!(paused_body["progress_batch"]["total_waste"].is_null());
    assert_eq!(paused_body["progress_batch"]["diameter"], 45.5);
    assert_eq!(paused_body["progress_event"]["diameter"], 45.5);
    assert_eq!(paused_body["progress_batches"].as_array().unwrap().len(), 4);
    assert_eq!(paused_body["prints"].as_array().unwrap().len(), 4);

    let resumed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-pause",
                "action":"resume"
            }"#,
        ))
        .await
        .expect("resume after pause");
    assert_eq!(resumed.status(), StatusCode::OK);

    let roll_completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-010",
                "order_id":"zakaz-rezka-pause",
                "action":"roll_complete",
                "produced_qty":18,
                "gross_qty":18,
                "diameter":45.5,
                "uom":"kg",
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("roll complete after resume");
    let roll_completed_status = roll_completed.status();
    let roll_completed_body = json_body(roll_completed).await;
    assert_eq!(
        roll_completed_status,
        StatusCode::OK,
        "{roll_completed_body:?}"
    );
    assert_eq!(
        roll_completed_body["states"]["zakaz-rezka-pause"],
        "in_progress"
    );
    assert_eq!(
        roll_completed_body["progress_batch"]["action"],
        "roll_complete"
    );
    assert_eq!(roll_completed_body["progress_batch"]["diameter"], 45.5);
    assert_eq!(roll_completed_body["progress_event"]["diameter"], 45.5);
    assert_eq!(
        roll_completed_body["progress_batches"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(roll_completed_body["prints"].as_array().unwrap().len(), 4);
    wait_for_progress_print_request_count(&print_requests, 8).await;
    let printed = print_requests.lock().await;
    assert_eq!(printed.len(), 8);
}

#[tokio::test]
async fn rezka_consumes_laminatsiya_wip_and_creates_distinct_frame_wips() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests.clone(),
        fail: false,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-rezka-wip-fanout".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec![
                "apparatus:default:asset-007".to_string(),
                "apparatus:default:asset-010".to_string(),
            ],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "worker-rezka-wip-fanout").await;
    let router = build_router(state);

    let map = serde_json::json!({
        "id": "zakaz-rezka-wip-fanout",
        "product_code": "REZKA-WIP-FANOUT",
        "title": "Rezka WIP fanout order",
        "order_number": "9327",
        "nodes": [
            {"id": "start", "kind": "start", "title": "Start"},
            {"id": "laminatsiya", "kind": "apparatus", "title": "Laminatsiya", "apparatus_id": "apparatus:default:asset-007"},
            {
                "id": "rezka",
                "kind": "apparatus",
                "title": "Rezka",
                "apparatus_id": "apparatus:default:asset-010",
                "rezka_kadr_count": 4,
                "rezka_label_length": 100
            },
            {"id": "end", "kind": "end", "title": "End"}
        ],
        "edges": [
            {"from": "start", "to": "laminatsiya"},
            {"from": "laminatsiya", "to": "rezka"},
            {"from": "rezka", "to": "end"}
        ]
    })
    .to_string();
    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &map,
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);

    let laminatsiya_started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-007",
                "order_id":"zakaz-rezka-wip-fanout",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start laminatsiya");
    assert_eq!(laminatsiya_started.status(), StatusCode::OK);

    let laminatsiya_paused = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-007",
                "order_id":"zakaz-rezka-wip-fanout",
                "action":"pause",
                "produced_qty":120,
                "uom":"m"
            }"#,
        ))
        .await
        .expect("pause laminatsiya");
    assert_eq!(laminatsiya_paused.status(), StatusCode::OK);
    let laminatsiya_paused_body = json_body(laminatsiya_paused).await;
    let source_batch_id = laminatsiya_paused_body["progress_batch"]["batch_id"]
        .as_str()
        .expect("source batch id")
        .to_string();
    let source_qr = laminatsiya_paused_body["progress_batch"]["qr_payload"]
        .as_str()
        .expect("source qr")
        .to_string();

    let laminatsiya_resumed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-007",
                "order_id":"zakaz-rezka-wip-fanout",
                "action":"resume"
            }"#,
        ))
        .await
        .expect("resume laminatsiya");
    assert_eq!(laminatsiya_resumed.status(), StatusCode::OK);

    let second_laminatsiya_completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-007",
                "order_id":"zakaz-rezka-wip-fanout",
                "action":"complete",
                "finished_goods_meter":80,
                "finished_goods_kg":10,
                "lamination_film_leftover_rolls":1,
                "total_waste":1,
                "uom":"m"
            }"#,
        ))
        .await
        .expect("complete final laminatsiya roll");
    assert_eq!(second_laminatsiya_completed.status(), StatusCode::OK);
    let second_laminatsiya_completed_body = json_body(second_laminatsiya_completed).await;
    let second_source_batch_id = second_laminatsiya_completed_body["progress_batch"]["batch_id"]
        .as_str()
        .expect("second source batch id")
        .to_string();
    let second_source_qr = second_laminatsiya_completed_body["progress_batch"]["qr_payload"]
        .as_str()
        .expect("second source qr")
        .to_string();
    wait_for_progress_print_request_count(&print_requests, 2).await;
    let print_request_count_before_rezka = print_requests.lock().await.len();

    let rezka_started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-010",
                    "order_id":"zakaz-rezka-wip-fanout",
                    "action":"start",
                    "qr_payload":"{source_qr}"
                }}"#
            ),
        ))
        .await
        .expect("start rezka from laminatsiya wip");
    assert_eq!(rezka_started.status(), StatusCode::OK);

    let queue_snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
        ))
        .await
        .expect("rezka queue controls");
    let queue_snapshot_body = json_body(queue_snapshot).await;
    let action_control = &queue_snapshot_body["queue_action_controls"]["apparatus:default:asset-010"]
        ["zakaz-rezka-wip-fanout"];
    let allowed_actions = action_control["allowed_actions"]
        .as_array()
        .expect("allowed rezka actions");
    assert!(allowed_actions.iter().any(|action| action == "complete"));
    assert!(allowed_actions.iter().any(|action| action == "merge"));
    assert!(
        allowed_actions
            .iter()
            .any(|action| action == "roll_complete")
    );
    assert_eq!(action_control["complete_requires_full_report"], false);

    let premature_source_switch = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-010",
                    "order_id":"zakaz-rezka-wip-fanout",
                    "action":"complete",
                    "produced_qty":90,
                    "gross_qty":11,
                    "diameter":45.5,
                    "uom":"m",
                    "qr_payload":"{second_source_qr}"
                }}"#
            ),
        ))
        .await
        .expect("reject switching active rezka source");
    assert_eq!(premature_source_switch.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(premature_source_switch).await["error"],
        "progress_batch_not_accepted"
    );

    let (missing_merge_input_status, missing_merge_input_body) = queue_action_json(
        &router,
        &worker_token,
        serde_json::json!({
            "apparatus": "apparatus:default:asset-010",
            "order_id": "zakaz-rezka-wip-fanout",
            "action": "merge"
        }),
    )
    .await;
    assert_eq!(missing_merge_input_status, StatusCode::BAD_REQUEST);
    assert_eq!(missing_merge_input_body["error"], "merge_input_required");

    let (merge_with_output_metric_status, merge_with_output_metric_body) = queue_action_json(
        &router,
        &worker_token,
        serde_json::json!({
            "apparatus": "apparatus:default:asset-010",
            "order_id": "zakaz-rezka-wip-fanout",
            "action": "merge",
            "qr_payload": second_source_qr,
            "diameter": 45.5
        }),
    )
    .await;
    assert_eq!(merge_with_output_metric_status, StatusCode::BAD_REQUEST);
    assert_eq!(
        merge_with_output_metric_body["error"],
        "progress_input_invalid"
    );

    let merged = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-010",
                    "order_id":"zakaz-rezka-wip-fanout",
                    "action":"merge",
                    "qr_payload":"{second_source_qr}",
                    "total_waste":0.2,
                    "uom":"kg"
                }}"#
            ),
        ))
        .await
        .expect("merge second laminatsiya WIP into active Rezka roll");
    let merged_status = merged.status();
    let merged_body = json_body(merged).await;
    assert_eq!(merged_status, StatusCode::OK, "{merged_body:?}");
    assert_eq!(
        merged_body["states"]["zakaz-rezka-wip-fanout"],
        "in_progress"
    );
    assert_eq!(merged_body["progress_event"]["action"], "merge");
    assert_eq!(merged_body["progress_event"]["total_waste"], 0.2);
    assert!(merged_body["progress_event"]["diameter"].is_null());
    assert_eq!(
        merged_body["progress_event"]["payload_json"]["material_balance"]["processed_input_batch_id"],
        source_batch_id
    );
    assert_eq!(
        merged_body["progress_event"]["payload_json"]["material_balance"]["processed_input_meter"],
        120.0
    );
    assert!(
        merged_body["progress_event"]["payload_json"]["material_balance"]["processed_input_net_kg"]
            .is_null()
    );
    assert_eq!(
        merged_body["progress_event"]["payload_json"]["material_balance"]["splice_waste_kg"],
        0.2
    );
    assert_eq!(
        merged_body["progress_event"]["payload_json"]["material_balance"]["output_measurement_deferred"],
        true
    );
    assert!(
        merged_body["session"]["payload_json"]["input_lineage"]
            .as_array()
            .unwrap()
            .iter()
            .any(|link| {
                link["input_batch_id"] == source_batch_id && link["status"] == "processed"
            })
    );
    assert!(
        merged_body["session"]["payload_json"]["input_lineage"]
            .as_array()
            .unwrap()
            .iter()
            .any(|link| {
                link["input_batch_id"] == second_source_batch_id && link["status"] == "in_use"
            })
    );
    assert!(
        merged_body["session"]["payload_json"]["rezka_active_partial_rolls"]
            .as_array()
            .unwrap()
            .iter()
            .all(|roll| {
                roll["source_input_batch_ids"]
                    .as_array()
                    .is_some_and(|sources| {
                        sources
                            .iter()
                            .any(|source| source.as_str() == Some(source_batch_id.as_str()))
                            && sources.iter().any(|source| {
                                source.as_str() == Some(second_source_batch_id.as_str())
                            })
                    })
            })
    );
    let merged_snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
        ))
        .await
        .expect("merged Rezka queue control");
    let merged_snapshot_body = json_body(merged_snapshot).await;
    let merged_control = &merged_snapshot_body["queue_action_controls"]["apparatus:default:asset-010"]
        ["zakaz-rezka-wip-fanout"];
    assert_eq!(
        merged_control["rezka_input_lineage"],
        merged_body["session"]["payload_json"]["input_lineage"]
    );
    assert_eq!(
        merged_control["rezka_active_partial_rolls"],
        merged_body["session"]["payload_json"]["rezka_active_partial_rolls"]
    );
    assert_eq!(
        print_requests.lock().await.len(),
        print_request_count_before_rezka
    );

    let duplicate_merge = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-010",
                    "order_id":"zakaz-rezka-wip-fanout",
                    "action":"merge",
                    "qr_payload":"{second_source_qr}"
                }}"#
            ),
        ))
        .await
        .expect("reject duplicate merge");
    assert_eq!(duplicate_merge.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(duplicate_merge).await["error"],
        "merge_input_same"
    );

    let (already_used_status, already_used_body) = queue_action_json(
        &router,
        &worker_token,
        serde_json::json!({
            "apparatus": "apparatus:default:asset-010",
            "order_id": "zakaz-rezka-wip-fanout",
            "action": "merge",
            "qr_payload": source_qr
        }),
    )
    .await;
    assert_eq!(already_used_status, StatusCode::CONFLICT);
    assert_eq!(already_used_body["error"], "merge_input_already_used");

    let final_completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-010",
                    "order_id":"zakaz-rezka-wip-fanout",
                    "action":"complete",
                    "produced_qty":170,
                    "gross_qty":21,
                    "diameter":45.5,
                    "uom":"m",
                    "qr_payload":"{second_source_qr}",
                    "rezka_bosma_waste":1.25,
                    "rezka_lamination_waste":2.5,
                    "rezka_edge_waste":0.75,
                    "printer":"zebra",
                    "print_mode":"rfid"
                }}"#
            ),
        ))
        .await
        .expect("complete final rezka roll");
    let final_completed_status = final_completed.status();
    let final_completed_body = json_body(final_completed).await;
    assert_eq!(
        final_completed_status,
        StatusCode::OK,
        "{final_completed_body:?}"
    );
    assert_eq!(
        final_completed_body["states"]["zakaz-rezka-wip-fanout"],
        "completed"
    );
    let final_output_batches = final_completed_body["progress_batches"]
        .as_array()
        .expect("final frame batches");
    assert_eq!(final_output_batches.len(), 4);
    assert_eq!(final_completed_body["prints"].as_array().unwrap().len(), 4);
    assert!(final_output_batches.iter().all(|batch| {
        batch["parent_batch_id"] == second_source_batch_id
            && batch["produced_qty"] == 170.0
            && batch["payload_json"]["source_input_links"]
                .as_array()
                .is_some_and(|links| {
                    links.len() == 2
                        && links
                            .iter()
                            .any(|link| link["input_batch_id"] == source_batch_id)
                        && links
                            .iter()
                            .any(|link| link["input_batch_id"] == second_source_batch_id)
                })
    }));
    assert_eq!(final_output_batches[0]["rezka_bosma_waste"], 1.25);
    assert_eq!(final_output_batches[0]["diameter"], 45.5);
    assert!(
        final_output_batches[1..]
            .iter()
            .all(|batch| batch["rezka_bosma_waste"].is_null())
    );
    assert!(
        final_completed_body["session"]["payload_json"]["rezka_active_partial_rolls"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );

    let source_status = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/wip-batches?apparatus=apparatus%3Adefault%3Aasset-010&status=all&order_id=zakaz-rezka-wip-fanout",
            &admin_token,
        ))
        .await
        .expect("list rezka lineage");
    let source_status_body = json_body(source_status).await;
    assert!(
        source_status_body["batches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|batch| {
                batch["batch_id"] == source_batch_id && batch["wip_status"] == "processed"
            })
    );
    assert!(
        source_status_body["batches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|batch| {
                batch["batch_id"] == second_source_batch_id && batch["wip_status"] == "processed"
            })
    );
    wait_for_progress_print_request_count(&print_requests, print_request_count_before_rezka + 4)
        .await;
    assert_eq!(
        print_requests.lock().await.len(),
        print_request_count_before_rezka + 4
    );
}

#[tokio::test]
async fn grouped_rezka_wip_survives_lamination_and_reenters_same_final_rezka() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-rezka-group-reentry".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec![
                "apparatus:default:asset-007".to_string(),
                "apparatus:default:asset-008".to_string(),
                "apparatus:default:asset-010".to_string(),
            ],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(
        &state,
        PrincipalRole::Aparatchi,
        "worker-rezka-group-reentry",
    )
    .await;
    let router = build_router(state);
    let order_id = "zakaz-rezka-group-reentry";

    let map = serde_json::json!({
        "id": order_id,
        "product_code": "REZKA-GROUP-REENTRY",
        "title": "Rezka group reentry",
        "order_number": "9340",
        "nodes": [
            {"id": "start", "kind": "start", "title": "Start"},
            {"id": "lamination_before", "kind": "apparatus", "title": "Laminatsiya 1", "apparatus_id": "apparatus:default:asset-007"},
            {"id": "rezka_before_lamination", "kind": "apparatus", "title": "Rezka", "apparatus_id": "apparatus:default:asset-010", "rezka_kadr_count": 3, "rezka_frame_groups": [1, 2]},
            {"id": "lamination_after", "kind": "apparatus", "title": "Laminatsiya 2", "apparatus_id": "apparatus:default:asset-008"},
            {"id": "rezka_final", "kind": "apparatus", "title": "Rezka", "apparatus_id": "apparatus:default:asset-010", "rezka_kadr_count": 3},
            {"id": "end", "kind": "end", "title": "End"}
        ],
        "edges": [
            {"from": "start", "to": "lamination_before"},
            {"from": "lamination_before", "to": "rezka_before_lamination"},
            {"from": "rezka_before_lamination", "to": "lamination_after"},
            {"from": "lamination_after", "to": "rezka_final"},
            {"from": "rezka_final", "to": "end"}
        ]
    });
    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &map.to_string(),
        ))
        .await
        .expect("save repeated Rezka map");
    assert_eq!(saved.status(), StatusCode::OK);

    let (status, body) = queue_action_json(
        &router,
        &worker_token,
        serde_json::json!({
            "apparatus": "apparatus:default:asset-007",
            "order_id": order_id,
            "action": "start"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");

    let (status, body) = queue_action_json(
        &router,
        &worker_token,
        serde_json::json!({
            "apparatus": "apparatus:default:asset-007",
            "order_id": order_id,
            "action": "complete",
            "finished_goods_meter": 120,
            "finished_goods_kg": 12,
            "lamination_film_leftover_rolls": 1,
            "total_waste": 0.5,
            "uom": "m"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let opening_qr = body["progress_batch"]["qr_payload"]
        .as_str()
        .expect("opening grouped Rezka source QR")
        .to_string();

    let snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
        ))
        .await
        .expect("intermediate Rezka controls");
    let snapshot = json_body(snapshot).await;
    let first_rezka_control =
        &snapshot["queue_action_controls"]["apparatus:default:asset-010"][order_id];
    assert_eq!(
        first_rezka_control["stage_node_id"],
        "rezka_before_lamination"
    );
    assert_eq!(
        first_rezka_control["rezka_output_kadr_counts"],
        serde_json::json!([1, 2])
    );

    let (status, body) = queue_action_json(
        &router,
        &worker_token,
        serde_json::json!({
            "apparatus": "apparatus:default:asset-010",
            "order_id": order_id,
            "action": "start",
            "qr_payload": opening_qr
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");

    let active_intermediate_snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
        ))
        .await
        .expect("active intermediate Rezka controls");
    let active_intermediate_snapshot = json_body(active_intermediate_snapshot).await;
    let active_intermediate_control = &active_intermediate_snapshot["queue_action_controls"]["apparatus:default:asset-010"]
        [order_id];
    assert_eq!(
        active_intermediate_control["complete_requires_full_report"],
        false
    );
    assert_eq!(
        active_intermediate_control["complete_requires_rezka_total_waste_only"],
        true
    );

    let (status, body) = queue_action_json(
        &router,
        &worker_token,
        serde_json::json!({
            "apparatus": "apparatus:default:asset-010",
            "order_id": order_id,
            "action": "complete",
            "uom": "m",
            "rezka_frames": [
                {"produced_qty": 120, "gross_qty": 12, "diameter": 45},
                {"produced_qty": 120, "gross_qty": 12, "diameter": 45}
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
    assert_eq!(body["error"], "rezka_progress_metrics_required");

    let (status, body) = queue_action_json(
        &router,
        &worker_token,
        serde_json::json!({
            "apparatus": "apparatus:default:asset-010",
            "order_id": order_id,
            "action": "complete",
            "total_waste": 0.3,
            "rezka_frames": [
                {"produced_qty": 120, "gross_qty": 12, "diameter": 45},
                {"produced_qty": 120, "gross_qty": 12, "diameter": 45},
                {"produced_qty": 120, "gross_qty": 12, "diameter": 45}
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
    assert_eq!(body["error"], "rezka_frame_count_mismatch");
    let (status, body) = queue_action_json(
        &router,
        &worker_token,
        serde_json::json!({
            "apparatus": "apparatus:default:asset-010",
            "order_id": order_id,
            "action": "complete",
            "uom": "m",
            "total_waste": 0.3,
            "rezka_frames": [
                {"produced_qty": 120, "gross_qty": 12, "diameter": 45},
                {"produced_qty": 120, "gross_qty": 12, "diameter": 45}
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let grouped_outputs = body["progress_batches"]
        .as_array()
        .expect("grouped Rezka outputs");
    assert_eq!(grouped_outputs.len(), 2);
    assert_eq!(
        grouped_outputs
            .iter()
            .map(|batch| batch["payload_json"]["contained_kadr_count"]
                .as_u64()
                .expect("contained kadr count"))
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(grouped_outputs.iter().all(|batch| {
        batch["payload_json"]["rezka_output_kind"] == "grouped_roll"
            && batch["payload_json"]["next_stage_node_id"] == "lamination_after"
            && batch["wip_status"] == "waiting"
            && batch["status_detail"]["flow_status"] == "waiting_next_stage"
    }));
    let grouped_qrs = grouped_outputs
        .iter()
        .map(|batch| {
            batch["qr_payload"]
                .as_str()
                .expect("grouped QR")
                .to_string()
        })
        .collect::<Vec<_>>();

    let intermediate_snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
        ))
        .await
        .expect("intermediate Rezka stage states");
    let intermediate_snapshot = json_body(intermediate_snapshot).await;
    assert_eq!(
        intermediate_snapshot["stage_states"][order_id]["rezka_before_lamination"],
        "completed"
    );
    assert_eq!(
        intermediate_snapshot["stage_states"][order_id]["lamination_after"],
        "pending"
    );
    assert_eq!(
        intermediate_snapshot["stage_states"][order_id]["rezka_final"],
        "pending"
    );

    let mut laminated_qrs = Vec::new();
    for (index, grouped_qr) in grouped_qrs.iter().enumerate() {
        let (status, body) = queue_action_json(
            &router,
            &worker_token,
            serde_json::json!({
                "apparatus": "apparatus:default:asset-008",
                "order_id": order_id,
                "action": "start",
                "qr_payload": grouped_qr
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let (status, body) = queue_action_json(
            &router,
            &worker_token,
            serde_json::json!({
                "apparatus": "apparatus:default:asset-008",
                "order_id": order_id,
                "action": "complete",
                "finished_goods_meter": 110 - index * 10,
                "finished_goods_kg": 11 - index,
                "lamination_film_leftover_rolls": 1,
                "total_waste": 0.5,
                "uom": "m"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(
            body["progress_batch"]["payload_json"]["contained_kadr_count"],
            serde_json::json!(index + 1)
        );
        assert_eq!(
            body["progress_batch"]["payload_json"]["next_stage_node_id"],
            "rezka_final"
        );
        assert_eq!(body["progress_batch"]["wip_status"], "waiting");
        assert_eq!(
            body["progress_batch"]["status_detail"]["flow_status"],
            "waiting_next_stage"
        );
        laminated_qrs.push(
            body["progress_batch"]["qr_payload"]
                .as_str()
                .expect("laminated grouped QR")
                .to_string(),
        );
        if index == 0 {
            let partial_lamination_snapshot = router
                .clone()
                .oneshot(request(
                    "GET",
                    "/v1/mobile/admin/production-maps/sequence",
                    &admin_token,
                ))
                .await
                .expect("partial Lamination stage states");
            let partial_lamination_snapshot = json_body(partial_lamination_snapshot).await;
            assert_eq!(
                partial_lamination_snapshot["stage_states"][order_id]["lamination_after"],
                "pending"
            );
            assert_eq!(
                partial_lamination_snapshot["stage_states"][order_id]["rezka_final"],
                "pending"
            );
        }
    }

    let snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
        ))
        .await
        .expect("final Rezka reentry controls");
    let snapshot = json_body(snapshot).await;
    let final_rezka_control =
        &snapshot["queue_action_controls"]["apparatus:default:asset-010"][order_id];
    assert_eq!(final_rezka_control["state"], "pending");
    assert_eq!(final_rezka_control["stage_node_id"], "rezka_final");
    assert_eq!(
        snapshot["stage_states"][order_id]["rezka_before_lamination"],
        "completed"
    );
    assert_eq!(
        snapshot["stage_states"][order_id]["lamination_after"],
        "completed"
    );
    assert_eq!(snapshot["stage_states"][order_id]["rezka_final"], "pending");
    assert!(
        final_rezka_control["rezka_output_kadr_counts"]
            .as_array()
            .is_some_and(|counts| !counts.is_empty())
    );
    assert_ne!(
        snapshot["order_statuses"][order_id]["order_status"],
        "completed"
    );

    let (status, body) = queue_action_json(
        &router,
        &worker_token,
        serde_json::json!({
            "apparatus": "apparatus:default:asset-010",
            "order_id": order_id,
            "action": "start",
            "qr_payload": laminated_qrs[0]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let active_snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
        ))
        .await
        .expect("active one-kadr final Rezka controls");
    let active_snapshot = json_body(active_snapshot).await;
    assert_eq!(
        active_snapshot["queue_action_controls"]["apparatus:default:asset-010"][order_id]["rezka_output_kadr_counts"],
        serde_json::json!([1])
    );
    assert_eq!(
        active_snapshot["queue_action_controls"]["apparatus:default:asset-010"][order_id]["complete_requires_full_report"],
        false
    );
    assert_eq!(
        active_snapshot["queue_action_controls"]["apparatus:default:asset-010"][order_id]["complete_requires_rezka_total_waste_only"],
        false
    );
    let (status, body) = queue_action_json(
        &router,
        &worker_token,
        serde_json::json!({
            "apparatus": "apparatus:default:asset-010",
            "order_id": order_id,
            "action": "merge",
            "qr_payload": laminated_qrs[1]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
    assert_eq!(body["error"], "merge_input_frame_count_mismatch");
    assert_eq!(body["active_kadr_count"], 1);
    assert_eq!(body["scanned_kadr_count"], 2);

    let unchanged_snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
        ))
        .await
        .expect("frame-mismatched Merge leaves final Rezka unchanged");
    let unchanged_snapshot = json_body(unchanged_snapshot).await;
    let unchanged_control = &unchanged_snapshot["queue_action_controls"]
        ["apparatus:default:asset-010"][order_id];
    assert_eq!(
        unchanged_control["rezka_output_kadr_counts"],
        serde_json::json!([1])
    );
    assert_eq!(
        unchanged_control["rezka_input_lineage"]
            .as_array()
            .expect("unchanged Rezka lineage")
            .len(),
        1
    );

    let (status, body) = queue_action_json(
        &router,
        &worker_token,
        serde_json::json!({
            "apparatus": "apparatus:default:asset-010",
            "order_id": order_id,
            "action": "complete",
            "uom": "m",
            "rezka_frames": [
                {"produced_qty": 110, "gross_qty": 11, "diameter": 44}
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["progress_batches"].as_array().unwrap().len(), 1);
    assert_eq!(body["states"][order_id], "pending");

    let snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
        ))
        .await
        .expect("second final Rezka input controls");
    let snapshot = json_body(snapshot).await;
    assert_eq!(
        snapshot["queue_action_controls"]["apparatus:default:asset-010"][order_id]["rezka_output_kadr_counts"],
        serde_json::json!([1, 1])
    );

    let (status, body) = queue_action_json(
        &router,
        &worker_token,
        serde_json::json!({
            "apparatus": "apparatus:default:asset-010",
            "order_id": order_id,
            "action": "start",
            "qr_payload": laminated_qrs[1]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let active_final_snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
        ))
        .await
        .expect("active final Rezka controls");
    let active_final_snapshot = json_body(active_final_snapshot).await;
    assert_eq!(
        active_final_snapshot["queue_action_controls"]["apparatus:default:asset-010"][order_id]["complete_requires_full_report"],
        true
    );
    assert_eq!(
        active_final_snapshot["queue_action_controls"]["apparatus:default:asset-010"][order_id]["complete_requires_rezka_total_waste_only"],
        false
    );
    let (status, body) = queue_action_json(
        &router,
        &worker_token,
        serde_json::json!({
            "apparatus": "apparatus:default:asset-010",
            "order_id": order_id,
            "action": "complete",
            "uom": "m",
            "rezka_bosma_waste": 0.1,
            "rezka_lamination_waste": 0.1,
            "rezka_edge_waste": 0.1,
            "rezka_frames": [
                {"produced_qty": 100, "gross_qty": 10, "diameter": 43},
                {"produced_qty": 100, "gross_qty": 10, "diameter": 43}
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["states"][order_id], "completed");
    let final_outputs = body["progress_batches"]
        .as_array()
        .expect("final individual frames");
    assert_eq!(final_outputs.len(), 2);
    assert!(final_outputs.iter().all(|batch| {
        batch["payload_json"]["contained_kadr_count"] == 1
            && batch["payload_json"]["rezka_output_kind"] == "frame"
    }));
    let completed_snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
        ))
        .await
        .expect("completed repeated Rezka stage states");
    let completed_snapshot = json_body(completed_snapshot).await;
    assert_eq!(
        completed_snapshot["stage_states"][order_id]["rezka_final"],
        "completed"
    );
}
