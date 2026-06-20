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
