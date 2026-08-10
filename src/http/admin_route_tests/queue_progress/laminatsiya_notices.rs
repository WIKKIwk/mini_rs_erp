use super::*;

#[tokio::test]
async fn laminatsiya_complete_with_both_leftovers_stays_in_history() {
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
            principal_ref: "worker-laminatsiya-notice".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["Laminatsiya 1".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(
        &state,
        PrincipalRole::Aparatchi,
        "worker-laminatsiya-notice",
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
                "zakaz-laminatsiya-notice",
                "Laminatsiya notice order",
                "9325",
                "Laminatsiya 1",
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
                "apparatus":"Laminatsiya 1",
                "order_id":"zakaz-laminatsiya-notice",
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
                "apparatus":"Laminatsiya 1",
                "order_id":"zakaz-laminatsiya-notice",
                "action":"complete",
                "lamination_print_leftover_rolls":1.5,
                "lamination_film_leftover_rolls":2.5,
                "total_waste":3.5,
                "finished_goods_kg":20.75,
                "finished_goods_meter":140.25,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("complete with both leftovers");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body:?}");

    let requests = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/completion-requests",
            &admin_token,
        ))
        .await
        .expect("completion requests");
    let requests_status = requests.status();
    let requests_body = json_body(requests).await;
    assert_eq!(requests_status, StatusCode::OK, "{requests_body:?}");
    assert!(
        requests_body["completion_requests"]
            .as_array()
            .expect("completion_requests")
            .is_empty()
    );

    let history = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/progress-qr/history",
            &worker_token,
        ))
        .await
        .expect("progress history");
    let history_status = history.status();
    let history_body = json_body(history).await;
    assert_eq!(history_status, StatusCode::OK, "{history_body:?}");
    let batches = history_body["batches"].as_array().expect("batches");
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0]["order_id"], "zakaz-laminatsiya-notice");
    assert_eq!(batches[0]["lamination_print_leftover_rolls"], 1.5);
    assert_eq!(batches[0]["lamination_film_leftover_rolls"], 2.5);
}
