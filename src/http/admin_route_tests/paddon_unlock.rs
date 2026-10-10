use super::*;
use crate::core::production_map::QueueActionActor;

const UNLOCK: &str = "/v1/mobile/admin/production-maps/paddons/unlock";

#[tokio::test]
async fn paddon_unlock_permissions_follow_lock_owner_and_the_admin_setting() {
    let state = test_state();
    state.production_maps.update_paddon_settings(None, Some(true), &QueueActionActor {
        role: "admin".into(), ref_: "admin".into(), display_name: "Admin".into(),
    }).await.unwrap();
    for worker in ["lock-owner", "other-worker"] {
        state
            .admin
            .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: worker.into(),
                role_id: "aparatchi".into(),
                assigned_apparatus: vec!["apparatus:default:asset-010".into()],
                assigned_item_groups: vec![],
            })
            .await
            .unwrap();
    }
    let owner = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "lock-owner".into(),
        display_name: "Owner".into(),
    };
    let paddon = state
        .production_maps
        .create_paddon("", "", &owner)
        .await
        .unwrap();
    state
        .production_maps
        .confirm_paddon_print(&paddon.code, &owner)
        .await
        .unwrap();
    let owner_token = session_for(&state, PrincipalRole::Aparatchi, "lock-owner").await;
    let other_token = session_for(&state, PrincipalRole::Aparatchi, "other-worker").await;
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let customer = session(&state, PrincipalRole::Customer).await;
    let router = build_router(state.clone());
    let body = serde_json::json!({"code": paddon.code}).to_string();
    for path in ["detail", "qr/report"] {
        let path = format!(
            "/v1/mobile/admin/production-maps/paddons/{path}?code={}",
            paddon.code
        );
        for (token, allowed) in [
            (&owner_token, true),
            (&other_token, false),
            (&admin_token, true),
        ] {
            let response = router
                .clone()
                .oneshot(request("GET", &path, token))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(json_body(response).await["can_unlock"], allowed);
        }
    }
    for (token, expected) in [
        ("", StatusCode::UNAUTHORIZED),
        (customer.as_str(), StatusCode::FORBIDDEN),
        (other_token.as_str(), StatusCode::FORBIDDEN),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body("POST", UNLOCK, token, &body))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if token == other_token {
            assert_eq!(
                json_body(response).await["error"],
                "paddon_unlock_forbidden"
            );
        }
    }
    assert!(
        state
            .production_maps
            .paddon_snapshot(&paddon.code)
            .await
            .unwrap()
            .paddon
            .locked_at_unix
            .is_some()
    );
    for invalid in [
        r#"{"code":" "}"#.to_string(),
        serde_json::json!({"code":paddon.code,"actor_ref":"lock-owner"}).to_string(),
        "{".to_string(),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body("POST", UNLOCK, &owner_token, &invalid))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let response = router
        .clone()
        .oneshot(request("GET", UNLOCK, &owner_token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    let response = router
        .clone()
        .oneshot(request_with_body("POST", UNLOCK, &owner_token, &body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert!(value["paddon"]["locked_at_unix"].is_null());
    assert_eq!(value["can_manage_items"], true);
    assert_eq!(value["can_unlock"], false);
    state
        .production_maps
        .confirm_paddon_print(&paddon.code, &owner)
        .await
        .unwrap();
    let response = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/paddons/management-settings",
            &admin_token,
            r#"{"free_movement_enabled":true}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = router
        .clone()
        .oneshot(request(
            "GET",
            &format!(
                "/v1/mobile/admin/production-maps/paddons/detail?code={}",
                paddon.code
            ),
            &other_token,
        ))
        .await
        .unwrap();
    assert_eq!(json_body(response).await["can_unlock"], true);
    let response = router
        .oneshot(request_with_body("POST", UNLOCK, &other_token, &body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(json_body(response).await["paddon"]["locked_at_unix"].is_null());
}
