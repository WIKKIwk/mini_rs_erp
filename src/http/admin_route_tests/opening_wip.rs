use super::*;

const LAMINATION_ID: &str = "apparatus:default:asset-007";

#[tokio::test]
async fn opening_wip_worker_lookup_is_qr_exact_and_apparatus_scoped() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "opening-wip-worker".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec![LAMINATION_ID.to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("worker assignment");
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "opening-wip-other-worker".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("other worker assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "opening-wip-worker").await;
    let other_worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "opening-wip-other-worker").await;
    let router = build_router(state);
    let order_id = "zakaz-opening-wip-worker-lookup";

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json_with_dims(
                order_id,
                "Opening WIP worker lookup",
                "OWIP-LOOKUP",
                LAMINATION_ID,
                1,
                900.0,
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);

    let mismatched_location = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/opening-wip",
            &admin_token,
            &format!(
                r#"{{
                    "idempotency_key":"opening-wip-location-mismatch",
                    "order_id":"{order_id}",
                    "entry_apparatus":"{LAMINATION_ID}",
                    "current_location":"apparatus:default:asset-008",
                    "batches":[{{
                        "quantity_basis":"measured",
                        "finished_goods_meter":100.0,
                        "finished_goods_kg":12.0,
                        "bobina_kg":1.0
                    }}]
                }}"#
            ),
        ))
        .await
        .expect("reject mismatched Opening WIP location");
    assert_eq!(mismatched_location.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(mismatched_location).await["error"],
        "opening_wip_location_mismatch"
    );

    let created = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/opening-wip",
            &admin_token,
            &format!(
                r#"{{
                    "idempotency_key":"opening-wip-worker-lookup-request",
                    "order_id":"{order_id}",
                    "entry_apparatus":"{LAMINATION_ID}",
                    "current_location":"{LAMINATION_ID}",
                    "batches":[{{
                        "quantity_basis":"measured",
                        "finished_goods_meter":100.0,
                        "finished_goods_kg":12.0,
                        "bobina_kg":1.0
                    }}]
                }}"#
            ),
        ))
        .await
        .expect("create Opening WIP");
    let created_status = created.status();
    let created_body = json_body(created).await;
    assert_eq!(created_status, StatusCode::OK, "{created_body:?}");
    let batch_id = created_body["record"]["batches"][0]["batch_id"]
        .as_str()
        .expect("batch id");
    let qr_payload = created_body["record"]["batches"][0]["qr_payload"]
        .as_str()
        .expect("qr payload");
    let lookup_body = format!(
        r#"{{
            "apparatus":"{LAMINATION_ID}",
            "order_id":"{order_id}",
            "batch_id":"{batch_id}",
            "qr_payload":"{qr_payload}"
        }}"#
    );

    let candidates = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/opening-wip/lookup",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"{LAMINATION_ID}",
                    "order_id":"{order_id}",
                    "qr_payload":""
                }}"#
            ),
        ))
        .await
        .expect("worker candidates");
    let candidates_status = candidates.status();
    let candidates_response = json_body(candidates).await;
    assert_eq!(
        candidates_status,
        StatusCode::OK,
        "{candidates_response:?}"
    );
    assert_eq!(candidates_response["batches"].as_array().map(Vec::len), Some(1));
    assert_eq!(candidates_response["batches"][0]["batch_id"], batch_id);
    assert_eq!(candidates_response["batches"][0]["qr_payload"], qr_payload);

    let lookup = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/opening-wip/lookup",
            &worker_token,
            &lookup_body,
        ))
        .await
        .expect("worker lookup");
    let lookup_status = lookup.status();
    let lookup_response = json_body(lookup).await;
    assert_eq!(lookup_status, StatusCode::OK, "{lookup_response:?}");
    assert_eq!(lookup_response["batch"]["batch_id"], batch_id);
    assert_eq!(lookup_response["batch"]["qr_payload"], qr_payload);

    let wrong_order = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/opening-wip/lookup",
            &worker_token,
            &lookup_body.replace(order_id, "zakaz-other"),
        ))
        .await
        .expect("wrong order lookup");
    assert_eq!(wrong_order.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(wrong_order).await["error"],
        "opening_wip_qr_mismatch"
    );

    let wrong_worker = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/opening-wip/lookup",
            &other_worker_token,
            &lookup_body,
        ))
        .await
        .expect("unassigned worker lookup");
    assert_eq!(wrong_worker.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(wrong_worker).await["error"],
        "apparatus_not_assigned"
    );

    let list = router
        .oneshot(request(
            "GET",
            &format!(
                "/v1/mobile/admin/production-maps/opening-wip?order_id={order_id}&status=waiting"
            ),
            &worker_token,
        ))
        .await
        .expect("worker list denied");
    assert_eq!(list.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn opening_wip_from_print_source_opens_lamination_scan_and_starts_with_its_qr() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "opening-wip-source-worker".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec![LAMINATION_ID.to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("lamination worker assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(
        &state,
        PrincipalRole::Aparatchi,
        "opening-wip-source-worker",
    )
    .await;
    let router = build_router(state);
    let order_id = "zakaz-opening-wip-print-source";

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &two_apparatus_order_map_json(
                order_id,
                "Opening WIP print to lamination",
                "OWIP-SOURCE",
                "apparatus:default:bosma_7",
                LAMINATION_ID,
            ),
        ))
        .await
        .expect("save print to lamination map");
    assert_eq!(saved.status(), StatusCode::OK);

    let sequence = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
            &format!(
                r#"{{
                    "apparatus":"{LAMINATION_ID}",
                    "order_ids":["{order_id}"]
                }}"#
            ),
        ))
        .await
        .expect("set lamination sequence");
    assert_eq!(sequence.status(), StatusCode::OK);

    let created = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/opening-wip",
            &admin_token,
            &format!(
                r#"{{
                    "idempotency_key":"opening-wip-print-source-request",
                    "order_id":"{order_id}",
                    "source_apparatus":"apparatus:default:bosma_7",
                    "source_stage_node_id":"first",
                    "batches":[{{
                        "quantity_basis":"measured",
                        "finished_goods_meter":100.0,
                        "finished_goods_kg":12.0,
                        "bobina_kg":1.0
                    }}]
                }}"#
            ),
        ))
        .await
        .expect("create Opening WIP from print source");
    let created_status = created.status();
    let created_body = json_body(created).await;
    assert_eq!(created_status, StatusCode::OK, "{created_body:?}");
    let intake = &created_body["record"]["intake"];
    assert_eq!(intake["entry_apparatus"], "apparatus:default:bosma_7");
    assert_eq!(intake["source_apparatus"], "apparatus:default:bosma_7");
    assert_eq!(intake["source_operation"], "print");
    assert_eq!(intake["current_location"], "");
    assert_eq!(intake["resume_apparatus"], "");
    assert_eq!(intake["resume_stage_node_id"], "first");
    let batch_id = created_body["record"]["batches"][0]["batch_id"]
        .as_str()
        .expect("Opening WIP batch ID");
    let qr_payload = created_body["record"]["batches"][0]["qr_payload"]
        .as_str()
        .expect("Opening WIP QR");

    let snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &worker_token,
        ))
        .await
        .expect("lamination queue snapshot");
    let snapshot_status = snapshot.status();
    let snapshot_body = json_body(snapshot).await;
    assert_eq!(snapshot_status, StatusCode::OK, "{snapshot_body:?}");
    let control = &snapshot_body["queue_action_controls"][LAMINATION_ID][order_id];
    assert_eq!(control["interaction"]["opening_wip_mode"], "scan_required");
    assert_eq!(control["interaction"]["previous_wip_mode"], "not_required");
    assert!(
        control["allowed_actions"]
            .as_array()
            .expect("allowed actions")
            .iter()
            .any(|action| action == "start")
    );

    let lookup = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/opening-wip/lookup",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"{LAMINATION_ID}",
                    "order_id":"{order_id}",
                    "batch_id":"{batch_id}",
                    "qr_payload":"{qr_payload}"
                }}"#
            ),
        ))
        .await
        .expect("lookup print-source QR at lamination");
    let lookup_status = lookup.status();
    let lookup_body = json_body(lookup).await;
    assert_eq!(lookup_status, StatusCode::OK, "{lookup_body:?}");
    assert_eq!(lookup_body["batch"]["batch_id"], batch_id);
    assert_eq!(lookup_body["batch"]["qr_payload"], qr_payload);

    let start_without_qr = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"{LAMINATION_ID}",
                    "order_id":"{order_id}",
                    "action":"start"
                }}"#
            ),
        ))
        .await
        .expect("reject unscanned Opening WIP start");
    let start_without_qr_status = start_without_qr.status();
    let start_without_qr_body = json_body(start_without_qr).await;
    assert_eq!(
        start_without_qr_status,
        StatusCode::BAD_REQUEST,
        "{start_without_qr_body:?}"
    );
    assert_eq!(start_without_qr_body["error"], "progress_qr_required");

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"{LAMINATION_ID}",
                    "order_id":"{order_id}",
                    "action":"start",
                    "qr_payload":"{qr_payload}"
                }}"#
            ),
        ))
        .await
        .expect("start lamination from Opening WIP QR");
    let started_status = started.status();
    let started_body = json_body(started).await;
    assert_eq!(started_status, StatusCode::OK, "{started_body:?}");
    assert_eq!(started_body["states"][order_id], "in_progress");

    let in_use = router
        .clone()
        .oneshot(request(
            "GET",
            &format!(
                "/v1/mobile/admin/production-maps/opening-wip?order_id={order_id}&status=in_use"
            ),
            &admin_token,
        ))
        .await
        .expect("Opening WIP in-use state");
    let in_use_status = in_use.status();
    let in_use_body = json_body(in_use).await;
    assert_eq!(in_use_status, StatusCode::OK, "{in_use_body:?}");
    assert_eq!(in_use_body["records"][0]["batches"][0]["wip_status"], "in_use");
    assert_eq!(
        in_use_body["records"][0]["batches"][0]["used_by_apparatus"],
        LAMINATION_ID
    );

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"{LAMINATION_ID}",
                    "order_id":"{order_id}",
                    "action":"complete",
                    "finished_goods_meter":100.0,
                    "finished_goods_kg":12.0,
                    "lamination_print_leftover_rolls":0.5,
                    "lamination_film_leftover_rolls":0.5,
                    "total_waste":0.5,
                    "uom":"m"
                }}"#
            ),
        ))
        .await
        .expect("complete lamination from Opening WIP QR");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body:?}");
    assert_eq!(completed_body["states"][order_id], "completed");

    let repeated = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/opening-wip",
            &admin_token,
            &format!(
                r#"{{
                    "idempotency_key":"opening-wip-print-source-request-again",
                    "order_id":"{order_id}",
                    "source_apparatus":"apparatus:default:bosma_7",
                    "source_stage_node_id":"first",
                    "batches":[{{
                        "quantity_basis":"measured",
                        "finished_goods_meter":100.0,
                        "finished_goods_kg":12.0,
                        "bobina_kg":1.0
                    }}]
                }}"#
            ),
        ))
        .await
        .expect("reject repeated Opening WIP source");
    let repeated_status = repeated.status();
    let repeated_body = json_body(repeated).await;
    assert_eq!(repeated_status, StatusCode::CONFLICT, "{repeated_body:?}");
    assert_eq!(
        repeated_body["error"],
        "opening_wip_target_stage_already_completed"
    );
}
