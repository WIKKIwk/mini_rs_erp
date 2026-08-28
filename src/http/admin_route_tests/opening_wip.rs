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
    let worker_token = session_for(
        &state,
        PrincipalRole::Aparatchi,
        "opening-wip-worker",
    )
    .await;
    let other_worker_token = session_for(
        &state,
        PrincipalRole::Aparatchi,
        "opening-wip-other-worker",
    )
    .await;
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
                    "source_operation":"Bosma",
                    "current_location":"Laminatsiya oldi",
                    "batches":[{{"quantity_basis":"unknown"}}]
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
