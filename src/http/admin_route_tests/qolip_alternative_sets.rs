use super::*;

#[tokio::test]
async fn qolip_alternative_sets_accept_one_complete_batch_and_reject_mixing() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "alternative-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![
                "apparatus:default:bosma_7".into(),
                "apparatus:default:bosma_8".into(),
            ],
            assigned_item_groups: Vec::new(),
        })
        .await
        .unwrap();
    let admin = session(&state, PrincipalRole::Admin).await;
    let worker = session_for(&state, PrincipalRole::Aparatchi, "alternative-worker").await;
    let router = build_router(state);
    for (id, number, apparatus, colors) in [
        (
            "zakaz-alternative-a",
            "7601",
            "apparatus:default:bosma_7",
            7,
        ),
        (
            "zakaz-alternative-b",
            "7602",
            "apparatus:default:bosma_8",
            8,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &admin,
                &production_order_map_json_with_product(
                    id,
                    "Same product",
                    "ITEM-ALTERNATIVE",
                    number,
                    apparatus,
                    colors,
                    850.0,
                ),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let mut set_ids = Vec::new();
    for codes in [vec!["ALT-A1", "ALT-A2"], vec!["ALT-B1", "ALT-B2", "ALT-B3"]] {
        let specs: Vec<_> = codes
            .iter()
            .map(|code| {
                serde_json::json!({
                    "item_code": "ITEM-ALTERNATIVE", "item_name": "Same product",
                    "item_group": "Tayyor mahsulot", "warehouse": "Qolip ombor",
                    "qolip_code": code, "size": 42,
                })
            })
            .collect();
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/qolip/product-specs/batch",
                &admin,
                &serde_json::json!({"specs": specs}).to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        let products = body["products"].as_array().unwrap();
        let id = products[0]["qolip_set_id"].as_str().unwrap();
        assert!(!id.is_empty());
        assert!(products.iter().all(|product| product["qolip_set_id"] == id));
        set_ids.push(id.to_string());
    }
    assert_ne!(set_ids[0], set_ids[1]);
    for (codes, error) in [
        (vec!["ALT-A1"], "qolip_scan_incomplete"),
        (vec!["ALT-B1", "ALT-B2"], "qolip_scan_incomplete"),
        (vec!["ALT-A1", "ALT-B1"], "qolip_scan_mixed_sets"),
        (
            vec!["ALT-A1", "ALT-A2", "ALT-B1", "ALT-B2", "ALT-B3"],
            "qolip_scan_mixed_sets",
        ),
    ] {
        let response = router.clone().oneshot(request_with_body("POST",
            "/v1/mobile/admin/production-maps/queue-action", &worker,
            &serde_json::json!({"apparatus": "apparatus:default:bosma_7", "order_id": "zakaz-alternative-a",
                "action": "start", "qolip_codes": codes}).to_string(),
        )).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(response).await["error"], error);
    }
    for (order_id, apparatus, codes, id) in [
        (
            "zakaz-alternative-a",
            "apparatus:default:bosma_7",
            vec!["ALT-B3", "alt-b1", "ALT-B2", "ALT-B2"],
            &set_ids[1],
        ),
        (
            "zakaz-alternative-b",
            "apparatus:default:bosma_8",
            vec!["ALT-A1", "ALT-A2"],
            &set_ids[0],
        ),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/admin/production-maps/queue-action",
                &worker,
                &serde_json::json!({"apparatus": apparatus, "order_id": order_id,
                "action": "start", "qolip_codes": codes})
                .to_string(),
            ))
            .await
            .unwrap();
        let status = response.status();
        let body = json_body(response).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["session"]["payload_json"]["qolip_set_id"], id.as_str());
        assert_eq!(body["session"]["payload_json"]["qolip_lock_owner"], true);
    }
}
