use super::*;

#[tokio::test]
async fn admin_rejects_zero_output_completion_request_and_notifies_worker() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-reject-complete".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat".to_string()],
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "worker-reject-complete").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-reject-zero",
                "Reject zero order",
                "9421",
                "7 ta rangli pechat",
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
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-reject-zero",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let requested = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-reject-zero",
                "action":"complete",
                "completion_request_note":"kg va metraj yo'q"
            }"#,
        ))
        .await
        .expect("complete request");
    assert_eq!(requested.status(), StatusCode::OK);
    let requested_body = json_body(requested).await;
    let event_id = requested_body["completion_request"]["event_id"]
        .as_str()
        .expect("event id");

    let rejected = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/completion-requests/decision",
            &admin_token,
            &format!(r#"{{"event_id":"{event_id}","decision":"reject"}}"#),
        ))
        .await
        .expect("reject");
    let rejected_status = rejected.status();
    let rejected_body = json_body(rejected).await;
    assert_eq!(rejected_status, StatusCode::OK, "{rejected_body:?}");
    assert_eq!(rejected_body["states"]["zakaz-reject-zero"], "in_progress");
    assert_eq!(rejected_body["decision"]["decision"], "rejected");
    assert_eq!(
        rejected_body["decision"]["message"],
        "Sizni so'rovingiz rad etildi"
    );

    let requests_after = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/completion-requests",
            &admin_token,
        ))
        .await
        .expect("requests after reject");
    let requests_body = json_body(requests_after).await;
    assert_eq!(
        requests_body["completion_requests"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let decisions = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/completion-request-decisions",
            &worker_token,
        ))
        .await
        .expect("worker decisions");
    let decisions_status = decisions.status();
    let decisions_body = json_body(decisions).await;
    assert_eq!(decisions_status, StatusCode::OK, "{decisions_body:?}");
    assert_eq!(
        decisions_body["completion_request_decisions"][0]["decision"],
        "rejected"
    );
    assert_eq!(
        decisions_body["completion_request_decisions"][0]["worker_ref"],
        "worker-reject-complete"
    );
}

#[tokio::test]
async fn queue_pause_print_failure_keeps_committed_pause_log() {
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
            principal_ref: "worker-print-fail".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat".to_string()],
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-print-fail").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-print-fail",
                "Print fail order",
                "9302",
                "7 ta rangli pechat",
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
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-print-fail",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let pause_failed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-print-fail",
                "action":"pause",
                "produced_qty":9.5,
                "uom":"kg",
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("pause");
    let pause_status = pause_failed.status();
    let pause_body = json_body(pause_failed).await;
    assert_eq!(pause_status, StatusCode::OK, "{pause_body:?}");
    assert_eq!(pause_body["print"]["ok"], false);
    assert_eq!(pause_body["print"]["status"], "failed");

    let sequence = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
        ))
        .await
        .expect("sequence");
    let body = json_body(sequence).await;
    assert_eq!(
        body["queue_states"]["7 ta rangli pechat"]["zakaz-print-fail"],
        "paused"
    );
    assert_eq!(print_requests.lock().await.len(), 1);
}
