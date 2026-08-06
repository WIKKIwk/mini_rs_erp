use super::*;

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
            assigned_apparatus: vec!["Rezka".to_string()],
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
                "Rezka",
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
                "apparatus":"Rezka",
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
                "apparatus":"Rezka",
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
                "apparatus":"Rezka",
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
                "apparatus":"Rezka",
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
            assigned_apparatus: vec!["Rezka".to_string()],
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
            &pechat_order_map_json("zakaz-rezka-pause", "Rezka pause order", "9326", "Rezka"),
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
                "apparatus":"Rezka",
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
                "apparatus":"Rezka",
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
                "apparatus":"Rezka",
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
                "apparatus":"Rezka",
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
                "apparatus":"Rezka",
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
    assert_eq!(paused_body["progress_batch"]["status"], "paused");
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
                "apparatus":"Rezka",
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
                "apparatus":"Rezka",
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
            assigned_apparatus: vec!["Laminatsiya".to_string(), "Rezka".to_string()],
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
            {"id": "laminatsiya", "kind": "apparatus", "title": "Laminatsiya"},
            {
                "id": "rezka",
                "kind": "apparatus",
                "title": "Rezka",
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
                "apparatus":"Laminatsiya",
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
                "apparatus":"Laminatsiya",
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
                "apparatus":"Laminatsiya",
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
                "apparatus":"Laminatsiya",
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
                    "apparatus":"Rezka",
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
    let action_control =
        &queue_snapshot_body["queue_action_controls"]["Rezka"]["zakaz-rezka-wip-fanout"];
    let allowed_actions = action_control["allowed_actions"]
        .as_array()
        .expect("allowed rezka actions");
    assert!(allowed_actions.iter().any(|action| action == "complete"));
    assert!(
        !allowed_actions
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
                    "apparatus":"Rezka",
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

    let partially_completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"Rezka",
                    "order_id":"zakaz-rezka-wip-fanout",
                    "action":"complete",
                    "produced_qty":90,
                    "gross_qty":11,
                    "diameter":45.5,
                    "uom":"m",
                    "qr_payload":"{source_qr}",
                    "printer":"zebra",
                    "print_mode":"rfid"
                }}"#
            ),
        ))
        .await
        .expect("complete current rezka WIP");
    let partially_completed_status = partially_completed.status();
    let partially_completed_body = json_body(partially_completed).await;
    assert_eq!(
        partially_completed_status,
        StatusCode::OK,
        "{partially_completed_body:?}"
    );
    assert_eq!(
        partially_completed_body["states"]["zakaz-rezka-wip-fanout"],
        "pending"
    );

    let output_batches = partially_completed_body["progress_batches"]
        .as_array()
        .expect("frame batches");
    assert_eq!(output_batches.len(), 4);
    assert_eq!(
        partially_completed_body["prints"].as_array().unwrap().len(),
        4
    );
    let output_ids = output_batches
        .iter()
        .map(|batch| batch["batch_id"].as_str().expect("frame batch id"))
        .collect::<std::collections::BTreeSet<_>>();
    let output_qrs = output_batches
        .iter()
        .map(|batch| batch["qr_payload"].as_str().expect("frame qr"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(output_ids.len(), 4);
    assert_eq!(output_qrs.len(), 4);
    assert!(output_batches.iter().all(|batch| {
        batch["batch_id"] != source_batch_id
            && batch["qr_payload"] != source_qr
            && batch["parent_batch_id"] == source_batch_id
            && batch["produced_qty"] == 90.0
            && batch["finished_goods_kg"] == 11.0
            && batch["finished_goods_meter"] == 90.0
    }));
    assert!(
        output_batches
            .iter()
            .all(|batch| { batch["status_detail"]["flow_status"] == "free_wip" })
    );
    assert_eq!(
        output_batches[0]["rezka_bosma_waste"],
        serde_json::Value::Null
    );
    assert_eq!(output_batches[0]["diameter"], 45.5);

    let second_rezka_started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"Rezka",
                    "order_id":"zakaz-rezka-wip-fanout",
                    "action":"start",
                    "qr_payload":"{second_source_qr}"
                }}"#
            ),
        ))
        .await
        .expect("start second rezka WIP");
    assert_eq!(second_rezka_started.status(), StatusCode::OK);

    let final_completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"Rezka",
                    "order_id":"zakaz-rezka-wip-fanout",
                    "action":"complete",
                    "produced_qty":80,
                    "gross_qty":10,
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
    assert_eq!(
        final_output_batches[0]["parent_batch_id"],
        second_source_batch_id
    );
    assert_eq!(final_output_batches[0]["rezka_bosma_waste"], 1.25);
    assert_eq!(final_output_batches[0]["diameter"], 45.5);
    assert!(
        final_output_batches[1..]
            .iter()
            .all(|batch| batch["rezka_bosma_waste"].is_null())
    );

    let source_status = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/wip-batches?apparatus=Rezka&status=all&order_id=zakaz-rezka-wip-fanout",
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
    wait_for_progress_print_request_count(&print_requests, print_request_count_before_rezka + 8)
        .await;
    assert_eq!(
        print_requests.lock().await.len(),
        print_request_count_before_rezka + 8
    );
}
