use super::*;

const CUT: &str = "apparatus:default:asset-010";
const CUT_TWO: &str = "apparatus:default:asset-011";
const PRINT: &str = "apparatus:default:bosma_7";
const MAPS: &str = "/v1/mobile/admin/production-maps";
const WORKERS: &str = "/v1/mobile/rezka-master/workers";
const ADMIN_ASSIGNMENTS: &str = "/v1/mobile/admin/role-assignments";
const SETTINGS: &str = "/v1/mobile/rezka-master/order-settings";

#[tokio::test]
async fn rezka_master_login_grants_only_cut_apparatus_and_scoped_routes() {
    let mut state = test_state();
    state.worker_show_all_apparatus_tabs = true;
    let admin = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);
    let created = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/system-users",
            &admin,
            r#"{"role":"rezka_master","name":"Rezka Master","phone":"+998907771145"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let created = json_body(created).await;
    let id = created["id"].as_str().unwrap();
    assert!(id.starts_with("rezka_master_"));
    let code = router
        .clone()
        .oneshot(request(
            "POST",
            &format!("/v1/mobile/admin/system-users/code/regenerate?id={id}"),
            &admin,
        ))
        .await
        .unwrap();
    assert_eq!(code.status(), StatusCode::OK);
    let code = json_body(code).await;
    assert!(code["code"].as_str().unwrap().starts_with("94"));
    let login = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            &serde_json::json!({"phone":"+998907771145","code":code["code"]}).to_string(),
        ))
        .await
        .unwrap();
    let status = login.status();
    let body = json_body(login).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["profile"]["role"], "rezka_master");
    assert_eq!(body["assigned_apparatus"], serde_json::json!([CUT]));
    assert!(
        !body["capabilities"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("admin.access"))
    );
    let token = body["token"].as_str().unwrap();
    for path in [
        "/v1/mobile/admin/settings",
        "/v1/mobile/admin/role-assignments",
        &format!("{MAPS}/sequence"),
    ] {
        let denied = router
            .clone()
            .oneshot(request("GET", path, token))
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN, "{path}");
    }
    let seq = router
        .oneshot(request(
            "GET",
            &format!("{MAPS}/sequence?worker_scope=true"),
            token,
        ))
        .await
        .unwrap();
    assert_eq!(seq.status(), StatusCode::OK);
    assert_eq!(
        json_body(seq).await["worker_show_all_apparatus_tabs"],
        false
    );
}

#[tokio::test]
async fn rezka_master_changes_only_cut_worker_access_and_detects_stale_choices() {
    let state = test_state();
    state.apparatus.bootstrap_factory_defaults().await.unwrap();
    let master = session_for(&state, PrincipalRole::RezkaMaster, "rezka-master-test").await;
    let admin = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state.clone());
    let id = "worker-mixed";
    for worker_id in [id, "worker-print", "worker-group", "worker-unassigned"] {
        let worker = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/admin/workers",
                &admin,
                &serde_json::json!({"id": worker_id, "name": worker_id, "level": "Master"})
                    .to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(
            worker.status(),
            StatusCode::OK,
            "{}",
            json_body(worker).await
        );
    }
    for (worker_id, apparatus_ids) in [(id, vec![CUT, PRINT]), ("worker-print", vec![PRINT])] {
        let assignment = router
            .clone()
            .oneshot(request_with_body(
                "PUT",
                ADMIN_ASSIGNMENTS,
                &admin,
                &serde_json::json!({
                    "principal_role": "aparatchi",
                    "principal_ref": worker_id,
                    "role_id": "aparatchi",
                    "assigned_apparatus": apparatus_ids,
                    "assigned_item_groups": ["FILM"],
                })
                .to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(
            assignment.status(),
            StatusCode::OK,
            "{}",
            json_body(assignment).await
        );
    }
    let group = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/worker-groups",
            &admin,
            &serde_json::json!({
                "apparatus_id": CUT, "group_code": "A", "shift": "kunduz",
                "start_time": "08:00", "end_time": "17:00", "start_day": "monday",
                "worker_ids": ["worker-group"],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(group.status(), StatusCode::OK, "{}", json_body(group).await);
    let list = router
        .clone()
        .oneshot(request("GET", WORKERS, &master))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let list = json_body(list).await;
    assert_eq!(list["workers"].as_array().unwrap().len(), 1);
    assert_eq!(list["workers"][0]["id"], id);
    assert_eq!(list["assignments"].as_array().unwrap().len(), 1);
    assert_eq!(
        list["assignments"][0]["assigned_apparatus"],
        serde_json::json!([CUT])
    );
    for (worker_id, next, previous, status, error) in [
        (
            "worker-print",
            vec![CUT],
            vec![],
            StatusCode::NOT_FOUND,
            "worker_not_found",
        ),
        (
            "worker-group",
            vec![CUT],
            vec![],
            StatusCode::NOT_FOUND,
            "worker_not_found",
        ),
        (
            "worker-unassigned",
            vec![CUT],
            vec![],
            StatusCode::NOT_FOUND,
            "worker_not_found",
        ),
        (
            id,
            vec![PRINT],
            vec![CUT],
            StatusCode::BAD_REQUEST,
            "rezka_apparatus_required",
        ),
        (
            id,
            vec![],
            vec![CUT],
            StatusCode::BAD_REQUEST,
            "rezka_apparatus_required",
        ),
        (
            id,
            vec![CUT_TWO],
            vec![],
            StatusCode::CONFLICT,
            "rezka_assignment_changed",
        ),
        (id, vec![CUT_TWO], vec![CUT], StatusCode::OK, ""),
        (id, vec![CUT, CUT_TWO], vec![CUT_TWO], StatusCode::OK, ""),
        (id, vec![CUT_TWO], vec![CUT, CUT_TWO], StatusCode::OK, ""),
        (
            id,
            vec![],
            vec![CUT_TWO],
            StatusCode::BAD_REQUEST,
            "rezka_apparatus_required",
        ),
    ] {
        let result = router.clone().oneshot(request_with_body(
            "PUT", WORKERS, &master,
            &serde_json::json!({"worker_id":worker_id,"apparatus_ids":next,"previous_apparatus_ids":previous}).to_string(),
        )).await.unwrap();
        let actual_status = result.status();
        let body = json_body(result).await;
        assert_eq!(actual_status, status, "{body}");
        if !error.is_empty() {
            assert_eq!(body["error"], error);
        }
    }
    let assignment = state
        .admin
        .role_assignments()
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.principal_ref == id)
        .unwrap();
    assert_eq!(assignment.assigned_apparatus, vec![CUT_TWO, PRINT]);
    assert_eq!(assignment.assigned_item_groups, vec!["FILM"]);
    let list = router
        .clone()
        .oneshot(request("GET", WORKERS, &master))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let list = json_body(list).await;
    assert_eq!(list["workers"][0]["id"], id);
    assert_eq!(
        list["assignments"][0]["assigned_apparatus"],
        serde_json::json!([CUT_TWO])
    );

    let cleared = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            ADMIN_ASSIGNMENTS,
            &admin,
            &serde_json::json!({
                "principal_role": "aparatchi", "principal_ref": id,
                "role_id": "aparatchi", "assigned_apparatus": [PRINT],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(cleared.status(), StatusCode::OK);
    let list = router
        .oneshot(request("GET", WORKERS, &master))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    assert_eq!(json_body(list).await["workers"], serde_json::json!([]));
}

#[tokio::test]
async fn rezka_master_updates_order_flags_and_paddon_sharing_within_cut_scope() {
    let state = test_state();
    let token = session_for(&state, PrincipalRole::RezkaMaster, "rezka-master-test").await;
    for (id, number, apparatus) in [("zakaz-cut", "9915", CUT), ("zakaz-print", "9916", PRINT)] {
        let map = serde_json::from_str(&pechat_order_map_json(id, id, number, apparatus)).unwrap();
        state.production_maps.upsert_map(map).await.unwrap();
    }
    let router = build_router(state.clone());
    for (id, expected) in [
        ("zakaz-print", StatusCode::NOT_FOUND),
        ("zakaz-cut", StatusCode::OK),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "PUT",
                SETTINGS,
                &token,
                &serde_json::json!({
                    "order_id":id,"duplicate_rezka_labels":true,"include_bobina_in_net":true,
                    "previous_duplicate_rezka_labels":false,"previous_include_bobina_in_net":false,
                })
                .to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{}", json_body(response).await);
    }
    let map = state
        .production_maps
        .raw_map("zakaz-cut")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(map.duplicate_rezka_labels, Some(true));
    assert_eq!(map.include_bobina_in_net, Some(true));
    for (apparatus, expected) in [(PRINT, StatusCode::FORBIDDEN), (CUT, StatusCode::OK)] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                &format!("{MAPS}/queue-action"),
                &token,
                &serde_json::json!({"order_id":"zakaz-cut","apparatus":apparatus,"action":"start"})
                    .to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{}", json_body(response).await);
    }
    let active = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            SETTINGS,
            &token,
            r#"{
          "order_id":"zakaz-cut","duplicate_rezka_labels":true,"include_bobina_in_net":false,
          "previous_duplicate_rezka_labels":true,"previous_include_bobina_in_net":true
        }"#,
        ))
        .await
        .unwrap();
    assert_eq!(active.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(active).await["error"],
        "rezka_weight_mode_run_active"
    );
    let response = router
        .oneshot(request_with_body(
            "PUT",
            &format!("{MAPS}/paddons/management-settings"),
            &token,
            r#"{"worker_visibility_enabled":true}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let settings = json_body(response).await;
    assert_eq!(settings["settings"]["worker_visibility_enabled"], true);
    assert_eq!(settings["settings"]["free_movement_enabled"], false);
}
