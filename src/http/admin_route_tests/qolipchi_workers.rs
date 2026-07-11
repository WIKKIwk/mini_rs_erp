use super::*;

#[tokio::test]
async fn qolipchi_is_a_system_user_with_qolipchi_login() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/system-users",
            &token,
            r#"{"id":"qolipchi_1","role":"qolipchi","name":"Qolipchi","phone":"+998901112266"}"#,
        ))
        .await
        .expect("create qolipchi system user");
    assert_eq!(created.status(), StatusCode::OK);

    let regenerated = build_router(state.clone())
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/system-users/code/regenerate?id=qolipchi_1",
            &token,
        ))
        .await
        .expect("regenerate qolipchi code");
    assert_eq!(regenerated.status(), StatusCode::OK);
    let code = json_body(regenerated).await["code"]
        .as_str()
        .expect("generated code")
        .to_string();
    assert!(code.starts_with("50"), "{code}");
    let direct = state.auth.login("+998901112266", &code).await;
    assert!(direct.is_ok(), "direct login failed: {direct:?}");

    let login = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            &format!(r#"{{"phone":"+998901112266","code":"{code}"}}"#),
        ))
        .await
        .expect("qolipchi login");
    assert_eq!(login.status(), StatusCode::OK);
    let body = json_body(login).await;
    assert_eq!(body["profile"]["role"], "qolipchi");
    assert_eq!(body["profile"]["ref"], "qolipchi_1");
}

#[tokio::test]
async fn worker_cannot_login_as_qolipchi_even_with_qolipchi_assignment() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    assert_eq!(
        build_router(state.clone())
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/admin/workers",
                &token,
                r#"{"id":"worker_1","name":"Worker","phone":"+998901112299","level":"Master"}"#,
            ))
            .await
            .expect("create worker")
            .status(),
        StatusCode::OK,
    );
    assert_eq!(
        build_router(state.clone())
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/role-assignments",
                &token,
                r#"{"principal_role":"qolipchi","principal_ref":"worker_1","role_id":"qolipchi"}"#,
            ))
            .await
            .expect("legacy assignment")
            .status(),
        StatusCode::OK,
    );
    let regenerated = build_router(state.clone())
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/workers/code/regenerate?id=worker_1",
            &token,
        ))
        .await
        .expect("worker code");
    let code = json_body(regenerated).await["code"]
        .as_str()
        .expect("worker code")
        .to_string();
    assert!(code.starts_with("40"), "{code}");

    let login = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            &format!(r#"{{"phone":"+998901112299","code":"50{}"}}"#, &code[2..]),
        ))
        .await
        .expect("qolipchi login rejection");
    assert_eq!(login.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn qolipchi_is_listed_as_system_user_and_never_as_worker() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/system-users",
            &token,
            r#"{"id":"qolipchi_list","role":"qolipchi","name":"Qolipchi list","phone":"+998901110002"}"#,
        ))
        .await
        .expect("create qolipchi");

    let worker_list = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/workers?role=qolipchi",
            &token,
        ))
        .await
        .expect("worker list");
    assert_eq!(json_body(worker_list).await.as_array().unwrap().len(), 0);

    let users = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?role=qolipchi&limit=20&offset=0",
            &token,
        ))
        .await
        .expect("qolipchi users list");
    assert_eq!(users.status(), StatusCode::OK);
    let body = json_body(users).await;
    assert_eq!(body["items"][0]["source"], "system_user");
    assert_eq!(body["items"][0]["entity_ref"], "qolipchi_list");
    assert_eq!(body["items"][0]["principal_role"], "qolipchi");
}
