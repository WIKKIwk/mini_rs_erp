use super::*;
use crate::core::production_map::queue_state::{ApparatusQueueAction, ApparatusQueueOrderState};
use crate::core::production_map::{
    ApparatusQueueActionEvent, ApparatusQueuePolicy, OrderRunSession, OrderRunStatus,
    ProductionMapStorePort, QueueActionActor,
};
use crate::core::qolip::{
    MemoryQolipStore, QolipCheckout, QolipLocation, QolipService, QolipStorePort,
};

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
async fn admin_worker_delete_requires_connection_confirmation_and_cleans_assignments() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"id":"worker_delete_1","name":"Delete worker","level":"Master"}"#,
        ))
        .await
        .expect("create worker");
    assert_eq!(created.status(), StatusCode::OK);

    let group = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/worker-groups",
            &token,
            r#"{
                "apparatus":"Laminatsiya 1",
                "group_code":"A",
                "shift":"kunduz",
                "worker_ids":["worker_delete_1"]
            }"#,
        ))
        .await
        .expect("save worker group");
    assert_eq!(group.status(), StatusCode::OK);

    let check = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/workers/delete-check?id=worker_delete_1",
            &token,
        ))
        .await
        .expect("delete check");
    assert_eq!(check.status(), StatusCode::OK);
    let check_body = json_body(check).await;
    assert_eq!(check_body["blocked"], false);
    assert_eq!(check_body["requires_confirmation"], true);
    assert!(check_body["connections"].as_array().is_some_and(|items| {
        items.iter().any(|item| item["kind"] == "worker_group")
            && items.iter().any(|item| item["kind"] == "apparatus")
    }));

    let rejected = build_router(state.clone())
        .oneshot(request(
            "DELETE",
            "/v1/mobile/admin/workers?id=worker_delete_1",
            &token,
        ))
        .await
        .expect("unconfirmed delete");
    assert_eq!(rejected.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(rejected).await["requires_confirmation"], true);

    let deleted = build_router(state.clone())
        .oneshot(request(
            "DELETE",
            "/v1/mobile/admin/workers?id=worker_delete_1&confirm_connections=true",
            &token,
        ))
        .await
        .expect("confirmed delete");
    assert_eq!(deleted.status(), StatusCode::OK);
    assert_eq!(json_body(deleted).await["ok"], true);

    let groups = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/worker-groups?apparatus=Laminatsiya%201",
            &token,
        ))
        .await
        .expect("worker groups after delete");
    let groups_body = json_body(groups).await;
    assert_eq!(groups_body[0]["worker_ids"], serde_json::json!([]));

    let assignments = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/role-assignments", &token))
        .await
        .expect("role assignments after delete");
    assert!(
        !json_body(assignments)
            .await
            .as_array()
            .expect("assignments")
            .iter()
            .any(|assignment| assignment["principal_ref"] == "worker_delete_1")
    );

    let workers = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/workers", &token))
        .await
        .expect("workers after delete");
    assert!(
        json_body(workers)
            .await
            .as_array()
            .expect("workers")
            .is_empty()
    );
}

#[tokio::test]
async fn admin_worker_delete_is_blocked_by_active_work_even_when_confirmed() {
    let mut state = test_state();
    let production_store = Arc::new(MemoryProductionMapStore::new());
    state.production_maps = ProductionMapService::new(production_store.clone());
    let token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"id":"worker_active_1","name":"Active worker","level":"Master"}"#,
        ))
        .await
        .expect("create worker");
    assert_eq!(created.status(), StatusCode::OK);

    production_store
        .put_order_run_session(OrderRunSession {
            session_id: "session-worker-active-1".to_string(),
            apparatus: "Pechat 1".to_string(),
            order_id: "zakaz-worker-active-1".to_string(),
            status: OrderRunStatus::Active,
            worker_role: "aparatchi".to_string(),
            worker_ref: "worker_active_1".to_string(),
            worker_display_name: "Active worker".to_string(),
            started_at_unix: 1,
            updated_at_unix: 1,
            payload_json: serde_json::json!({}),
        })
        .await
        .expect("active worker session");

    let rejected = build_router(state.clone())
        .oneshot(request(
            "DELETE",
            "/v1/mobile/admin/workers?id=worker_active_1&confirm_connections=true",
            &token,
        ))
        .await
        .expect("delete active worker");
    assert_eq!(rejected.status(), StatusCode::CONFLICT);
    let rejected_body = json_body(rejected).await;
    assert_eq!(rejected_body["blocked"], true);
    assert_eq!(
        rejected_body["active_work"][0]["order_id"],
        "zakaz-worker-active-1"
    );
    assert_eq!(rejected_body["active_work"][0]["apparatus"], "Pechat 1");

    let workers = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/workers", &token))
        .await
        .expect("workers after blocked delete");
    assert_eq!(
        json_body(workers).await.as_array().expect("workers").len(),
        1
    );
}

#[tokio::test]
async fn admin_worker_delete_is_blocked_by_open_qolip_checkout() {
    let mut state = test_state();
    let qolip_store = Arc::new(MemoryQolipStore::new());
    state.qolip = QolipService::new(qolip_store.clone());
    let token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"id":"worker_qolip_1","name":"Qolip worker","level":"Master"}"#,
        ))
        .await
        .expect("create worker");
    assert_eq!(created.status(), StatusCode::OK);

    qolip_store
        .put_location(QolipLocation {
            id: "location-worker-qolip-1".to_string(),
            block: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
            item_code: "ITEM-1".to_string(),
            item_name: "Item 1".to_string(),
            qolip_code: "Q-1".to_string(),
            size: 10,
            quantity: 2,
            row_letter: "A".to_string(),
            column_number: Some(1),
            location_label: "A1".to_string(),
            created_by_role: "admin".to_string(),
            created_by_ref: "admin".to_string(),
            created_by_name: "Admin".to_string(),
        })
        .await
        .expect("qolip location");
    qolip_store
        .issue_checkout(QolipCheckout {
            id: "checkout-worker-qolip-1".to_string(),
            location_id: "location-worker-qolip-1".to_string(),
            block: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
            item_code: "ITEM-1".to_string(),
            item_name: "Item 1".to_string(),
            item_group: String::new(),
            qolip_code: "Q-1".to_string(),
            size: 10,
            quantity: 1,
            row_letter: "A".to_string(),
            column_number: Some(1),
            location_label: "A1".to_string(),
            issued_to_ref: "worker_qolip_1".to_string(),
            issued_to_name: "Qolip worker".to_string(),
            status: "open".to_string(),
            issued_by_role: "admin".to_string(),
            issued_by_ref: "admin".to_string(),
            issued_by_name: "Admin".to_string(),
            issued_at: String::new(),
        })
        .await
        .expect("open qolip checkout");

    let rejected = build_router(state.clone())
        .oneshot(request(
            "DELETE",
            "/v1/mobile/admin/workers?id=worker_qolip_1&confirm_connections=true",
            &token,
        ))
        .await
        .expect("delete worker with open qolip checkout");
    assert_eq!(rejected.status(), StatusCode::CONFLICT);
    let rejected_body = json_body(rejected).await;
    assert_eq!(rejected_body["blocked"], true);
    assert_eq!(rejected_body["active_work"][0]["kind"], "qolip_checkout");

    let workers = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/workers", &token))
        .await
        .expect("workers after blocked qolip delete");
    assert_eq!(
        json_body(workers).await.as_array().expect("workers").len(),
        1
    );
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
            &with_test_qolip(
                r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-worker-profile",
                "action":"start"
            }"#,
                "zakaz-worker-profile",
            ),
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

#[tokio::test]
async fn replacement_worker_with_same_name_does_not_inherit_old_history() {
    let mut state = test_state();
    let production_store = Arc::new(MemoryProductionMapStore::new());
    state.production_maps = ProductionMapService::new(production_store.clone());
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let old_worker = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &admin_token,
            r#"{"id":"worker_history_old","name":"Bir xil ism","phone":"+998901119999","level":"Master"}"#,
        ))
        .await
        .expect("create old worker");
    assert_eq!(old_worker.status(), StatusCode::OK);

    production_store
        .append_apparatus_queue_action_event(ApparatusQueueActionEvent {
            event_id: "event-worker-history-old".to_string(),
            apparatus: "Pechat 1".to_string(),
            order_id: "zakaz-worker-history-old".to_string(),
            action: ApparatusQueueAction::Start,
            from_state: ApparatusQueueOrderState::Pending,
            to_state: ApparatusQueueOrderState::InProgress,
            policy: ApparatusQueuePolicy::FreePick,
            actor: QueueActionActor {
                role: "aparatchi".to_string(),
                ref_: "worker_history_old".to_string(),
                display_name: "Bir xil ism".to_string(),
            },
            assigned_apparatus: Vec::new(),
            payload_json: serde_json::json!({}),
        })
        .await
        .expect("old worker history");

    let deactivated = build_router(state.clone())
        .oneshot(request(
            "DELETE",
            "/v1/mobile/admin/workers?id=worker_history_old",
            &admin_token,
        ))
        .await
        .expect("deactivate old worker");
    assert_eq!(deactivated.status(), StatusCode::OK);

    let replacement = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &admin_token,
            r#"{"id":"worker_history_new","name":"Bir xil ism","phone":"+998901119999","level":"Master"}"#,
        ))
        .await
        .expect("create replacement worker");
    assert_eq!(replacement.status(), StatusCode::OK);

    let detail = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/workers/profile-detail?id=worker_history_new",
            &admin_token,
        ))
        .await
        .expect("replacement profile detail");
    assert_eq!(detail.status(), StatusCode::OK);
    let detail_body = json_body(detail).await;
    assert_eq!(detail_body["worker"]["id"], "worker_history_new");
    assert_eq!(detail_body["recent_logs"], serde_json::json!([]));

    let old_history = production_store
        .queue_action_logs_for_worker(&["worker_history_old".to_string()], "", 10)
        .await
        .expect("preserved old history");
    assert_eq!(old_history.len(), 1);
    assert_eq!(old_history[0].actor_ref, "worker_history_old");
}
