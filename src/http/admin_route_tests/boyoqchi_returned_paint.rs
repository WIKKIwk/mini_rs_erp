use super::*;

#[tokio::test]
async fn boyoqchi_is_independent_system_role_with_80_login_code() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/system-users",
            &admin_token,
            r#"{"id":"boyoqchi_1","role":"boyoqchi","name":"Bo‘yoqchi","phone":"+998901112280"}"#,
        ))
        .await
        .expect("create boyoqchi");
    assert_eq!(created.status(), StatusCode::OK);

    let regenerated = build_router(state.clone())
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/system-users/code/regenerate?id=boyoqchi_1",
            &admin_token,
        ))
        .await
        .expect("regenerate boyoqchi code");
    assert_eq!(regenerated.status(), StatusCode::OK);
    let code = json_body(regenerated).await["code"]
        .as_str()
        .expect("generated code")
        .to_string();
    assert!(code.starts_with("80"), "{code}");

    let login = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            &format!(r#"{{"phone":"+998901112280","code":"{code}"}}"#),
        ))
        .await
        .expect("boyoqchi login");
    assert_eq!(login.status(), StatusCode::OK);
    let body = json_body(login).await;
    assert_eq!(body["profile"]["role"], "boyoqchi");
    assert_eq!(body["profile"]["ref"], "boyoqchi_1");
    assert_eq!(body["capabilities"][0], "boyoqchi.access");
    assert!(
        body["capabilities"]
            .as_array()
            .expect("capabilities")
            .iter()
            .any(|value| value == "returned_paint.request.read")
    );
}

#[tokio::test]
async fn aparatchi_sends_returned_paint_and_only_boyoqchi_can_read_it() {
    let state = test_state();
    let aparatchi_token = session_for(&state, PrincipalRole::Aparatchi, "worker-1").await;
    let boyoqchi_token = session_for(&state, PrincipalRole::Boyoqchi, "boyoqchi-1").await;
    let qolipchi_token = session_for(&state, PrincipalRole::Qolipchi, "qolipchi-1").await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/returned-paint/requests",
            &aparatchi_token,
            r#"{
                "order_id":"order-1",
                "order_code":"1212",
                "order_name":"Estello",
                "apparatus":"7 ta rangli bosma",
                "items":[
                    {"usage":"rasxot","category":"colors","name":"Oq","values":{"Mix":3}},
                    {"usage":"astatka","category":"solvents","name":"Spirtlar","values":{"Etil":1.5}}
                ]
            }"#,
        ))
        .await
        .expect("create returned paint request");
    assert_eq!(created.status(), StatusCode::OK);

    let forbidden = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/returned-paint/requests",
            &qolipchi_token,
        ))
        .await
        .expect("qolipchi read");
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    let inbox = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/returned-paint/requests",
            &boyoqchi_token,
        ))
        .await
        .expect("boyoqchi inbox");
    assert_eq!(inbox.status(), StatusCode::OK);
    let body = json_body(inbox).await;
    assert_eq!(body["items"][0]["order_id"], "order-1");
    assert_eq!(body["items"][0]["items"][0]["usage"], "rasxot");
    assert_eq!(body["items"][0]["items"][1]["usage"], "astatka");
}
