use super::*;

const PATH: &str = "/v1/mobile/admin/production-maps/paddons/management-settings";

#[tokio::test]
async fn paddon_free_movement_is_off_by_default_and_only_admin_can_toggle_it() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "settings-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec!["apparatus:default:asset-010".into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let admin = session(&state, PrincipalRole::Admin).await;
    let worker = session_for(&state, PrincipalRole::Aparatchi, "settings-worker").await;
    let customer = session(&state, PrincipalRole::Customer).await;
    let router = build_router(state.clone());
    let read = router
        .clone()
        .oneshot(request("GET", PATH, &worker))
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::OK);
    assert_eq!(
        json_body(read).await["settings"]["free_movement_enabled"],
        false
    );
    for (token, expected) in [
        ("", StatusCode::UNAUTHORIZED),
        (worker.as_str(), StatusCode::FORBIDDEN),
        (customer.as_str(), StatusCode::FORBIDDEN),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "PUT",
                PATH,
                token,
                r#"{"free_movement_enabled":true}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    for body in [
        r#"{}"#,
        r#"{"free_movement_enabled":"true"}"#,
        r#"{"worker_visibility_enabled":"true"}"#,
        r#"{"free_movement_enabled":true,"actor_ref":"admin"}"#,
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body("PUT", PATH, &admin, body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            PATH,
            &admin,
            r#"{"free_movement_enabled":true}"#,
        ))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(
        json_body(saved).await["settings"]["free_movement_enabled"],
        true
    );
    let read = router
        .clone()
        .oneshot(request("GET", PATH, &worker))
        .await
        .unwrap();
    assert_eq!(
        json_body(read).await["settings"]["free_movement_enabled"],
        true
    );
    // A worker cannot masquerade as admin by changing its role assignment.
    let masquerade = state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "settings-worker".into(),
            role_id: "admin".into(),
            assigned_apparatus: vec![],
            assigned_item_groups: vec![],
        })
        .await;
    assert!(masquerade.is_err());
    let denied = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            PATH,
            &worker,
            r#"{"free_movement_enabled":false}"#,
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let off = router
        .oneshot(request_with_body(
            "PUT",
            PATH,
            &admin,
            r#"{"free_movement_enabled":false}"#,
        ))
        .await
        .unwrap();
    assert_eq!(off.status(), StatusCode::OK);
    assert_eq!(
        json_body(off).await["settings"]["free_movement_enabled"],
        false
    );
}
