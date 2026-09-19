use super::*;

const APPARATUS: &str = "apparatus:default:asset-010";
const PATH: &str = "/v1/mobile/admin/production-maps/paddons/active";

#[tokio::test]
async fn active_paddon_is_shared_by_sessions_but_isolated_by_authenticated_user() {
    let state = test_state();
    for worker in ["worker-a", "worker-b"] {
        state
            .admin
            .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: worker.into(),
                role_id: "aparatchi".into(),
                assigned_apparatus: vec![APPARATUS.into()],
                assigned_item_groups: vec![],
            })
            .await
            .unwrap();
    }
    let first = session_for(&state, PrincipalRole::Aparatchi, "worker-a").await;
    let second = session_for(&state, PrincipalRole::Aparatchi, "worker-a").await;
    let other = session_for(&state, PrincipalRole::Aparatchi, "worker-b").await;
    let admin = session_for(&state, PrincipalRole::Admin, "worker-a").await;
    let created = state
        .production_maps
        .create_paddon(
            "",
            "",
            &crate::core::production_map::QueueActionActor {
                role: "admin".into(),
                ref_: "admin".into(),
                display_name: "Admin".into(),
            },
        )
        .await
        .unwrap();
    let router = build_router(state);
    let url = format!("{PATH}?apparatus={APPARATUS}");
    let body = serde_json::json!({"apparatus":APPARATUS,"code":created.code}).to_string();
    let saved = router
        .clone()
        .oneshot(request_with_body("PUT", PATH, &first, &body))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(json_body(saved).await["code"], created.code);
    for (token, expected) in [
        (&first, Some(created.code.as_str())),
        (&second, Some(created.code.as_str())),
        (&other, None),
        (&admin, None),
    ] {
        let response = router
            .clone()
            .oneshot(request("GET", &url, token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_body(response).await["code"].as_str(), expected);
    }
    // Invalid and forged scope changes cannot clear or replace the saved selection.
    for body in [
        serde_json::json!({"apparatus":APPARATUS,"code":"missing"}),
        serde_json::json!({"apparatus":APPARATUS}),
        serde_json::json!({"apparatus":APPARATUS,"code":"", "actor_ref":"worker-b"}),
        serde_json::json!({"apparatus":"apparatus:default:asset-007","code":""}),
    ] {
        let rejected = router
            .clone()
            .oneshot(request_with_body("PUT", PATH, &first, &body.to_string()))
            .await
            .unwrap();
        assert!(!rejected.status().is_success());
    }
    let response = router
        .clone()
        .oneshot(request("GET", &url, &second))
        .await
        .unwrap();
    assert_eq!(json_body(response).await["code"], created.code);
    let cleared = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            PATH,
            &second,
            &serde_json::json!({"apparatus":APPARATUS,"code":""}).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(cleared.status(), StatusCode::OK);
    let response = router
        .clone()
        .oneshot(request("GET", &url, &first))
        .await
        .unwrap();
    assert!(json_body(response).await["code"].is_null());
    let denied = router.oneshot(request("GET", &url, "")).await.unwrap();
    assert!(!denied.status().is_success());
}

#[tokio::test]
async fn active_paddon_rejects_non_cut_apparatus_even_for_admin() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            PATH,
            &token,
            r#"{"apparatus":"apparatus:default:asset-007","code":""}"#,
        ))
        .await
        .unwrap();
    assert!(!response.status().is_success());
}
