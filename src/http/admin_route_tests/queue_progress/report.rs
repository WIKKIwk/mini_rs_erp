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
                "apparatus:default:asset-007".to_string(),
                "apparatus:default:paket".to_string(),
                "apparatus:default:asset-010".to_string(),
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
                "apparatus:default:asset-007",
                "apparatus:default:paket",
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
                "order_id":"zakaz-qr-report",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start first");
    assert_eq!(started.status(), StatusCode::OK);

    let first_completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:asset-007",
                "order_id":"zakaz-qr-report",
                "action":"complete",
                "lamination_film_leftover_rolls":1,
                "total_waste":1,
                "finished_goods_kg":100,
                "finished_goods_meter":1000,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("complete first");
    assert_eq!(first_completed.status(), StatusCode::OK);
    let first_completed_body = json_body(first_completed).await;
    let old_qr_payload = first_completed_body["progress_batch"]["qr_payload"]
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
                    "apparatus":"apparatus:default:paket",
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
                "apparatus":"apparatus:default:paket",
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
                "apparatus:default:asset-010",
                "apparatus:default:paket",
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
                "apparatus":"apparatus:default:asset-010",
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
    let worker_report_status = worker_report.status();
    let worker_report_body = json_body(worker_report).await;
    assert_eq!(
        worker_report_status,
        StatusCode::OK,
        "{worker_report_body:?}"
    );
    assert_eq!(
        worker_report_body["scanned_batch"]["qr_payload"],
        old_qr_payload
    );

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
        report_body["current_batch"]["status_detail"]["work_status"], "completed",
        "{report_body:?}"
    );
    assert_eq!(
        report_body["current_batch"]["status_detail"]["flow_status"],
        "free_wip"
    );
    assert!(
        report_body["current_batch"]["status_detail"]
            .get("stock_status")
            .is_none()
    );
    assert_eq!(report_body["order_status"]["order_status"], "completed");
    assert_eq!(report_body["order_status"]["flow_status"], "free_wip");
    assert_eq!(report_body["is_stale"], true);
    assert_eq!(report_body["stale_reason"], "processed_by_next_stage");
    assert_eq!(report_body["order"]["id"], "zakaz-qr-report");
    assert_eq!(report_body["order"]["title"], "QR report order");
    assert_eq!(
        report_body["queue_states"]["apparatus:default:paket"]["zakaz-qr-report"],
        "completed"
    );
    assert!(
        report_body["queue_states"]["apparatus:default:asset-010"]
            .get("zakaz-qr-unrelated")
            .is_none()
    );
    assert_eq!(report_body["logs"].as_array().expect("logs").len(), 4);
    assert!(
        report_body["corrections"]
            .as_array()
            .expect("corrections")
            .is_empty()
    );
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
        ("worker-qr-history-a", "apparatus:default:bosma_7"),
        ("worker-qr-history-b", "apparatus:default:bosma_8"),
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
    let mut other_batch_id = String::new();
    for (order_id, order_number, apparatus, token) in [
        (
            "zakaz-qr-history-a",
            "9503",
            "apparatus:default:bosma_7",
            &worker_a_token,
        ),
        (
            "zakaz-qr-history-b",
            "9504",
            "apparatus:default:bosma_8",
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
            other_batch_id = paused_body["progress_batch"]["batch_id"]
                .as_str()
                .expect("other batch id")
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
    let own_qr = batches[0]["qr_payload"]
        .as_str()
        .expect("own qr")
        .to_string();
    let own_batch_id = batches[0]["batch_id"]
        .as_str()
        .expect("own batch id")
        .to_string();
    assert_eq!(batches[0]["revision"], 1);
    wait_for_progress_print_request_count(&print_requests, 2).await;

    let corrected = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/progress-qr/correct",
            &worker_a_token,
            &format!(
                r#"{{
                    "batch_id":"{own_batch_id}",
                    "expected_revision":1,
                    "produced_qty":13,
                    "uom":"kg",
                    "description":"corrected",
                    "reason":"O'lchov noto'g'ri kiritilgan"
                }}"#,
            ),
        ))
        .await
        .expect("correct own progress batch");
    let corrected_status = corrected.status();
    let corrected_body = json_body(corrected).await;
    assert_eq!(corrected_status, StatusCode::OK, "{corrected_body:?}");
    assert_eq!(corrected_body["batch"]["revision"], 2);
    assert_eq!(corrected_body["batch"]["produced_qty"], 13.0);

    let correction_report = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/progress-qr/report",
            &admin_token,
            &format!(r#"{{"qr_payload":"{own_qr}"}}"#),
        ))
        .await
        .expect("qr report with correction audit");
    let correction_report_status = correction_report.status();
    let correction_report_body = json_body(correction_report).await;
    assert_eq!(
        correction_report_status,
        StatusCode::OK,
        "{correction_report_body:?}"
    );
    let corrections = correction_report_body["corrections"]
        .as_array()
        .expect("corrections");
    assert_eq!(corrections.len(), 1);
    assert_eq!(corrections[0]["batch_id"], own_batch_id);
    assert_eq!(corrections[0]["previous_revision"], 1);
    assert_eq!(corrections[0]["new_revision"], 2);
    assert_eq!(corrections[0]["reason"], "O'lchov noto'g'ri kiritilgan");
    assert_eq!(corrections[0]["actor"]["ref_"], "worker-qr-history-a");
    assert_eq!(corrections[0]["old_values"]["produced_qty"], 12.0);
    assert_eq!(corrections[0]["new_values"]["produced_qty"], 13.0);

    let forbidden_correction = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/progress-qr/correct",
            &worker_a_token,
            &format!(
                r#"{{
                    "batch_id":"{other_batch_id}",
                    "expected_revision":1,
                    "produced_qty":13,
                    "uom":"kg",
                    "reason":"not owner"
                }}"#,
            ),
        ))
        .await
        .expect("reject other worker correction");
    assert_eq!(forbidden_correction.status(), StatusCode::FORBIDDEN);

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
    wait_for_progress_print_request_count(&print_requests, 3).await;
    let printed = print_requests.lock().await;
    assert_eq!(printed.len(), 3);
    assert!(printed.iter().any(|request| request.epc == own_qr));
    let reprint_request = printed.last().expect("reprint request");
    assert_eq!(reprint_request.epc, own_qr);
    assert!(reprint_request.item_name.contains("tayyor mahsulot"));
    assert!(!reprint_request.item_name.contains("yarim tayyor mahsulot"));
}
