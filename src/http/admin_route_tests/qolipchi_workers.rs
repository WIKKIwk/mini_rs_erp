use super::*;

#[tokio::test]
async fn qolipchi_worker_gets_qolipchi_code_and_login_role() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let worker = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"id":"qolipchi_worker_1","name":"Qolipchi worker","phone":"+998901112266","level":"Master"}"#,
        ))
        .await
        .expect("create qolipchi worker");
    assert_eq!(worker.status(), StatusCode::OK);

    let assignment = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &token,
            r#"{
                "principal_role":"qolipchi",
                "principal_ref":"qolipchi_worker_1",
                "role_id":"qolipchi"
            }"#,
        ))
        .await
        .expect("assign qolipchi role");
    assert_eq!(assignment.status(), StatusCode::OK);

    let regenerated = build_router(state.clone())
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/workers/code/regenerate?id=qolipchi_worker_1",
            &token,
        ))
        .await
        .expect("qolipchi worker code regenerate");
    assert_eq!(regenerated.status(), StatusCode::OK);
    let code = json_body(regenerated).await["code"]
        .as_str()
        .expect("generated qolipchi code")
        .to_string();
    assert!(code.starts_with("50"), "{code}");

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
    let login_body = json_body(login).await;
    assert_eq!(login_body["profile"]["role"], "qolipchi");
    assert_eq!(login_body["profile"]["ref"], "qolipchi_worker_1");
}

#[tokio::test]
async fn qolipchi_login_rejects_worker_without_role_assignment() {
    let mut state = test_state();
    let role_store = Arc::new(MemoryRoleDefinitionStore::new());
    state.admin = state.admin.with_role_store(role_store.clone());
    let token = session(&state, PrincipalRole::Admin).await;

    let worker = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"id":"qolipchi_worker_revoke","name":"Revoked qolipchi","phone":"+998901112299","level":"Master"}"#,
        ))
        .await
        .expect("create qolipchi worker");
    assert_eq!(worker.status(), StatusCode::OK);

    let assignment = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &token,
            r#"{
                "principal_role":"qolipchi",
                "principal_ref":"qolipchi_worker_revoke",
                "role_id":"qolipchi"
            }"#,
        ))
        .await
        .expect("assign qolipchi role");
    assert_eq!(assignment.status(), StatusCode::OK);

    let regenerated = build_router(state.clone())
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/workers/code/regenerate?id=qolipchi_worker_revoke",
            &token,
        ))
        .await
        .expect("qolipchi worker code regenerate");
    assert_eq!(regenerated.status(), StatusCode::OK);
    let code = json_body(regenerated).await["code"]
        .as_str()
        .expect("generated qolipchi code")
        .to_string();
    assert!(code.starts_with("50"), "{code}");

    role_store
        .delete_role_assignment(&PrincipalRole::Qolipchi, "qolipchi_worker_revoke")
        .await
        .expect("delete qolipchi role assignment");

    let login = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            &format!(r#"{{"phone":"+998901112299","code":"{code}"}}"#),
        ))
        .await
        .expect("qolipchi login");
    assert_eq!(login.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_workers_filters_worker_and_qolipchi_roles_on_backend() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let worker = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"id":"plain_worker","name":"Plain worker","phone":"+998901110001","level":"Master"}"#,
        ))
        .await
        .expect("create worker");
    assert_eq!(worker.status(), StatusCode::OK);

    let qolipchi = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"id":"qolipchi_worker","name":"Qolipchi worker","phone":"+998901110002","level":"Master"}"#,
        ))
        .await
        .expect("create qolipchi worker");
    assert_eq!(qolipchi.status(), StatusCode::OK);

    let assignment = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &token,
            r#"{
                "principal_role":"qolipchi",
                "principal_ref":"qolipchi_worker",
                "role_id":"qolipchi"
            }"#,
        ))
        .await
        .expect("assign qolipchi role");
    assert_eq!(assignment.status(), StatusCode::OK);

    let worker_response = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/workers?role=worker",
            &token,
        ))
        .await
        .expect("worker list");
    assert_eq!(worker_response.status(), StatusCode::OK);
    let workers = json_body(worker_response).await;
    let workers = workers.as_array().expect("workers");
    assert!(workers.iter().any(|item| item["id"] == "plain_worker"));
    assert!(workers.iter().all(|item| item["id"] != "qolipchi_worker"));

    let qolipchi_response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/workers?role=qolipchi",
            &token,
        ))
        .await
        .expect("qolipchi list");
    assert_eq!(qolipchi_response.status(), StatusCode::OK);
    let qolipchilar = json_body(qolipchi_response).await;
    let qolipchilar = qolipchilar.as_array().expect("qolipchilar");
    assert_eq!(qolipchilar.len(), 1);
    assert_eq!(qolipchilar[0]["id"], "qolipchi_worker");
}
