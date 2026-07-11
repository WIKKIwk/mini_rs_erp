use super::*;

#[tokio::test]
async fn progress_qr_report_marks_processed_qr_as_stale_and_returns_order_flow() {
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
            principal_ref: "worker-qr-report".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec![
                "Pechat sexi".to_string(),
                "Qadoqlash stol".to_string(),
                "Yordamchi aparat".to_string(),
            ],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-qr-report").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &two_apparatus_order_map_json(
                "zakaz-qr-report",
                "QR report order",
                "9501",
                "Pechat sexi",
                "Qadoqlash stol",
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
                "apparatus":"Pechat sexi",
                "order_id":"zakaz-qr-report",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start first");
    assert_eq!(started.status(), StatusCode::OK);

    let paused = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"Pechat sexi",
                "order_id":"zakaz-qr-report",
                "action":"pause",
                "produced_qty":100,
                "uom":"kg",
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("pause first");
    assert_eq!(paused.status(), StatusCode::OK);
    let paused_body = json_body(paused).await;
    let old_qr_payload = paused_body["progress_batch"]["qr_payload"]
        .as_str()
        .expect("old qr payload")
        .to_string();

    let second_started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"Qadoqlash stol",
                    "order_id":"zakaz-qr-report",
                    "action":"start",
                    "qr_payload":"{old_qr_payload}"
                }}"#
            ),
        ))
        .await
        .expect("start second");
    assert_eq!(second_started.status(), StatusCode::OK);

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"Qadoqlash stol",
                "order_id":"zakaz-qr-report",
                "action":"complete",
                "produced_qty":96,
                "uom":"kg",
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("complete second");
    assert_eq!(completed.status(), StatusCode::OK);
    let completed_body = json_body(completed).await;
    let latest_qr_payload = completed_body["progress_batch"]["qr_payload"]
        .as_str()
        .expect("latest qr payload")
        .to_string();
    assert_ne!(latest_qr_payload, old_qr_payload);

    let unrelated_saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &two_apparatus_order_map_json(
                "zakaz-qr-unrelated",
                "Unrelated QR report order",
                "9502",
                "Yordamchi aparat",
                "Qadoqlash stol",
            ),
        ))
        .await
        .expect("save unrelated map");
    assert_eq!(unrelated_saved.status(), StatusCode::OK);
    let unrelated_started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"Yordamchi aparat",
                "order_id":"zakaz-qr-unrelated",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start unrelated");
    assert_eq!(unrelated_started.status(), StatusCode::OK);

    let worker_report = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/progress-qr/report",
            &worker_token,
            &format!(r#"{{"qr_payload":"{old_qr_payload}"}}"#),
        ))
        .await
        .expect("worker qr report");
    assert_eq!(worker_report.status(), StatusCode::FORBIDDEN);

    let report = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/progress-qr/report",
            &admin_token,
            &format!(r#"{{"qr_payload":"{old_qr_payload}"}}"#),
        ))
        .await
        .expect("qr report");
    let report_status = report.status();
    let report_body = json_body(report).await;
    assert_eq!(report_status, StatusCode::OK, "{report_body:?}");
    assert_eq!(report_body["ok"], true);
    assert_eq!(report_body["scanned_batch"]["qr_payload"], old_qr_payload);
    assert_eq!(report_body["scanned_batch"]["wip_status"], "processed");
    assert_eq!(
        report_body["scanned_batch"]["status_detail"]["flow_status"],
        "consumed_by_next_stage"
    );
    assert_eq!(
        report_body["current_batch"]["qr_payload"],
        latest_qr_payload
    );
    assert_eq!(
        report_body["current_batch"]["status_detail"]["work_status"],
        "completed"
    );
    assert_eq!(
        report_body["current_batch"]["status_detail"]["flow_status"],
        "finished_pending_acceptance"
    );
    assert_eq!(
        report_body["current_batch"]["status_detail"]["stock_status"],
        "pending_acceptance"
    );
    assert_eq!(
        report_body["order_status"]["order_status"],
        "finished_pending_acceptance"
    );
    assert_eq!(
        report_body["order_status"]["finished_pending_acceptance_count"],
        1
    );
    assert_eq!(report_body["is_stale"], true);
    assert_eq!(report_body["stale_reason"], "processed_by_next_stage");
    assert_eq!(report_body["order"]["id"], "zakaz-qr-report");
    assert_eq!(report_body["order"]["title"], "QR report order");
    assert_eq!(
        report_body["queue_states"]["Qadoqlash stol"]["zakaz-qr-report"],
        "completed"
    );
    assert!(
        report_body["queue_states"]["Yordamchi aparat"]
            .get("zakaz-qr-unrelated")
            .is_none()
    );
    assert_eq!(report_body["logs"].as_array().expect("logs").len(), 4);
    assert_eq!(
        report_body["run_sessions"]
            .as_array()
            .expect("run sessions")
            .len(),
        2
    );
    assert_eq!(
        report_body["progress_batches"]
            .as_array()
            .expect("progress batches")
            .len(),
        2
    );
    assert_eq!(report_body["opened_by"]["actor_ref"], "worker-qr-report");
}

#[tokio::test]
async fn progress_qr_history_lists_own_batches_and_reprints_existing_qr() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests.clone(),
        fail: false,
    }));
    for (worker_ref, apparatus) in [
        ("worker-qr-history-a", "7 ta rangli pechat"),
        ("worker-qr-history-b", "8 ta rangli pechat"),
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
            .expect("assignment");
    }
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_a_token = session_for(&state, PrincipalRole::Aparatchi, "worker-qr-history-a").await;
    let worker_b_token = session_for(&state, PrincipalRole::Aparatchi, "worker-qr-history-b").await;
    let router = build_router(state);

    let mut other_qr = String::new();
    for (order_id, order_number, apparatus, token) in [
        (
            "zakaz-qr-history-a",
            "9503",
            "7 ta rangli pechat",
            &worker_a_token,
        ),
        (
            "zakaz-qr-history-b",
            "9504",
            "8 ta rangli pechat",
            &worker_b_token,
        ),
    ] {
        let saved = router
            .clone()
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &admin_token,
                &pechat_order_map_json(
                    order_id,
                    &format!("QR history {order_number}"),
                    order_number,
                    apparatus,
                ),
            ))
            .await
            .expect("save map");
        assert_eq!(saved.status(), StatusCode::OK);
        provision_test_qolip(&router, &admin_token, order_id).await;

        let start_body = with_test_qolip(
            &format!(
                r#"{{
                    "apparatus":"{apparatus}",
                    "order_id":"{order_id}",
                    "action":"start"
                }}"#
            ),
            order_id,
        );

        let started = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/admin/production-maps/queue-action",
                token,
                &start_body,
            ))
            .await
            .expect("start");
        assert_eq!(started.status(), StatusCode::OK);

        let paused = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/admin/production-maps/queue-action",
                token,
                &format!(
                    r#"{{
                        "apparatus":"{apparatus}",
                        "order_id":"{order_id}",
                        "action":"pause",
                        "produced_qty":12,
                        "uom":"kg",
                        "printer":"zebra",
                        "print_mode":"rfid"
                    }}"#
                ),
            ))
            .await
            .expect("pause");
        let paused_status = paused.status();
        let paused_body = json_body(paused).await;
        assert_eq!(paused_status, StatusCode::OK, "{paused_body:?}");
        if order_id == "zakaz-qr-history-b" {
            other_qr = paused_body["progress_batch"]["qr_payload"]
                .as_str()
                .expect("other qr")
                .to_string();
        }
    }

    let history = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/progress-qr/history",
            &worker_a_token,
        ))
        .await
        .expect("history");
    let history_status = history.status();
    let history_body = json_body(history).await;
    assert_eq!(history_status, StatusCode::OK, "{history_body:?}");
    let batches = history_body["batches"].as_array().expect("batches");
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0]["order_id"], "zakaz-qr-history-a");
    let own_qr = batches[0]["qr_payload"].as_str().expect("own qr");

    let forbidden = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/progress-qr/reprint",
            &worker_a_token,
            r#"{
                "qr_payload":"missing-worker-b-qr"
            }"#,
        ))
        .await
        .expect("forbidden reprint missing");
    assert_eq!(forbidden.status(), StatusCode::NOT_FOUND);

    let forbidden_other = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/progress-qr/reprint",
            &worker_a_token,
            &format!(r#"{{"qr_payload":"{other_qr}"}}"#),
        ))
        .await
        .expect("forbidden reprint other worker");
    assert_eq!(forbidden_other.status(), StatusCode::FORBIDDEN);

    let reprinted = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/progress-qr/reprint",
            &worker_a_token,
            &format!(
                r#"{{
                    "qr_payload":"{own_qr}",
                    "printer":"zebra",
                    "print_mode":"rfid",
                    "print_count":1
                }}"#
            ),
        ))
        .await
        .expect("reprint own");
    let reprinted_status = reprinted.status();
    let reprinted_body = json_body(reprinted).await;
    assert_eq!(reprinted_status, StatusCode::OK, "{reprinted_body:?}");
    assert_eq!(reprinted_body["ok"], true);
    assert_eq!(reprinted_body["batch"]["qr_payload"], own_qr);
    assert_eq!(reprinted_body["print"]["status"], "printed");
    let printed = print_requests.lock().await;
    assert_eq!(printed.len(), 3);
    assert_eq!(printed[2].epc, own_qr);
}
