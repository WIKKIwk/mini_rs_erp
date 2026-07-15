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
                    {"usage":"rasxot","category":"colors","name":"Oq","values":{"Mix":10,"Oq":2,"Spirt":1}},
                    {"usage":"rasxot","category":"colors","name":"Qora","values":{"Mix":2.5,"Qora":0.5}},
                    {"usage":"astatka","category":"colors","name":"Oq","values":{"Mix":4,"Oq":1}},
                    {"usage":"astatka","category":"colors","name":"Qora","values":{"Mix":1,"Qora":0.25}},
                    {"usage":"rasxot","category":"lacquers","name":"Laklar","values":{"OPV lak":100}},
                    {"usage":"rasxot","category":"solvents","name":"Spirtlar","values":{"Etil":10,"Metoxil":2,"Rasvavitel":0.5}},
                    {"usage":"astatka","category":"solvents","name":"Spirtlar","values":{"Etil":1,"Aralashmalar":0.25}}
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
    assert_eq!(body["items"][0]["items"][2]["usage"], "astatka");
    assert_eq!(
        body["items"][0]["calculation"]["rasxot_mix_total"],
        "12.5"
    );
    assert_eq!(
        body["items"][0]["calculation"]["astatka_mix_total"],
        "5"
    );
    assert_eq!(
        body["items"][0]["calculation"]["rasxot_alcohol"],
        "16.25"
    );
    assert_eq!(
        body["items"][0]["calculation"]["astatka_alcohol"],
        "2.75"
    );
    assert_eq!(
        body["items"][0]["calculation"]["final_used_alcohol"],
        "13.5"
    );
    assert_eq!(
        body["items"][0]["calculation"]["rasxot_pure_paint"],
        "12.25"
    );
    assert_eq!(
        body["items"][0]["calculation"]["astatka_pure_paint"],
        "4.75"
    );
    assert_eq!(
        body["items"][0]["calculation"]["final_used_paint"],
        "7.5"
    );
}

#[tokio::test]
async fn image_only_report_waits_for_boyoqchi_and_completes_same_record_once() {
    let state = test_state();
    let aparatchi_token = session_for(&state, PrincipalRole::Aparatchi, "worker-image").await;
    let boyoqchi_token = session_for(&state, PrincipalRole::Boyoqchi, "boyoqchi-image").await;
    let router = build_router(state);
    let image_body = vec![0xA5; 2 * 1024 * 1024 + 1];

    let upload = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/returned-paint/images?order_id=order-image&apparatus=7%20ta%20rangli%20bosma")
                .header(header::AUTHORIZATION, format!("Bearer {aparatchi_token}"))
                .header(header::CONTENT_TYPE, "image/jpeg")
                .header("x-file-name", "qoldiq.jpg")
                .body(Body::from(image_body.clone()))
                .expect("upload request"),
        )
        .await
        .expect("upload response");
    let upload_status = upload.status();
    let upload_body = json_body(upload).await;
    assert_eq!(upload_status, StatusCode::OK, "{upload_body}");
    let image_id = upload_body["image"]["image_id"]
        .as_str()
        .expect("image id")
        .to_string();

    let mismatched_order = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/returned-paint/requests",
            &aparatchi_token,
            &format!(
                r#"{{
                    "order_id":"another-order",
                    "order_code":"8964",
                    "order_name":"Boshqa order",
                    "apparatus":"7 ta rangli bosma",
                    "image_id":"{image_id}",
                    "items":[]
                }}"#
            ),
        ))
        .await
        .expect("mismatched image request");
    assert_eq!(mismatched_order.status(), StatusCode::BAD_REQUEST);

    let partially_filled = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/returned-paint/requests",
            &aparatchi_token,
            &format!(
                r#"{{
                    "order_id":"order-image",
                    "order_code":"8963",
                    "order_name":"Rasmli order",
                    "apparatus":"7 ta rangli bosma",
                    "image_id":"{image_id}",
                    "items":[
                        {{"usage":"rasxot","category":"colors","name":"Oq","values":{{"Mix":10,"Oq":2}}}}
                    ]
                }}"#
            ),
        ))
        .await
        .expect("partially filled image request");
    assert_eq!(partially_filled.status(), StatusCode::BAD_REQUEST);

    let pending = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/returned-paint/requests",
            &aparatchi_token,
            &format!(
                r#"{{
                    "order_id":"order-image",
                    "order_code":"8963",
                    "order_name":"Rasmli order",
                    "apparatus":"7 ta rangli bosma",
                    "image_id":"{image_id}",
                    "items":[]
                }}"#
            ),
        ))
        .await
        .expect("pending request");
    let pending_status = pending.status();
    let pending_body = json_body(pending).await;
    assert_eq!(pending_status, StatusCode::OK, "{pending_body}");
    assert_eq!(pending_body["status"], "waiting_for_boyoqchi_input");
    assert!(pending_body.get("calculation").is_none());
    assert_eq!(pending_body["image"]["image_id"], image_id);
    let request_id = pending_body["id"].as_str().expect("request id").to_string();

    let insufficient = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/returned-paint/requests/complete",
            &boyoqchi_token,
            &format!(
                r#"{{
                    "request_id":"{request_id}",
                    "items":[
                        {{"usage":"rasxot","category":"colors","name":"Oq","values":{{"Mix":10,"Oq":2}}}}
                    ]
                }}"#
            ),
        ))
        .await
        .expect("insufficient completion");
    assert_eq!(insufficient.status(), StatusCode::BAD_REQUEST);

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/returned-paint/requests/complete",
            &boyoqchi_token,
            &format!(
                r#"{{
                    "request_id":"{request_id}",
                    "items":[
                        {{"usage":"rasxot","category":"colors","name":"Oq","values":{{"Mix":10,"Oq":2,"Qora":0}}}},
                        {{"usage":"astatka","category":"colors","name":"Oq","values":{{"Mix":1,"Oq":0,"Qora":0}}}}
                    ]
                }}"#
            ),
        ))
        .await
        .expect("complete request");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body}");
    assert_eq!(completed_body["id"], request_id);
    assert_eq!(completed_body["status"], "completed");
    assert_eq!(completed_body["calculation"]["rasxot_mix_total"], "10");
    assert_eq!(completed_body["calculation"]["astatka_mix_total"], "1");

    let retry = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/returned-paint/requests/complete",
            &boyoqchi_token,
            &format!(
                r#"{{
                    "request_id":"{request_id}",
                    "items":[
                        {{"usage":"rasxot","category":"colors","name":"Qora","values":{{"Mix":99,"Qora":99,"Sariq":0}}}},
                        {{"usage":"astatka","category":"colors","name":"Qora","values":{{"Mix":1,"Qora":0,"Sariq":0}}}}
                    ]
                }}"#
            ),
        ))
        .await
        .expect("retry completion");
    let retry_body = json_body(retry).await;
    assert_eq!(retry_body["id"], request_id);
    assert_eq!(retry_body["calculation"]["rasxot_mix_total"], "10");

    let view = router
        .clone()
        .oneshot(request(
            "GET",
            &format!("/v1/mobile/returned-paint/images/view?id={image_id}"),
            &boyoqchi_token,
        ))
        .await
        .expect("view image");
    assert_eq!(view.status(), StatusCode::OK);
    assert_eq!(
        &to_bytes(view.into_body(), usize::MAX)
            .await
            .expect("image body")[..],
        &image_body
    );

    let delete_attached = router
        .oneshot(request(
            "DELETE",
            &format!("/v1/mobile/returned-paint/images?id={image_id}"),
            &aparatchi_token,
        ))
        .await
        .expect("delete attached image");
    assert_eq!(delete_attached.status(), StatusCode::BAD_REQUEST);
}
