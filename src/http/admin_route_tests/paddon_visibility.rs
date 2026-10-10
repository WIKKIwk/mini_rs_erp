use super::*;

const BASE: &str = "/v1/mobile/admin/production-maps/paddons";
const CUT: &str = "apparatus:default:asset-010";

#[tokio::test]
async fn paddon_visibility_defaults_to_owner_and_admin_can_enable_and_revoke_sharing() {
    let state = test_state();
    for worker in ["paddon-worker-a", "paddon-worker-b"] {
        state
            .admin
            .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: worker.into(),
                role_id: "aparatchi".into(),
                assigned_apparatus: vec![CUT.into()],
                assigned_item_groups: vec![],
            })
            .await
            .unwrap();
    }
    let admin = session(&state, PrincipalRole::Admin).await;
    let a = session_for(&state, PrincipalRole::Aparatchi, "paddon-worker-a").await;
    let b = session_for(&state, PrincipalRole::Aparatchi, "paddon-worker-b").await;
    let router = build_router(state);
    let settings_path = format!("{BASE}/management-settings");
    let response = router
        .clone()
        .oneshot(request("GET", &settings_path, &admin))
        .await
        .unwrap();
    assert_eq!(
        json_body(response).await["settings"]["worker_visibility_enabled"],
        false
    );

    let mut codes = Vec::new();
    for token in [&b, &a] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                &format!("{BASE}/create"),
                token,
                "{}",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        codes.push(
            json_body(response).await["paddon"]["code"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    let other_code = &codes[0];
    let own_code = &codes[1];
    for (token, code) in [(&a, own_code), (&b, other_code)] {
        for query in [
            "limit=1",
            "limit=1&selectable_only=true&creator_ref=paddon-worker-b",
        ] {
            let response = router
                .clone()
                .oneshot(request("GET", &format!("{BASE}?{query}"), token))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let value = json_body(response).await;
            assert_eq!(value["paddons"].as_array().unwrap().len(), 1);
            assert_eq!(value["paddons"][0]["code"], code.as_str());
        }
    }
    let response = router
        .clone()
        .oneshot(request("GET", BASE, &admin))
        .await
        .unwrap();
    assert_eq!(
        json_body(response).await["paddons"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    for suffix in ["detail", "qr/report"] {
        let response = router
            .clone()
            .oneshot(request(
                "GET",
                &format!("{BASE}/{suffix}?code={own_code}"),
                &a,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = router
            .clone()
            .oneshot(request(
                "GET",
                &format!("{BASE}/{suffix}?code={other_code}"),
                &a,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    for suffix in [
        "qr/print",
        "qr/confirm",
        "unlock",
        "delete",
        "items/add",
        "items/add-batch",
        "items/remove",
        "items/remove-batch",
        "active/next",
    ] {
        let body = serde_json::json!({"code":other_code,"apparatus":CUT,"progress_batch_id":"wip","progress_batch_ids":["wip"]});
        // Endpoints with strict bodies receive only their documented fields.
        let body = match suffix {
            "qr/confirm" | "unlock" | "delete" => serde_json::json!({"code":other_code}),
            "active/next" => serde_json::json!({"code":other_code,"apparatus":CUT}),
            _ => body,
        };
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                &format!("{BASE}/{suffix}"),
                &a,
                &body.to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{suffix}");
    }
    let selection = serde_json::json!({"apparatus":CUT,"code":other_code}).to_string();
    let response = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            &format!("{BASE}/active"),
            &a,
            &selection,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Moving rolls and seeing other workers' paddons are independent settings.
    let response = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            &settings_path,
            &admin,
            r#"{"free_movement_enabled":true}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["settings"]["worker_visibility_enabled"],
        false
    );
    let response = router
        .clone()
        .oneshot(request(
            "GET",
            &format!("{BASE}/detail?code={other_code}"),
            &a,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            &settings_path,
            &a,
            r#"{"worker_visibility_enabled":true}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    for enabled in [true, false] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "PUT",
                &settings_path,
                &admin,
                &serde_json::json!({"worker_visibility_enabled":enabled}).to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let settings = json_body(response).await;
        assert_eq!(settings["settings"]["worker_visibility_enabled"], enabled);
        assert_eq!(settings["settings"]["free_movement_enabled"], true);
        for token in [&a, &b] {
            let response = router
                .clone()
                .oneshot(request("GET", BASE, token))
                .await
                .unwrap();
            assert_eq!(
                json_body(response).await["paddons"]
                    .as_array()
                    .unwrap()
                    .len(),
                if enabled { 2 } else { 1 }
            );
        }
        let response = router
            .clone()
            .oneshot(request(
                "GET",
                &format!("{BASE}/detail?code={other_code}"),
                &a,
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if enabled {
                StatusCode::OK
            } else {
                StatusCode::NOT_FOUND
            }
        );
        if enabled {
            let response = router
                .clone()
                .oneshot(request_with_body(
                    "PUT",
                    &format!("{BASE}/active"),
                    &a,
                    &selection,
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = router
            .clone()
            .oneshot(request(
                "GET",
                &format!("{BASE}/active?apparatus={CUT}"),
                &a,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            json_body(response).await["code"].as_str(),
            enabled.then_some(other_code.as_str())
        );
    }
}
