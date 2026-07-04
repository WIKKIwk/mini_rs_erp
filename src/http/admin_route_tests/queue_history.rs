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
                assigned_apparatus: vec!["7 ta rangli pechat".to_string()],
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
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &admin_token,
                &pechat_order_map_json(id, "Completed route", number, "7 ta rangli pechat"),
            ))
            .await
            .expect("save map");
        assert_eq!(response.status(), StatusCode::OK);
    }

    let sequence = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_ids":["zakaz-complete-1","zakaz-complete-2","zakaz-complete-3"]
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
            let response = router
                .clone()
                .oneshot(request_with_body(
                    "POST",
                    "/v1/mobile/admin/production-maps/queue-action",
                    token,
                    &format!(
                        r#"{{"apparatus":"7 ta rangli pechat","order_id":"{order_id}","action":"{action}","produced_qty":1,"uom":"kg","return_ink_kg":1,"total_waste":1,"finished_goods_kg":1,"finished_goods_meter":1}}"#
                    ),
                ))
                .await
                .expect("queue action");
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

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
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["order_id"], "zakaz-complete-2");
    assert_eq!(items[1]["order_id"], "zakaz-complete-1");

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
        ("worker-closed-pechat", "7 ta rangli pechat"),
        ("worker-closed-lamin", "Laminatsiya 1"),
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
                "7 ta rangli pechat",
                "Laminatsiya 1",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);

    let mut pechat_pause_qr = String::new();
    let mut pechat_output_qr = String::new();
    for action in ["start", "pause", "resume", "complete"] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/admin/production-maps/queue-action",
                &pechat_worker,
                &format!(
                    r#"{{"apparatus":"7 ta rangli pechat","order_id":"zakaz-closed-route","action":"{action}","produced_qty":1,"gross_qty":1,"uom":"kg","return_ink_kg":1,"total_waste":1,"finished_goods_kg":1,"finished_goods_meter":1,"printer":"zebra","print_mode":"rfid"}}"#
                ),
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

    for qr in [pechat_pause_qr.as_str(), pechat_output_qr.as_str()] {
        for action in ["start", "complete"] {
            let body = if action == "complete" {
                r#"{"apparatus":"Laminatsiya 1","order_id":"zakaz-closed-route","action":"complete","lamination_film_leftover_rolls":1,"total_waste":1,"finished_goods_kg":1,"finished_goods_meter":1,"produced_qty":1,"gross_qty":1,"uom":"kg","printer":"zebra","print_mode":"rfid"}"#.to_string()
            } else {
                format!(
                    r#"{{"apparatus":"Laminatsiya 1","order_id":"zakaz-closed-route","action":"start","produced_qty":1,"gross_qty":1,"uom":"kg","printer":"zebra","print_mode":"rfid","qr_payload":"{qr}"}}"#
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
    assert_eq!(logs[3]["apparatus"], "7 ta rangli pechat");
    assert_eq!(logs[7]["action"], "complete");
    assert_eq!(logs[7]["apparatus"], "Laminatsiya 1");
    assert_eq!(logs[7]["actor_ref"], "worker-closed-lamin");
}
