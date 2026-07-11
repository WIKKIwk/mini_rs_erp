use super::*;

#[tokio::test]
async fn admin_workers_are_separate_from_users_and_persist_level() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"name":"Ali ishchi","level":"Brigader"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(created.status(), StatusCode::OK);
    let created_json = json_body(created).await;
    assert_eq!(created_json["name"], "Ali ishchi");
    assert_eq!(created_json["level"], "Brigader");
    let worker_id = created_json["id"].as_str().expect("id");

    let updated = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/workers",
            &token,
            &format!(r#"{{"id":"{worker_id}","name":"","level":"2 - darajali"}}"#),
        ))
        .await
        .expect("response");
    assert_eq!(updated.status(), StatusCode::OK);
    assert_eq!(json_body(updated).await["level"], "2 - darajali");

    let phone_updated = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/workers",
            &token,
            &format!(r#"{{"id":"{worker_id}","phone":"+998901112233"}}"#),
        ))
        .await
        .expect("phone response");
    assert_eq!(phone_updated.status(), StatusCode::OK);
    assert_eq!(json_body(phone_updated).await["phone"], "+998901112233");

    let listed = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/workers", &token))
        .await
        .expect("response");
    assert_eq!(listed.status(), StatusCode::OK);
    let workers = json_body(listed).await;
    assert_eq!(workers.as_array().expect("workers").len(), 1);
    assert_eq!(workers[0]["name"], "Ali ishchi");
    assert_eq!(workers[0]["phone"], "+998901112233");
}

#[tokio::test]
async fn admin_worker_groups_save_custom_codes_schedule_and_reject_duplicate_worker() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let first = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"name":"Jalol ishchi","level":"Brigader"}"#,
        ))
        .await
        .expect("create first worker");
    let first_id = json_body(first).await["id"]
        .as_str()
        .expect("first worker id")
        .to_string();

    let second = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"name":"Vali ishchi","level":"Master"}"#,
        ))
        .await
        .expect("create second worker");
    let second_id = json_body(second).await["id"]
        .as_str()
        .expect("second worker id")
        .to_string();

    let saved = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/worker-groups",
            &token,
            &format!(
                r#"{{
                    "apparatus":"Laminatsiya 1",
                    "group_code":"b guruh",
                    "shift":"kechki",
                    "start_time":"08:30",
                    "end_time":"20:30",
                    "work_days_per_week":6,
                    "start_day":"monday",
                    "accounting_enabled":true,
                    "worker_ids":["{first_id}","{second_id}"]
                }}"#
            ),
        ))
        .await
        .expect("save worker group");
    assert_eq!(saved.status(), StatusCode::OK);
    let saved_body = json_body(saved).await;
    assert_eq!(saved_body["apparatus"], "Laminatsiya 1");
    assert_eq!(saved_body["group_code"], "B GURUH");
    assert_eq!(saved_body["shift"], "kechki");
    assert_eq!(saved_body["start_time"], "08:30");
    assert_eq!(saved_body["end_time"], "20:30");
    assert_eq!(saved_body["work_days_per_week"], 6);
    assert_eq!(saved_body["start_day"], "monday");
    assert_eq!(saved_body["accounting_enabled"], true);
    assert_eq!(saved_body["workers"].as_array().expect("workers").len(), 2);

    let assignments = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/role-assignments", &token))
        .await
        .expect("role assignments");
    assert_eq!(assignments.status(), StatusCode::OK);
    let assignment_body = json_body(assignments).await;
    let assignments = assignment_body.as_array().expect("assignments");
    for worker_id in [&first_id, &second_id] {
        let assignment = assignments
            .iter()
            .find(|assignment| {
                assignment["principal_role"] == "aparatchi"
                    && assignment["principal_ref"] == worker_id.as_str()
            })
            .expect("worker aparatchi assignment");
        assert_eq!(assignment["role_id"], "aparatchi");
        assert_eq!(
            assignment["assigned_apparatus"],
            serde_json::json!(["Laminatsiya 1"])
        );
    }

    let saved = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/worker-groups",
            &token,
            &format!(
                r#"{{
                    "apparatus":"Laminatsiya 1",
                    "group_code":"b guruh",
                    "shift":"kechki",
                    "start_time":"08:30",
                    "end_time":"20:30",
                    "work_days_per_week":6,
                    "start_day":"monday",
                    "accounting_enabled":true,
                    "worker_ids":["{first_id}"]
                }}"#
            ),
        ))
        .await
        .expect("save worker group without second worker");
    assert_eq!(saved.status(), StatusCode::OK);

    let assignments = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/role-assignments", &token))
        .await
        .expect("role assignments after worker removal");
    assert_eq!(assignments.status(), StatusCode::OK);
    let assignment_body = json_body(assignments).await;
    let assignments = assignment_body.as_array().expect("assignments");
    assert!(assignments.iter().any(|assignment| {
        assignment["principal_role"] == "aparatchi"
            && assignment["principal_ref"] == first_id
            && assignment["assigned_apparatus"] == serde_json::json!(["Laminatsiya 1"])
    }));
    assert!(!assignments.iter().any(|assignment| {
        assignment["principal_role"] == "aparatchi" && assignment["principal_ref"] == second_id
    }));

    let duplicate = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/worker-groups",
            &token,
            &format!(
                r#"{{
                    "apparatus":"Laminatsiya 1",
                    "group_code":"ba",
                    "shift":"kunduz",
                    "worker_ids":["{first_id}"]
                }}"#
            ),
        ))
        .await
        .expect("duplicate worker group");
    assert_eq!(duplicate.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(duplicate).await["error"],
        "worker is duplicated in apparatus groups"
    );

    let second_group = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/worker-groups",
            &token,
            &format!(
                r#"{{
                    "apparatus":"Laminatsiya 1",
                    "group_code":"dd",
                    "shift":"tungi",
                    "worker_ids":["{first_id}"]
                }}"#
            ),
        ))
        .await
        .expect("save second worker group");
    assert_eq!(second_group.status(), StatusCode::BAD_REQUEST);

    let listed = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/worker-groups?apparatus=Laminatsiya%201",
            &token,
        ))
        .await
        .expect("list worker groups");
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = json_body(listed).await;
    let groups = listed_body.as_array().expect("groups");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["group_code"], "B GURUH");
    assert_eq!(groups[0]["shift"], "kechki");

    let invalid = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/worker-groups",
            &token,
            r#"{
                "apparatus":"Laminatsiya 1",
                "group_code":"zz",
                "shift":"night",
                "worker_ids":["missing-worker"]
            }"#,
        ))
        .await
        .expect("invalid worker group");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(invalid).await["error"], "worker not found");
}

#[tokio::test]
async fn worker_login_receives_group_assigned_apparatus() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let worker = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"id":"worker_001","name":"Ali worker","phone":"+998901112233","level":"Master"}"#,
        ))
        .await
        .expect("create worker");
    assert_eq!(worker.status(), StatusCode::OK);

    let saved = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/worker-groups",
            &token,
            r#"{
                "apparatus":"Laminatsiya 1",
                "group_code":"A",
                "shift":"kunduz",
                "worker_ids":["worker_001"]
            }"#,
        ))
        .await
        .expect("save worker group");
    assert_eq!(saved.status(), StatusCode::OK);

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            r#"{"phone":"+998901112233","code":"401234567890"}"#,
        ))
        .await
        .expect("worker login");
    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["profile"]["role"], "aparatchi");
    assert_eq!(value["profile"]["ref"], "worker_001");
    assert_eq!(
        value["assigned_apparatus"],
        serde_json::json!(["Laminatsiya 1"])
    );
    assert!(
        value["capabilities"]
            .as_array()
            .expect("capabilities")
            .iter()
            .any(|capability| capability == "apparatus.queue.manage")
    );
}

#[tokio::test]
async fn admin_worker_detail_regenerates_login_code_like_customer() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let worker = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"id":"worker_code_1","name":"Code worker","phone":"+998901112244","level":"Master"}"#,
        ))
        .await
        .expect("create worker");
    assert_eq!(worker.status(), StatusCode::OK);

    let detail = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/workers/detail?id=worker_code_1",
            &token,
        ))
        .await
        .expect("worker detail");
    assert_eq!(detail.status(), StatusCode::OK);
    let detail_body = json_body(detail).await;
    assert_eq!(detail_body["id"], "worker_code_1");
    assert_eq!(detail_body["name"], "Code worker");
    assert_eq!(detail_body["phone"], "+998901112244");
    assert_eq!(detail_body["code"], "");

    let regenerated = build_router(state.clone())
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/workers/code/regenerate?id=worker_code_1",
            &token,
        ))
        .await
        .expect("worker code regenerate");
    assert_eq!(regenerated.status(), StatusCode::OK);
    let regenerated_body = json_body(regenerated).await;
    let code = regenerated_body["code"]
        .as_str()
        .expect("generated worker code");
    assert!(code.starts_with("40"), "{code}");

    let login = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            &format!(r#"{{"phone":"+998901112244","code":"{code}"}}"#),
        ))
        .await
        .expect("worker login");
    assert_eq!(login.status(), StatusCode::OK);
    let login_body = json_body(login).await;
    assert_eq!(login_body["profile"]["role"], "aparatchi");
    assert_eq!(login_body["profile"]["ref"], "worker_code_1");
}

#[tokio::test]
async fn admin_worker_profile_detail_returns_assignments_and_activity() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &admin_token,
            r#"{"id":"worker_profile_1","name":"Profile worker","phone":"+998901112255","level":"Master"}"#,
        ))
        .await
        .expect("create worker");
    assert_eq!(created.status(), StatusCode::OK);

    let group = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/worker-groups",
            &admin_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "group_code":"A",
                "shift":"kunduz",
                "worker_ids":["worker_profile_1"]
            }"#,
        ))
        .await
        .expect("save group");
    assert_eq!(group.status(), StatusCode::OK);

    let map = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-worker-profile",
                "Profile order",
                "9911",
                "7 ta rangli pechat",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(map.status(), StatusCode::OK);

    let router = build_router(state.clone());
    provision_test_qolip(&router, &admin_token, "zakaz-worker-profile").await;

    let sequence = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_ids":["zakaz-worker-profile"]
            }"#,
        ))
        .await
        .expect("save sequence");
    assert_eq!(sequence.status(), StatusCode::OK);

    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker_profile_1").await;
    let started = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-worker-profile",
                "action":"start"
            }"#, "zakaz-worker-profile"),
        ))
        .await
        .expect("start queue");
    assert_eq!(started.status(), StatusCode::OK);

    let detail = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/workers/profile-detail?id=worker_profile_1",
            &admin_token,
        ))
        .await
        .expect("worker profile detail");
    assert_eq!(detail.status(), StatusCode::OK);
    let body = json_body(detail).await;
    assert_eq!(body["worker"]["id"], "worker_profile_1");
    assert_eq!(
        body["assigned_groups"][0]["apparatus"],
        "7 ta rangli pechat"
    );
    assert_eq!(
        body["active_sessions"][0]["order_id"],
        "zakaz-worker-profile"
    );
    assert_eq!(body["recent_logs"][0]["actor_ref"], "worker_profile_1");
}
