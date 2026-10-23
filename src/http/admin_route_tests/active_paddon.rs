use super::*;

const APPARATUS: &str = "apparatus:default:asset-010";
const PATH: &str = "/v1/mobile/admin/production-maps/paddons/active";

#[tokio::test]
async fn paddon_delete_requires_write_access_and_valid_input() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "delete-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![APPARATUS.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let path = "/v1/mobile/admin/production-maps/paddons/delete";
    let admin = session(&state, PrincipalRole::Admin).await;
    let worker = session_for(&state, PrincipalRole::Aparatchi, "delete-worker").await;
    let customer = session(&state, PrincipalRole::Customer).await;
    let router = build_router(state);
    for (token, expected) in [
        ("", StatusCode::UNAUTHORIZED),
        (customer.as_str(), StatusCode::FORBIDDEN),
        (admin.as_str(), StatusCode::BAD_REQUEST),
        (worker.as_str(), StatusCode::BAD_REQUEST),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body("POST", path, token, r#"{"code":" "}"#))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let response = router.oneshot(request("GET", path, &admin)).await.unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

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
        .create_paddon_for_apparatus(
            Some(APPARATUS),
            "",
            "",
            &crate::core::production_map::QueueActionActor {
                role: "aparatchi".into(),
                ref_: "worker-a".into(),
                display_name: "Worker A".into(),
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

#[tokio::test]
async fn worker_paddon_inventories_and_active_selections_are_isolated_by_cut_apparatus() {
    const SECOND: &str = "apparatus:default:asset-011";
    const LAMINATE: &str = "apparatus:default:asset-007";
    const BASE: &str = "/v1/mobile/admin/production-maps/paddons";
    let state = test_state();
    state.apparatus.bootstrap_factory_defaults().await.unwrap();
    state.admin.upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
        principal_role: PrincipalRole::Aparatchi,
        principal_ref: "multi-cut-worker".into(), role_id: "aparatchi".into(),
        assigned_apparatus: vec![APPARATUS.into(), SECOND.into(), LAMINATE.into()],
        assigned_item_groups: vec![],
    }).await.unwrap();
    let token = session_for(&state, PrincipalRole::Aparatchi, "multi-cut-worker").await;
    let router = build_router(state.clone());
    let missing_scope = router.clone().oneshot(request_with_body("POST", &format!("{BASE}/create"), &token, "{}")).await.unwrap();
    assert_eq!(missing_scope.status(), StatusCode::BAD_REQUEST);
    let missing_list_scope = router.clone().oneshot(request("GET", BASE, &token)).await.unwrap();
    assert_eq!(missing_list_scope.status(), StatusCode::BAD_REQUEST);
    let mut codes = Vec::new();
    for apparatus in [APPARATUS, SECOND] {
        let created = router.clone().oneshot(request_with_body("POST", &format!("{BASE}/create"), &token,
            &serde_json::json!({"apparatus":apparatus,"location":"Rezka"}).to_string())).await.unwrap();
        assert_eq!(created.status(), StatusCode::OK);
        let created = json_body(created).await;
        assert_eq!(created["paddon"]["apparatus_id"], apparatus);
        codes.push(created["paddon"]["code"].as_str().unwrap().to_string());
    }
    for (apparatus, code) in [APPARATUS, SECOND].into_iter().zip(&codes) {
        let page = router.clone().oneshot(request("GET", &format!("{BASE}?apparatus={apparatus}&selectable_only=true"), &token)).await.unwrap();
        assert_eq!(page.status(), StatusCode::OK);
        let page = json_body(page).await;
        assert_eq!(page["paddons"].as_array().unwrap().len(), 1);
        assert_eq!(page["paddons"][0]["code"], code.as_str());
        let selected = router.clone().oneshot(request_with_body("PUT", PATH, &token,
            &serde_json::json!({"apparatus":apparatus,"code":code}).to_string())).await.unwrap();
        assert_eq!(selected.status(), StatusCode::OK);
    }
    let rejected = router.clone().oneshot(request_with_body("PUT", PATH, &token,
        &serde_json::json!({"apparatus":SECOND,"code":codes[0]}).to_string())).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    for (apparatus, code) in [APPARATUS, SECOND].into_iter().zip(&codes) {
        let selected = router.clone().oneshot(request("GET", &format!("{PATH}?apparatus={apparatus}"), &token)).await.unwrap();
        assert_eq!(selected.status(), StatusCode::OK);
        assert_eq!(json_body(selected).await["code"], code.as_str());
    }
    for apparatus in [LAMINATE, "apparatus:default:asset-012"] {
        let page = router.clone().oneshot(request("GET", &format!("{BASE}?apparatus={apparatus}"), &token)).await.unwrap();
        assert_eq!(page.status(), StatusCode::BAD_REQUEST);
        let created = router.clone().oneshot(request_with_body("POST", &format!("{BASE}/create"), &token,
            &serde_json::json!({"apparatus":apparatus}).to_string())).await.unwrap();
        assert_eq!(created.status(), StatusCode::BAD_REQUEST);
    }
    state.production_maps.confirm_paddon_print(&codes[0], &crate::core::production_map::QueueActionActor {
        role: "aparatchi".into(), ref_: "multi-cut-worker".into(), display_name: "Worker".into(),
    }).await.unwrap();
    let rejected = router.clone().oneshot(request_with_body("POST", &format!("{BASE}/active/next"), &token,
        &serde_json::json!({"code":codes[0],"apparatus":SECOND}).to_string())).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    let successor = router.clone().oneshot(request_with_body("POST", &format!("{BASE}/active/next"), &token,
        &serde_json::json!({"code":codes[0],"apparatus":APPARATUS}).to_string())).await.unwrap();
    assert_eq!(successor.status(), StatusCode::OK);
    let successor = json_body(successor).await;
    assert_eq!(successor["paddon"]["apparatus_id"], APPARATUS);
    let legacy = state.production_maps.create_paddon("", "unresolved", &crate::core::production_map::QueueActionActor {
        role: "aparatchi".into(), ref_: "multi-cut-worker".into(), display_name: "Worker".into(),
    }).await.unwrap();
    let unresolved = router.clone().oneshot(request("GET", &format!("{BASE}/detail?code={}", legacy.code), &token)).await.unwrap();
    assert_eq!(unresolved.status(), StatusCode::BAD_REQUEST);
    // Losing the apparatus assignment also revokes known-code mutations and transfers.
    state.admin.upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
        principal_role: PrincipalRole::Aparatchi,
        principal_ref: "multi-cut-worker".into(), role_id: "aparatchi".into(),
        assigned_apparatus: vec![SECOND.into()], assigned_item_groups: vec![],
    }).await.unwrap();
    let denied_transfer = router.oneshot(request_with_body("POST", &format!("{BASE}/items/transfer"), &token,
        &serde_json::json!({"source_code":codes[0],"target_code":successor["paddon"]["code"],"progress_batch_ids":["known-roll"]}).to_string())).await.unwrap();
    assert_eq!(denied_transfer.status(), StatusCode::BAD_REQUEST);
}
