use super::*;

#[tokio::test]
async fn admin_freeze_request_is_finalized_by_linked_worker_safe_stop() {
    let production_store = Arc::new(MemoryProductionMapStore::new());
    let mut state = test_state();
    state.production_maps = production_map_service_with_store(&state, production_store.clone());
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-laminatsiya-freeze-request".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:asset-007".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(
        &state,
        PrincipalRole::Aparatchi,
        "worker-laminatsiya-freeze-request",
    )
    .await;
    let router = build_router(state);
    let order_id = "zakaz-laminatsiya-freeze-request";

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json_with_dims(
                order_id,
                "Laminatsiya admin freeze request",
                "9320",
                "apparatus:default:asset-007",
                2,
                950.0,
            ),
        ))
        .await
        .expect("save map");
    let saved_status = saved.status();
    let saved_body = json_body(saved).await;
    assert_eq!(saved_status, StatusCode::OK, "{saved_body:?}");

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
                    "order_id":"{order_id}",
                    "action":"start"
                }}"#
            ),
        ))
        .await
        .expect("start");
    let started_status = started.status();
    let started_body = json_body(started).await;
    assert_eq!(started_status, StatusCode::OK, "{started_body:?}");
    let session_id = started_body["session"]["session_id"]
        .as_str()
        .expect("started session id")
        .to_string();

    let requested = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/order-control",
            &admin_token,
            &format!(r#"{{"order_id":"{order_id}","action":"freeze"}}"#),
        ))
        .await
        .expect("request freeze");
    let requested_status = requested.status();
    let requested_body = json_body(requested).await;
    assert_eq!(requested_status, StatusCode::OK, "{requested_body:?}");
    assert_eq!(requested_body["control"]["state"], "freeze_requested");
    assert_eq!(
        requested_body["control"]["freeze_request"]["target_session_id"],
        session_id
    );

    let snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &worker_token,
        ))
        .await
        .expect("freeze requested snapshot");
    let snapshot_status = snapshot.status();
    let snapshot_body = json_body(snapshot).await;
    assert_eq!(snapshot_status, StatusCode::OK, "{snapshot_body:?}");
    assert_eq!(
        snapshot_body["sequences"]["apparatus:default:asset-007"],
        serde_json::json!([order_id])
    );
    assert_eq!(
        snapshot_body["queue_states"]["apparatus:default:asset-007"][order_id],
        "in_progress"
    );
    assert_eq!(
        snapshot_body["queue_action_controls"]["apparatus:default:asset-007"][order_id]["freeze_request"]
            ["target_session_id"],
        session_id
    );
    let freeze_request_id = snapshot_body["queue_action_controls"]["apparatus:default:asset-007"]
        [order_id]["freeze_request"]["request_id"]
        .as_str()
        .expect("linked freeze request id")
        .to_string();

    let missing_request_id = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
                    "order_id":"{order_id}",
                    "action":"detach_roll",
                    "finished_goods_meter":100,
                    "finished_goods_kg":20,
                    "bobina_kg":2
                }}"#
            ),
        ))
        .await
        .expect("missing freeze request id");
    assert_eq!(missing_request_id.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(missing_request_id).await["error"],
        "order_freeze_request_mismatch"
    );

    let partial_output = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
                    "order_id":"{order_id}",
                    "action":"detach_roll",
                    "freeze_request_id":"{freeze_request_id}",
                    "finished_goods_meter":100,
                    "description":"partial output must roll back"
                }}"#
            ),
        ))
        .await
        .expect("partial safe-stop output");
    assert_eq!(partial_output.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(partial_output).await["error"],
        "freeze_safe_stop_output_incomplete"
    );

    let unchanged = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &worker_token,
        ))
        .await
        .expect("unchanged snapshot");
    let unchanged_body = json_body(unchanged).await;
    assert_eq!(
        unchanged_body["sequences"]["apparatus:default:asset-007"],
        serde_json::json!([order_id])
    );
    assert_eq!(
        unchanged_body["queue_states"]["apparatus:default:asset-007"][order_id],
        "in_progress"
    );
    assert_eq!(
        unchanged_body["order_controls"][order_id]["state"],
        "freeze_requested"
    );

    production_store.fail_next_queue_progress_commit();
    let failed_commit = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
                    "order_id":"{order_id}",
                    "action":"detach_roll",
                    "freeze_request_id":"{freeze_request_id}",
                    "finished_goods_meter":100,
                    "finished_goods_kg":20,
                    "bobina_kg":2,
                    "description":"controlled commit failure",
                    "print_transport":"offline"
                }}"#
            ),
        ))
        .await
        .expect("controlled failed commit");
    assert_eq!(failed_commit.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let after_failed_commit = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &worker_token,
        ))
        .await
        .expect("snapshot after failed commit");
    let after_failed_commit_body = json_body(after_failed_commit).await;
    assert_eq!(
        after_failed_commit_body["sequences"]["apparatus:default:asset-007"],
        serde_json::json!([order_id])
    );
    assert_eq!(
        after_failed_commit_body["queue_states"]["apparatus:default:asset-007"][order_id],
        "in_progress"
    );
    assert_eq!(
        after_failed_commit_body["order_controls"][order_id]["state"],
        "freeze_requested"
    );
    assert_eq!(
        after_failed_commit_body["queue_action_controls"]["apparatus:default:asset-007"][order_id]
            ["freeze_request"]["target_session_id"],
        session_id
    );

    let no_rolled_back_batch = router
        .clone()
        .oneshot(request(
            "GET",
            &format!("/v1/mobile/admin/production-maps/wip-batches?status=all&order_id={order_id}"),
            &admin_token,
        ))
        .await
        .expect("no batch after failed commit");
    assert!(
        json_body(no_rolled_back_batch).await["batches"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );

    let safe_stop = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
                    "order_id":"{order_id}",
                    "action":"detach_roll",
                    "freeze_request_id":"{freeze_request_id}",
                    "finished_goods_meter":100,
                    "finished_goods_kg":20,
                    "bobina_kg":2,
                    "description":"healthy linked safe stop",
                    "print_transport":"offline"
                }}"#
            ),
        ))
        .await
        .expect("linked safe-stop");
    let safe_stop_status = safe_stop.status();
    let safe_stop_body = json_body(safe_stop).await;
    assert_eq!(safe_stop_status, StatusCode::OK, "{safe_stop_body:?}");
    assert_eq!(safe_stop_body["states"][order_id], "frozen");
    assert_eq!(safe_stop_body["order_control"]["state"], "frozen");
    assert_eq!(safe_stop_body["session"]["status"], "frozen");
    assert_eq!(safe_stop_body["progress_event"]["action"], "detach_roll");
    assert_eq!(
        safe_stop_body["progress_batch"]["finished_goods_meter"],
        100.0
    );
    assert!(
        safe_stop_body["progress_batch"]["qr_payload"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert_eq!(safe_stop_body["prints"].as_array().map(Vec::len), Some(1));

    let frozen_snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &worker_token,
        ))
        .await
        .expect("frozen snapshot");
    let frozen_snapshot_body = json_body(frozen_snapshot).await;
    assert_eq!(
        frozen_snapshot_body["sequences"]["apparatus:default:asset-007"],
        serde_json::json!([])
    );
    assert_eq!(
        frozen_snapshot_body["queue_states"]["apparatus:default:asset-007"][order_id],
        "frozen"
    );
    assert_eq!(
        frozen_snapshot_body["frozen_orders_by_apparatus"]["apparatus:default:asset-007"][0]["order_id"],
        order_id
    );

    let completed = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/completed-orders",
            &admin_token,
        ))
        .await
        .expect("completed orders");
    assert!(
        json_body(completed).await["completed_orders"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );

    let hidden_wip = router
        .clone()
        .oneshot(request(
            "GET",
            &format!(
                "/v1/mobile/admin/production-maps/wip-batches?status=waiting&order_id={order_id}"
            ),
            &admin_token,
        ))
        .await
        .expect("hidden frozen WIP");
    assert!(
        json_body(hidden_wip).await["batches"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );

    let unfrozen = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/order-control",
            &admin_token,
            &format!(r#"{{"order_id":"{order_id}","action":"unfreeze"}}"#),
        ))
        .await
        .expect("unfreeze");
    let unfrozen_status = unfrozen.status();
    let unfrozen_body = json_body(unfrozen).await;
    assert_eq!(unfrozen_status, StatusCode::OK, "{unfrozen_body:?}");
    assert_eq!(unfrozen_body["control"]["state"], "active");

    let visible_wip = router
        .clone()
        .oneshot(request(
            "GET",
            &format!(
                "/v1/mobile/admin/production-maps/wip-batches?status=waiting&order_id={order_id}"
            ),
            &admin_token,
        ))
        .await
        .expect("visible unfrozen WIP");
    assert_eq!(
        json_body(visible_wip).await["batches"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    let refrozen = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/order-control",
            &admin_token,
            &format!(r#"{{"order_id":"{order_id}","action":"freeze"}}"#),
        ))
        .await
        .expect("refreeze requeued paused order");
    let refrozen_status = refrozen.status();
    let refrozen_body = json_body(refrozen).await;
    assert_eq!(refrozen_status, StatusCode::OK, "{refrozen_body:?}");
    assert_eq!(refrozen_body["control"]["state"], "frozen");

    let refrozen_snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &worker_token,
        ))
        .await
        .expect("snapshot after refreeze");
    let refrozen_snapshot_body = json_body(refrozen_snapshot).await;
    assert_eq!(
        refrozen_snapshot_body["queue_states"]["apparatus:default:asset-007"][order_id],
        "frozen"
    );
    assert_eq!(
        refrozen_snapshot_body["sequences"]["apparatus:default:asset-007"],
        serde_json::json!([])
    );

    let second_unfreeze = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/order-control",
            &admin_token,
            &format!(r#"{{"order_id":"{order_id}","action":"unfreeze"}}"#),
        ))
        .await
        .expect("second unfreeze");
    let second_unfreeze_status = second_unfreeze.status();
    let second_unfreeze_body = json_body(second_unfreeze).await;
    assert_eq!(
        second_unfreeze_status,
        StatusCode::OK,
        "{second_unfreeze_body:?}"
    );
    assert_eq!(second_unfreeze_body["control"]["state"], "active");

    let second_unfreeze_snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &worker_token,
        ))
        .await
        .expect("snapshot after second unfreeze");
    let second_unfreeze_snapshot_body = json_body(second_unfreeze_snapshot).await;
    assert_eq!(
        second_unfreeze_snapshot_body["queue_states"]["apparatus:default:asset-007"][order_id],
        "pending"
    );
    assert_eq!(
        second_unfreeze_snapshot_body["sequences"]["apparatus:default:asset-007"],
        serde_json::json!([order_id])
    );

    let start_again = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
                    "order_id":"{order_id}",
                    "action":"start"
                }}"#
            ),
        ))
        .await
        .expect("start existing session");
    assert_eq!(start_again.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(start_again).await["error"],
        "queue_action_not_allowed"
    );

    let resumed = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
                    "order_id":"{order_id}",
                    "action":"resume"
                }}"#
            ),
        ))
        .await
        .expect("resume existing session");
    let resumed_status = resumed.status();
    let resumed_body = json_body(resumed).await;
    assert_eq!(resumed_status, StatusCode::OK, "{resumed_body:?}");
    assert_eq!(resumed_body["session"]["session_id"], session_id);
    assert_eq!(resumed_body["session"]["status"], "active");
}

#[tokio::test]
async fn laminatsiya_complete_requires_or_persists_completion_metrics() {
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
            principal_ref: "worker-laminatsiya-complete".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:asset-007".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(
        &state,
        PrincipalRole::Aparatchi,
        "worker-laminatsiya-complete",
    )
    .await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json_with_dims(
                "zakaz-laminatsiya-complete",
                "Laminatsiya complete order",
                "9323",
                "apparatus:default:asset-007",
                2,
                950.0,
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
                "apparatus":"apparatus:default:asset-007",
                "order_id":"zakaz-laminatsiya-complete",
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
                "apparatus":"apparatus:default:asset-007",
                "order_id":"zakaz-laminatsiya-complete",
                "action":"complete"
            }"#,
        ))
        .await
        .expect("complete without metrics");
    assert_eq!(missing_metrics.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_metrics).await["error"],
        "laminatsiya_completion_metrics_required"
    );

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-007",
                "order_id":"zakaz-laminatsiya-complete",
                "action":"complete",
                "lamination_film_leftover_rolls":3.5,
                "total_waste":2.25,
                "finished_goods_kg":14.75,
                "finished_goods_meter":110.5,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("complete with metrics");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body:?}");
    assert_eq!(
        completed_body["states"]["zakaz-laminatsiya-complete"],
        "completed"
    );
    assert!(completed_body["progress_batch"]["lamination_print_leftover_rolls"].is_null());
    assert_eq!(
        completed_body["progress_batch"]["lamination_film_leftover_rolls"],
        3.5
    );
    assert_eq!(completed_body["progress_batch"]["total_waste"], 2.25);
    assert_eq!(completed_body["progress_batch"]["finished_goods_kg"], 14.75);
    assert_eq!(
        completed_body["progress_batch"]["finished_goods_meter"],
        110.5
    );
    assert_eq!(
        completed_body["progress_event"]["lamination_film_leftover_rolls"],
        3.5
    );
    wait_for_progress_print_request_count(&print_requests, 1).await;
    let printed = print_requests.lock().await;
    assert_eq!(printed.len(), 1);
    assert_eq!(printed[0].gross_qty, 14.75);
    assert_eq!(printed[0].qty, Some(110.5));
    assert_eq!(printed[0].unit, "kg");
    assert_eq!(printed[0].progress_unit, "m");
}

#[tokio::test]
async fn finished_goods_stays_free_wip_until_assigned_warehouse_accepts() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests,
        fail: false,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-fg-receipt".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:asset-007".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-fg-receipt").await;
    let warehouse_token = session_for(&state, PrincipalRole::Werka, "warehouse-keeper-1").await;
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::Werka,
        "warehouse-keeper-1",
        "Tayyor mahsulot ombori",
    )
    .await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json_with_dims(
                "zakaz-fg-receipt",
                "Finished goods receipt order",
                "9407",
                "apparatus:default:asset-007",
                2,
                950.0,
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
                "apparatus":"apparatus:default:asset-007",
                "order_id":"zakaz-fg-receipt",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-007",
                "order_id":"zakaz-fg-receipt",
                "action":"complete",
                "lamination_film_leftover_rolls":1.25,
                "total_waste":2,
                "finished_goods_kg":24,
                "finished_goods_meter":180,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("complete");
    let completed_body = json_body(completed).await;
    let qr_payload = completed_body["progress_batch"]["qr_payload"]
        .as_str()
        .expect("progress qr");
    assert_eq!(
        completed_body["progress_batch"]["wip_status"], "waiting",
        "final output must remain free WIP until a receiver accepts it"
    );
    assert_eq!(
        completed_body["progress_batch"]["status_detail"]["flow_status"],
        "free_wip"
    );
    assert!(
        completed_body["progress_batch"]["status_detail"]
            .get("stock_status")
            .is_none()
    );
    assert_eq!(completed_body["order_status"]["order_status"], "completed");
    assert_eq!(completed_body["order_status"]["flow_status"], "free_wip");

    let waiting = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/wip-batches?status=waiting&order_id=zakaz-fg-receipt",
            &admin_token,
        ))
        .await
        .expect("waiting wip");
    let waiting_body = json_body(waiting).await;
    assert_eq!(
        waiting_body["batches"].as_array().expect("waiting").len(),
        1
    );

    let worker_cannot_receive = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/finished-goods/receive",
            &worker_token,
            &format!(r#"{{"qr_payload":"{qr_payload}","warehouse":"Tayyor mahsulot ombori"}}"#),
        ))
        .await
        .expect("worker receive attempt");
    assert_eq!(worker_cannot_receive.status(), StatusCode::FORBIDDEN);

    let unassigned_warehouse = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/finished-goods/receive",
            &warehouse_token,
            &format!(r#"{{"qr_payload":"{qr_payload}","warehouse":"Boshqa ombor"}}"#),
        ))
        .await
        .expect("unassigned warehouse attempt");
    assert_eq!(unassigned_warehouse.status(), StatusCode::FORBIDDEN);

    let received = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/finished-goods/receive",
            &warehouse_token,
            &format!(
                r#"{{
                    "qr_payload":"{qr_payload}",
                    "warehouse":"Tayyor mahsulot ombori"
                }}"#
            ),
        ))
        .await
        .expect("receive finished goods");
    let received_status = received.status();
    let received_body = json_body(received).await;
    assert_eq!(received_status, StatusCode::OK, "{received_body:?}");
    assert_eq!(
        received_body["stock"]["warehouse"],
        "Tayyor mahsulot ombori"
    );
    assert_eq!(received_body["stock"]["order_id"], "zakaz-fg-receipt");
    assert_eq!(received_body["stock"]["item_code"], "PECHAT-9407");
    assert_eq!(
        received_body["stock"]["item_name"],
        "Finished goods receipt order"
    );
    assert_eq!(received_body["stock"]["qty"], 24.0);
    assert_eq!(received_body["stock"]["uom"], "kg");
    assert_eq!(
        received_body["stock"]["accepted_by_ref"],
        "warehouse-keeper-1"
    );
    assert_eq!(received_body["batch"]["wip_status"], "processed");
    assert_eq!(
        received_body["batch"]["status_detail"]["flow_status"],
        "accepted_to_stock"
    );
    assert_eq!(
        received_body["batch"]["status_detail"]["stock_status"],
        "accepted"
    );
    assert_eq!(received_body["order_status"]["order_status"], "completed");
    assert_eq!(
        received_body["order_status"]["flow_status"],
        "accepted_to_stock"
    );
    assert_eq!(
        received_body["batch"]["payload_json"]["received_warehouse"],
        "Tayyor mahsulot ombori"
    );

    let waiting_after = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/wip-batches?status=waiting&order_id=zakaz-fg-receipt",
            &admin_token,
        ))
        .await
        .expect("waiting wip after receipt");
    let waiting_after_body = json_body(waiting_after).await;
    assert_eq!(
        waiting_after_body["batches"]
            .as_array()
            .expect("waiting after")
            .len(),
        0
    );
}

#[tokio::test]
async fn laminatsiya_complete_keeps_state_successful_when_progress_print_fails() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests.clone(),
        fail: true,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-laminatsiya-print-fail".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:asset-007".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(
        &state,
        PrincipalRole::Aparatchi,
        "worker-laminatsiya-print-fail",
    )
    .await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json_with_dims(
                "zakaz-laminatsiya-print-fail",
                "Laminatsiya print fail",
                "9328",
                "apparatus:default:asset-007",
                2,
                950.0,
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
                "apparatus":"apparatus:default:asset-007",
                "order_id":"zakaz-laminatsiya-print-fail",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-007",
                "order_id":"zakaz-laminatsiya-print-fail",
                "action":"complete",
                "lamination_film_leftover_rolls":1,
                "total_waste":1,
                "finished_goods_kg":9,
                "finished_goods_meter":90,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("complete");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body:?}");
    assert_eq!(
        completed_body["states"]["zakaz-laminatsiya-print-fail"],
        "completed"
    );
    assert_eq!(completed_body["progress_batch"]["status"], "completed");
    assert_eq!(completed_body["print"]["ok"], true);
    assert_eq!(completed_body["print"]["status"], "queued");
    assert_eq!(
        completed_body["print"]["printer_status"],
        "server_print_queued"
    );
    wait_for_progress_print_request_count(&print_requests, 1).await;
    assert_eq!(print_requests.lock().await.len(), 1);
}

#[tokio::test]
async fn laminatsiya_pause_does_not_persist_leftover_or_order_waste_metrics() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests,
        fail: false,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-laminatsiya-pause".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:asset-008".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "worker-laminatsiya-pause").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json_with_dims(
                "zakaz-laminatsiya-pause",
                "Laminatsiya pause order",
                "9324",
                "apparatus:default:asset-008",
                2,
                950.0,
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
                "apparatus":"apparatus:default:asset-008",
                "order_id":"zakaz-laminatsiya-pause",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let paused = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-008",
                "order_id":"zakaz-laminatsiya-pause",
                "action":"pause",
                "lamination_print_leftover_rolls":8,
                "lamination_film_leftover_rolls":4,
                "total_waste":1.5,
                "finished_goods_kg":10,
                "finished_goods_meter":72,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("pause with leftover metrics ignored");
    let paused_status = paused.status();
    let paused_body = json_body(paused).await;
    assert_eq!(paused_status, StatusCode::OK, "{paused_body:?}");
    assert_eq!(paused_body["progress_batch"]["status"], "roll_detached");
    assert!(paused_body["progress_batch"]["lamination_print_leftover_rolls"].is_null());
    assert!(paused_body["progress_batch"]["lamination_film_leftover_rolls"].is_null());
    assert!(paused_body["progress_batch"]["total_waste"].is_null());
    assert_eq!(paused_body["progress_batch"]["finished_goods_kg"], 10.0);
    assert_eq!(paused_body["progress_batch"]["finished_goods_meter"], 72.0);
    assert!(paused_body["progress_event"]["lamination_film_leftover_rolls"].is_null());
    assert!(paused_body["progress_event"]["total_waste"].is_null());
}

#[tokio::test]
async fn laminatsiya_queue_exposes_start_for_waiting_print_wip_behind_unready_queue_head() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests,
        fail: false,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-laminatsiya-print-wip".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec![
                "apparatus:default:bosma_7".to_string(),
                "apparatus:default:asset-007".to_string(),
            ],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(
        &state,
        PrincipalRole::Aparatchi,
        "worker-laminatsiya-print-wip",
    )
    .await;
    let router = build_router(state);
    let waiting_order_id = "zakaz-laminatsiya-print-wip-waiting";
    let order_id = "zakaz-laminatsiya-print-wip";

    for (map_order_id, title, order_number) in [
        (waiting_order_id, "Laminatsiya waiting queue head", "9341"),
        (order_id, "Laminatsiya print WIP order", "9340"),
    ] {
        let saved = router
            .clone()
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &admin_token,
                &two_apparatus_order_map_json(
                    map_order_id,
                    title,
                    order_number,
                    "apparatus:default:bosma_7",
                    "apparatus:default:asset-007",
                ),
            ))
            .await
            .expect("save map");
        assert_eq!(saved.status(), StatusCode::OK);
    }

    let sequence = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
                    "order_ids":["{waiting_order_id}","{order_id}"]
                }}"#
            ),
        ))
        .await
        .expect("set lamination sequence");
    assert_eq!(sequence.status(), StatusCode::OK);
    let print_sequence = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:bosma_7",
                    "order_ids":["{order_id}","{waiting_order_id}"]
                }}"#
            ),
        ))
        .await
        .expect("set print sequence");
    assert_eq!(print_sequence.status(), StatusCode::OK);
    provision_test_qolip(&router, &admin_token, order_id).await;

    let started_print = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                &format!(
                    r#"{{
                        "apparatus":"apparatus:default:bosma_7",
                        "order_id":"{order_id}",
                        "action":"start"
                    }}"#
                ),
                order_id,
            ),
        ))
        .await
        .expect("start print");
    assert_eq!(started_print.status(), StatusCode::OK);

    let paused_print = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:bosma_7",
                    "order_id":"{order_id}",
                    "action":"pause",
                    "produced_qty":100,
                    "uom":"kg"
                }}"#
            ),
        ))
        .await
        .expect("pause print with output WIP");
    let paused_print_status = paused_print.status();
    let paused_print_body = json_body(paused_print).await;
    assert_eq!(paused_print_status, StatusCode::OK, "{paused_print_body:?}");
    let print_qr = paused_print_body["progress_batch"]["qr_payload"]
        .as_str()
        .expect("print WIP QR")
        .to_string();

    let snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &worker_token,
        ))
        .await
        .expect("laminatsiya queue snapshot");
    let snapshot_status = snapshot.status();
    let snapshot_body = json_body(snapshot).await;
    assert_eq!(snapshot_status, StatusCode::OK, "{snapshot_body:?}");
    let waiting_control =
        &snapshot_body["queue_action_controls"]["apparatus:default:asset-007"][waiting_order_id];
    assert_eq!(waiting_control["state"], "pending");
    assert_eq!(waiting_control["previous_stage_ready"], false);
    assert_eq!(
        waiting_control["interaction"]["previous_wip_mode"],
        "waiting"
    );
    assert_eq!(
        waiting_control["interaction"]["mode"],
        "waiting_previous_stage"
    );
    assert!(
        waiting_control["allowed_actions"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    let action_control =
        &snapshot_body["queue_action_controls"]["apparatus:default:asset-007"][order_id];
    assert_eq!(action_control["state"], "pending");
    assert_eq!(action_control["previous_stage_ready"], false);
    assert_eq!(
        action_control["interaction"]["previous_wip_mode"],
        "scan_required"
    );
    assert_eq!(action_control["interaction"]["mode"], "fresh_start");
    assert!(
        action_control["allowed_actions"]
            .as_array()
            .expect("allowed actions")
            .iter()
            .any(|action| action == "start")
    );

    let started_without_qr = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
                    "order_id":"{order_id}",
                    "action":"start"
                }}"#
            ),
        ))
        .await
        .expect("reject unscanned laminatsiya WIP start");
    let started_without_qr_status = started_without_qr.status();
    let started_without_qr_body = json_body(started_without_qr).await;
    assert_eq!(
        started_without_qr_status,
        StatusCode::BAD_REQUEST,
        "{started_without_qr_body:?}"
    );
    assert_eq!(started_without_qr_body["error"], "progress_qr_required");

    let started_laminatsiya = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
                    "order_id":"{order_id}",
                    "action":"start",
                    "qr_payload":"{print_qr}"
                }}"#
            ),
        ))
        .await
        .expect("start laminatsiya from print WIP");
    let started_laminatsiya_status = started_laminatsiya.status();
    let started_laminatsiya_body = json_body(started_laminatsiya).await;
    assert_eq!(
        started_laminatsiya_status,
        StatusCode::OK,
        "{started_laminatsiya_body:?}"
    );
    assert_eq!(started_laminatsiya_body["states"][order_id], "in_progress");

    let in_use = router
        .oneshot(request(
            "GET",
            &format!(
                "/v1/mobile/admin/production-maps/wip-batches?apparatus=apparatus%3Adefault%3Aasset-007&status=in_use&order_id={order_id}"
            ),
            &admin_token,
        ))
        .await
        .expect("laminatsiya in-use WIP");
    let in_use_status = in_use.status();
    let in_use_body = json_body(in_use).await;
    assert_eq!(in_use_status, StatusCode::OK, "{in_use_body:?}");
    let in_use_batch = in_use_body["batches"]
        .as_array()
        .expect("in-use batches")
        .iter()
        .find(|batch| batch["qr_payload"] == print_qr)
        .expect("print WIP handed to laminatsiya");
    assert_eq!(in_use_batch["wip_status"], "in_use");
    assert_eq!(
        in_use_batch["used_by_apparatus"],
        "apparatus:default:asset-007"
    );
}

#[tokio::test]
async fn laminatsiya_merge_splices_same_order_wips_and_rejects_another_order() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests,
        fail: false,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-laminatsiya-merge".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec![
                "apparatus:default:bosma_7".to_string(),
                "apparatus:default:asset-007".to_string(),
                "apparatus:default:asset-008".to_string(),
            ],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "worker-laminatsiya-merge").await;
    let router = build_router(state);
    let order_id = "zakaz-laminatsiya-merge";
    let foreign_order_id = "zakaz-laminatsiya-merge-foreign";

    for map in [
        two_apparatus_order_map_json(
            order_id,
            "Laminatsiya merge",
            "9342",
            "apparatus:default:bosma_7",
            "apparatus:default:asset-007",
        ),
        pechat_order_map_json_with_dims(
            foreign_order_id,
            "Foreign merge source",
            "9343",
            "apparatus:default:asset-008",
            2,
            950.0,
        ),
    ] {
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
        let saved_status = saved.status();
        let saved_body = json_body(saved).await;
        assert_eq!(saved_status, StatusCode::OK, "{saved_body:?}");
    }

    let foreign_started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-008",
                    "order_id":"{foreign_order_id}",
                    "action":"start"
                }}"#
            ),
        ))
        .await
        .expect("start foreign source");
    assert_eq!(foreign_started.status(), StatusCode::OK);
    let foreign_paused = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-008",
                    "order_id":"{foreign_order_id}",
                    "action":"pause",
                    "finished_goods_meter":40,
                    "finished_goods_kg":5
                }}"#
            ),
        ))
        .await
        .expect("pause foreign source");
    let foreign_paused_status = foreign_paused.status();
    let foreign_paused_body = json_body(foreign_paused).await;
    assert_eq!(
        foreign_paused_status,
        StatusCode::OK,
        "{foreign_paused_body:?}"
    );
    let foreign_qr = foreign_paused_body["progress_batch"]["qr_payload"]
        .as_str()
        .expect("foreign WIP QR")
        .to_string();

    provision_test_qolip(&router, &admin_token, order_id).await;
    let print_started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                &format!(
                    r#"{{
                        "apparatus":"apparatus:default:bosma_7",
                        "order_id":"{order_id}",
                        "action":"start"
                    }}"#
                ),
                order_id,
            ),
        ))
        .await
        .expect("start print");
    assert_eq!(print_started.status(), StatusCode::OK);

    let first_print_paused = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:bosma_7",
                    "order_id":"{order_id}",
                    "action":"pause",
                    "produced_qty":100,
                    "uom":"m"
                }}"#
            ),
        ))
        .await
        .expect("pause first print roll");
    let first_print_paused_status = first_print_paused.status();
    let first_print_paused_body = json_body(first_print_paused).await;
    assert_eq!(
        first_print_paused_status,
        StatusCode::OK,
        "{first_print_paused_body:?}"
    );
    let first_batch_id = first_print_paused_body["progress_batch"]["batch_id"]
        .as_str()
        .expect("first print WIP id")
        .to_string();
    let first_qr = first_print_paused_body["progress_batch"]["qr_payload"]
        .as_str()
        .expect("first print WIP QR")
        .to_string();

    let print_resumed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:bosma_7",
                    "order_id":"{order_id}",
                    "action":"resume"
                }}"#
            ),
        ))
        .await
        .expect("resume print");
    assert_eq!(print_resumed.status(), StatusCode::OK);
    let second_print_completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:bosma_7",
                    "order_id":"{order_id}",
                    "action":"complete",
                    "produced_qty":80,
                    "returned_paint_items":[
                        {{"usage":"rasxot","category":"colors","name":"Oq","values":{{"Mix":1,"Oq":1,"Qora":0}}}},
                        {{"usage":"astatka","category":"colors","name":"Oq","values":{{"Mix":0.25,"Oq":0.25,"Qora":0}}}}
                    ],
                    "total_waste":1,
                    "finished_goods_kg":10,
                    "finished_goods_meter":80,
                    "printer":"zebra",
                    "print_mode":"rfid"
                }}"#
            ),
        ))
        .await
        .expect("complete second print roll");
    let second_print_paused_status = second_print_completed.status();
    let second_print_paused_body = json_body(second_print_completed).await;
    assert_eq!(
        second_print_paused_status,
        StatusCode::OK,
        "{second_print_paused_body:?}"
    );
    let second_batch_id = second_print_paused_body["progress_batch"]["batch_id"]
        .as_str()
        .expect("second print WIP id")
        .to_string();
    let second_qr = second_print_paused_body["progress_batch"]["qr_payload"]
        .as_str()
        .expect("second print WIP QR")
        .to_string();

    let laminatsiya_started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
                    "order_id":"{order_id}",
                    "action":"start",
                    "qr_payload":"{first_qr}"
                }}"#
            ),
        ))
        .await
        .expect("start laminatsiya");
    let laminatsiya_started_status = laminatsiya_started.status();
    let laminatsiya_started_body = json_body(laminatsiya_started).await;
    assert_eq!(
        laminatsiya_started_status,
        StatusCode::OK,
        "{laminatsiya_started_body:?}"
    );
    assert_eq!(
        laminatsiya_started_body["session"]["payload_json"]["input_lineage"]
            .as_array()
            .expect("started Laminatsiya input lineage")
            .len(),
        1
    );
    assert!(
        laminatsiya_started_body["session"]["payload_json"]["merge_count"].is_null(),
        "Laminatsiya Start must not be recorded as Merge"
    );
    assert!(
        laminatsiya_started_body["session"]["payload_json"]["rezka_active_partial_rolls"].is_null(),
        "Laminatsiya Start must not create child rolls before Merge"
    );

    let snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &worker_token,
        ))
        .await
        .expect("laminatsiya merge control");
    let snapshot_body = json_body(snapshot).await;
    let action_control = &snapshot_body["queue_action_controls"]
        ["apparatus:default:asset-007"][order_id];
    assert!(
        action_control["allowed_actions"]
            .as_array()
            .expect("allowed actions")
            .iter()
            .any(|action| action == "merge")
    );
    assert_eq!(
        action_control["rezka_input_lineage"]
            .as_array()
            .expect("generic merge lineage")
            .len(),
        1
    );

    let foreign_merge = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
                    "order_id":"{order_id}",
                    "action":"merge",
                    "qr_payload":"{foreign_qr}"
                }}"#
            ),
        ))
        .await
        .expect("reject foreign order merge");
    assert_eq!(foreign_merge.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(foreign_merge).await["error"],
        "merge_input_not_accepted"
    );

    let merged = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
                    "order_id":"{order_id}",
                    "action":"merge",
                    "qr_payload":"{second_qr}",
                    "total_waste":0.15,
                    "uom":"kg"
                }}"#
            ),
        ))
        .await
        .expect("merge second print WIP");
    let merged_status = merged.status();
    let merged_body = json_body(merged).await;
    assert_eq!(merged_status, StatusCode::OK, "{merged_body:?}");
    assert_eq!(merged_body["progress_event"]["action"], "merge");
    assert_eq!(merged_body["progress_event"]["total_waste"], 0.15);
    let lineage = merged_body["session"]["payload_json"]["input_lineage"]
        .as_array()
        .expect("merged lineage");
    assert!(lineage.iter().any(|link| {
        link["input_batch_id"] == first_batch_id && link["status"] == "processed"
    }));
    assert!(lineage.iter().any(|link| {
        link["input_batch_id"] == second_batch_id && link["status"] == "in_use"
    }));
    assert!(
        merged_body["session"]["payload_json"]["rezka_active_partial_rolls"].is_null()
    );

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
                    "order_id":"{order_id}",
                    "action":"complete",
                    "qr_payload":"{second_qr}",
                    "lamination_print_leftover_rolls":1,
                    "lamination_film_leftover_rolls":1,
                    "total_waste":1,
                    "finished_goods_kg":20,
                    "finished_goods_meter":175,
                    "printer":"zebra",
                    "print_mode":"rfid"
                }}"#
            ),
        ))
        .await
        .expect("complete merged laminatsiya roll");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body:?}");
    assert_eq!(completed_body["states"][order_id], "completed");
    let source_links = completed_body["progress_batch"]["payload_json"]["source_input_links"]
        .as_array()
        .expect("output source links");
    assert!(source_links.iter().any(|link| link["input_batch_id"] == first_batch_id));
    assert!(source_links.iter().any(|link| link["input_batch_id"] == second_batch_id));
}
