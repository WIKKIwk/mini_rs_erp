use super::*;

#[tokio::test]
async fn worker_completed_orders_are_actor_scoped_and_latest_first() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests,
        fail: false,
    }));
    for worker_ref in ["worker-complete-1", "worker-complete-2"] {
        state
            .admin
            .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: worker_ref.to_string(),
                role_id: "aparatchi".to_string(),
                assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
                assigned_item_groups: Vec::new(),
            })
            .await
            .expect("aparatchi assignment");
    }
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_one = session_for(&state, PrincipalRole::Aparatchi, "worker-complete-1").await;
    let worker_two = session_for(&state, PrincipalRole::Aparatchi, "worker-complete-2").await;
    let router = build_router(state);

    for (id, number) in [
        ("zakaz-complete-1", "9101"),
        ("zakaz-complete-2", "9102"),
        ("zakaz-complete-3", "9103"),
        ("zakaz-partial-pause", "9104"),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &admin_token,
                &pechat_order_map_json(id, "Completed route", number, "apparatus:default:bosma_7"),
            ))
            .await
            .expect("save map");
        assert_eq!(response.status(), StatusCode::OK);
        provision_test_qolip(&router, &admin_token, id).await;
    }

    let sequence = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
            r#"{
                "apparatus":"apparatus:default:bosma_7",
                "order_ids":["zakaz-complete-1","zakaz-complete-2","zakaz-complete-3","zakaz-partial-pause"]
            }"#,
        ))
        .await
        .expect("save sequence");
    assert_eq!(sequence.status(), StatusCode::OK);

    for (token, order_id) in [
        (&worker_one, "zakaz-complete-1"),
        (&worker_one, "zakaz-complete-2"),
        (&worker_two, "zakaz-complete-3"),
    ] {
        for action in ["start", "complete"] {
            let body = format!(
                r#"{{"apparatus":"apparatus:default:bosma_7","order_id":"{order_id}","action":"{action}","produced_qty":1,"uom":"kg","return_ink_kg":1,"total_waste":1,"finished_goods_kg":1,"finished_goods_meter":1}}"#
            );
            let body = if action == "start" {
                with_test_qolip(&body, order_id)
            } else if action == "complete" {
                with_test_returned_paint(&body)
            } else {
                body
            };
            let response = router
                .clone()
                .oneshot(request_with_body(
                    "POST",
                    "/v1/mobile/admin/production-maps/queue-action",
                    token,
                    &body,
                ))
                .await
                .expect("queue action");
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    let start_body = with_test_qolip(
        r#"{"apparatus":"apparatus:default:bosma_7","order_id":"zakaz-partial-pause","action":"start"}"#,
        "zakaz-partial-pause",
    );
    let start_response = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_one,
            &start_body,
        ))
        .await
        .expect("partial start");
    assert_eq!(start_response.status(), StatusCode::OK);
    let pause_response = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_one,
            r#"{"apparatus":"apparatus:default:bosma_7","order_id":"zakaz-partial-pause","action":"pause","produced_qty":1,"uom":"kg","return_ink_kg":1,"total_waste":1,"finished_goods_kg":1,"finished_goods_meter":1}"#,
        ))
        .await
        .expect("partial pause");
    assert_eq!(pause_response.status(), StatusCode::OK);

    let first_worker_completed = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/completed-orders",
            &worker_one,
        ))
        .await
        .expect("completed orders");
    assert_eq!(first_worker_completed.status(), StatusCode::OK);
    let body = json_body(first_worker_completed).await;
    let items = body["completed_orders"]
        .as_array()
        .expect("completed_orders");
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["order_id"], "zakaz-partial-pause");
    assert_eq!(items[0]["status"], "in_progress");
    assert_eq!(items[1]["order_id"], "zakaz-complete-2");
    assert_eq!(items[1]["status"], "completed");
    assert_eq!(items[2]["order_id"], "zakaz-complete-1");
    assert_eq!(items[2]["status"], "completed");

    let second_worker_completed = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/completed-orders",
            &worker_two,
        ))
        .await
        .expect("completed orders");
    assert_eq!(second_worker_completed.status(), StatusCode::OK);
    let body = json_body(second_worker_completed).await;
    let items = body["completed_orders"]
        .as_array()
        .expect("completed_orders");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["order_id"], "zakaz-complete-3");
    assert_eq!(items[0]["status"], "completed");
}

#[tokio::test]
async fn closed_orders_return_only_fully_completed_maps_with_action_logs() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests,
        fail: false,
    }));
    for (worker_ref, apparatus) in [
        ("worker-closed-pechat", "apparatus:default:bosma_7"),
        ("worker-closed-lamin", "apparatus:default:asset-007"),
    ] {
        state
            .admin
            .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: worker_ref.to_string(),
                role_id: "aparatchi".to_string(),
                assigned_apparatus: vec![apparatus.to_string()],
                assigned_item_groups: Vec::new(),
            })
            .await
            .expect("aparatchi assignment");
    }
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let pechat_worker = session_for(&state, PrincipalRole::Aparatchi, "worker-closed-pechat").await;
    let lamin_worker = session_for(&state, PrincipalRole::Aparatchi, "worker-closed-lamin").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &two_apparatus_order_map_json(
                "zakaz-closed-route",
                "Closed route",
                "9401",
                "apparatus:default:bosma_7",
                "apparatus:default:asset-007",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);
    provision_test_qolip(&router, &admin_token, "zakaz-closed-route").await;

    let mut pechat_pause_qr = String::new();
    let mut pechat_output_qr = String::new();
    for action in ["start", "pause", "resume", "complete"] {
        let body = format!(
            r#"{{"apparatus":"apparatus:default:bosma_7","order_id":"zakaz-closed-route","action":"{action}","produced_qty":1,"gross_qty":1,"uom":"kg","return_ink_kg":1,"total_waste":1,"finished_goods_kg":1,"finished_goods_meter":1,"printer":"zebra","print_mode":"rfid"}}"#
        );
        let body = if action == "start" {
            with_test_qolip(&body, "zakaz-closed-route")
        } else if action == "complete" {
            with_test_returned_paint(&body)
        } else {
            body
        };
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/admin/production-maps/queue-action",
                &pechat_worker,
                &body,
            ))
            .await
            .expect("pechat action");
        let status = response.status();
        if action == "pause" {
            let body = json_body(response).await;
            pechat_pause_qr = body["progress_batch"]["qr_payload"]
                .as_str()
                .expect("pechat pause qr")
                .to_string();
        } else if action == "complete" {
            let body = json_body(response).await;
            pechat_output_qr = body["progress_batch"]["qr_payload"]
                .as_str()
                .expect("pechat output qr")
                .to_string();
        }
        assert_eq!(status, StatusCode::OK);
    }

    let before_lamin = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/closed-orders",
            &admin_token,
        ))
        .await
        .expect("closed orders before lamin");
    assert_eq!(before_lamin.status(), StatusCode::OK);
    assert_eq!(
        json_body(before_lamin).await["closed_orders"]
            .as_array()
            .expect("closed_orders")
            .len(),
        0
    );

    let pechat_history_before_lamin = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/completed-orders",
            &pechat_worker,
        ))
        .await
        .expect("pechat worker history before lamin");
    let pechat_history_body = json_body(pechat_history_before_lamin).await;
    let pechat_history = pechat_history_body["completed_orders"]
        .as_array()
        .expect("pechat completed_orders");
    let pechat_stage_entry = pechat_history
        .iter()
        .find(|item| item["order_id"] == "zakaz-closed-route")
        .expect("pechat stage history entry");
    assert_eq!(pechat_stage_entry["apparatus"], "apparatus:default:bosma_7");
    assert_eq!(pechat_stage_entry["status"], "completed");

    for qr in [pechat_pause_qr.as_str(), pechat_output_qr.as_str()] {
        for action in ["start", "complete"] {
            let body = if action == "complete" {
                r#"{"apparatus":"apparatus:default:asset-007","order_id":"zakaz-closed-route","action":"complete","lamination_film_leftover_rolls":1,"total_waste":1,"finished_goods_kg":1,"finished_goods_meter":1,"produced_qty":1,"gross_qty":1,"uom":"kg","printer":"zebra","print_mode":"rfid"}"#.to_string()
            } else {
                format!(
                    r#"{{"apparatus":"apparatus:default:asset-007","order_id":"zakaz-closed-route","action":"start","produced_qty":1,"gross_qty":1,"uom":"kg","printer":"zebra","print_mode":"rfid","qr_payload":"{qr}"}}"#
                )
            };
            let response = router
                .clone()
                .oneshot(request_with_body(
                    "POST",
                    "/v1/mobile/admin/production-maps/queue-action",
                    &lamin_worker,
                    &body,
                ))
                .await
                .expect("lamin action");
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    let closed = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/closed-orders",
            &admin_token,
        ))
        .await
        .expect("closed orders");
    let closed_status = closed.status();
    let body = json_body(closed).await;
    assert_eq!(closed_status, StatusCode::OK, "{body:?}");
    let orders = body["closed_orders"].as_array().expect("closed_orders");
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0]["order_id"], "zakaz-closed-route");
    assert_eq!(orders[0]["order_number"], "9401");
    assert_eq!(orders[0]["closed_by_ref"], "worker-closed-lamin");
    assert_eq!(orders[0]["closed_by_display_name"], "Admin");
    let logs = orders[0]["logs"].as_array().expect("logs");
    assert_eq!(logs.len(), 8);
    assert_eq!(logs[0]["action"], "start");
    assert_eq!(logs[0]["actor_ref"], "worker-closed-pechat");
    assert_eq!(logs[3]["action"], "complete");
    assert_eq!(logs[3]["apparatus"], "apparatus:default:bosma_7");
    assert_eq!(logs[7]["action"], "complete");
    assert_eq!(logs[7]["apparatus"], "apparatus:default:asset-007");
    assert_eq!(logs[7]["actor_ref"], "worker-closed-lamin");

    let pechat_history_after_closed = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/completed-orders",
            &pechat_worker,
        ))
        .await
        .expect("pechat worker history after closed");
    let pechat_history_body = json_body(pechat_history_after_closed).await;
    let pechat_history = pechat_history_body["completed_orders"]
        .as_array()
        .expect("pechat completed_orders after closed");
    let pechat_stage_entry = pechat_history
        .iter()
        .find(|item| item["order_id"] == "zakaz-closed-route")
        .expect("pechat history retained after global close");
    assert_eq!(pechat_stage_entry["apparatus"], "apparatus:default:bosma_7");

    let lamin_history = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/completed-orders",
            &lamin_worker,
        ))
        .await
        .expect("lamin worker history after closed");
    let lamin_history_body = json_body(lamin_history).await;
    let lamin_history = lamin_history_body["completed_orders"]
        .as_array()
        .expect("lamin completed_orders");
    let lamin_stage_entry = lamin_history
        .iter()
        .find(|item| item["order_id"] == "zakaz-closed-route")
        .expect("lamin stage history entry");
    assert_eq!(
        lamin_stage_entry["apparatus"],
        "apparatus:default:asset-007"
    );
    assert_eq!(lamin_stage_entry["status"], "completed");
}
