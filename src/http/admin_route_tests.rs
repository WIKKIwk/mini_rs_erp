use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tokio::sync::Mutex;
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup, AdminState};
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminStatePort, AdminWritePort};
use crate::core::admin::service::AdminService;
use crate::core::apparatus_groups::{ApparatusGroupService, MemoryApparatusGroupStore};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::authz::{
    MemoryRoleDefinitionStore, RoleAssignment, RoleDefinition, RoleDefinitionStorePort,
};
use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderImage, CalculateOrderStorePort, CalculateOrderTemplate,
};
use crate::core::mini_orders::{MiniOrderError, MiniOrderSink, NoopMiniOrderSink};
use crate::core::production_map::{MemoryProductionMapStore, ProductionMapService};
use crate::core::session::manager::SessionManager;
use crate::core::werka::models::{DispatchRecord, SupplierItem};
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
use crate::core::werka::service::WerkaService;
use crate::core::worker_groups::{MemoryWorkerGroupStore, WorkerGroupService};
use crate::core::workers::{MemoryWorkerStore, WorkerService};

#[tokio::test]
async fn admin_user_list_returns_merged_paged_users_with_role_labels() {
    let mut state = test_state();
    let role_store = Arc::new(MemoryRoleDefinitionStore::new());
    role_store
        .put_role_definition(RoleDefinition {
            id: "item_creator".to_string(),
            label: "Item yaratuvchi".to_string(),
            base_role: Some(PrincipalRole::Customer),
            capability_codes: vec!["catalog.item.create".to_string()],
            system: false,
        })
        .await
        .expect("role");
    role_store
        .put_role_assignment(RoleAssignment {
            principal_role: PrincipalRole::Customer,
            principal_ref: "CUST-001".to_string(),
            role_id: "item_creator".to_string(),
            assigned_apparatus: Vec::new(),
        })
        .await
        .expect("assignment");
    state.admin = state.admin.with_role_store(role_store);
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?q=customer&limit=2&offset=0",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["items"].as_array().expect("items").len(), 1);
    assert_eq!(value["items"][0]["id"], "customer:CUST-001");
    assert_eq!(value["items"][0]["source"], "customer");
    assert_eq!(value["items"][0]["entity_ref"], "CUST-001");
    assert_eq!(value["items"][0]["name"], "Customer One");
    assert_eq!(value["items"][0]["role_label"], "Item yaratuvchi");
    assert_eq!(value["has_more"], false);
}

#[tokio::test]
async fn admin_settings_requires_admin_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Supplier).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/settings", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn admin_method_checks_happen_after_auth_like_go() {
    let state = test_state();
    let cases = [
        ("PATCH", "/v1/mobile/admin/settings"),
        ("POST", "/v1/mobile/admin/roles"),
        ("PATCH", "/v1/mobile/admin/workers"),
        ("POST", "/v1/mobile/admin/production-maps"),
        ("POST", "/v1/mobile/admin/role-assignments"),
        ("PATCH", "/v1/mobile/admin/suppliers"),
        ("POST", "/v1/mobile/admin/users/list"),
        ("POST", "/v1/mobile/admin/suppliers/list"),
        ("POST", "/v1/mobile/admin/suppliers/summary"),
        ("POST", "/v1/mobile/admin/suppliers/detail"),
        ("POST", "/v1/mobile/admin/suppliers/inactive"),
        ("POST", "/v1/mobile/admin/suppliers/items/assigned"),
        ("POST", "/v1/mobile/admin/suppliers/status"),
        ("POST", "/v1/mobile/admin/suppliers/phone"),
        ("POST", "/v1/mobile/admin/suppliers/items"),
        ("GET", "/v1/mobile/admin/suppliers/items/add"),
        ("GET", "/v1/mobile/admin/suppliers/items/remove"),
        ("GET", "/v1/mobile/admin/suppliers/code/regenerate"),
        ("GET", "/v1/mobile/admin/suppliers/remove"),
        ("GET", "/v1/mobile/admin/suppliers/restore"),
        ("PATCH", "/v1/mobile/admin/customers"),
        ("POST", "/v1/mobile/admin/customers/list"),
        ("POST", "/v1/mobile/admin/customers/detail"),
        ("POST", "/v1/mobile/admin/customers/phone"),
        ("GET", "/v1/mobile/admin/customers/code/regenerate"),
        ("GET", "/v1/mobile/admin/customers/items/add"),
        ("GET", "/v1/mobile/admin/customers/items/remove"),
        ("GET", "/v1/mobile/admin/customers/remove"),
        ("PATCH", "/v1/mobile/admin/items"),
        ("GET", "/v1/mobile/admin/items/bulk-move-group"),
        ("PATCH", "/v1/mobile/admin/item-groups"),
        ("POST", "/v1/mobile/admin/item-groups/tree"),
        ("POST", "/v1/mobile/admin/activity"),
        ("GET", "/v1/mobile/admin/werka/code/regenerate"),
    ];

    let supplier_token = session(&state, PrincipalRole::Supplier).await;
    let admin_token = session(&state, PrincipalRole::Admin).await;
    for (method, path) in cases {
        let unauthorized = build_router(state.clone())
            .oneshot(request(method, path, ""))
            .await
            .expect("response");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(json_body(unauthorized).await["error"], "unauthorized");

        let forbidden = build_router(state.clone())
            .oneshot(request(method, path, &supplier_token))
            .await
            .expect("response");
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN, "{path}");
        assert_eq!(json_body(forbidden).await["error"], "forbidden");

        let method_not_allowed = build_router(state.clone())
            .oneshot(request(method, path, &admin_token))
            .await
            .expect("response");
        assert_eq!(
            method_not_allowed.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{path}"
        );
        assert_eq!(
            json_body(method_not_allowed).await["error"],
            "method not allowed"
        );
    }
}

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
async fn admin_production_maps_save_compiles_program() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"hotlunch-test",
                "product_code":"HOTLUNCH",
                "title":"Hotlunch test",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {
                        "id":"formula",
                        "kind":"formula",
                        "title":"CPP hisob",
                        "item_code":"CPP",
                        "formula":{"target":"cpp_kg","expression":"order_qty * 1.08"}
                    },
                    {
                        "id":"task",
                        "kind":"task",
                        "title":"Rezkaga yuborish",
                        "role_code":"rezkachi",
                        "qty_formula":"cpp_kg",
                        "from_location":"CPP ombor",
                        "to_location":"Rezka apparat"
                    },
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"formula"},
                    {"from":"formula","to":"task"},
                    {"from":"task","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["map"]["id"], "hotlunch-test");
    assert_eq!(value["program"]["operations"][1]["op_code"], "calculate");
    assert_eq!(
        value["program"]["operations"][1]["args"]["expression"],
        "order_qty * 1.08"
    );

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("response");
    assert_eq!(list.status(), StatusCode::OK);
    assert_eq!(json_body(list).await[0]["map"]["product_code"], "HOTLUNCH");
}

#[tokio::test]
async fn production_map_nodes_preserve_alternative_group_metadata() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"zakaz-alt",
                "product_code":"ALT-001",
                "title":"Alternative order",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {
                        "id":"apparatus",
                        "kind":"apparatus",
                        "title":"7 ta rangli pechat",
                        "alternative_group_id":"alt-pechat-1",
                        "alternative_group_label":"pechat",
                        "alternative_assigned_title":"7 ta rangli pechat"
                    },
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"apparatus"},
                    {"from":"apparatus","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(
        value["map"]["nodes"][1]["alternative_group_id"],
        "alt-pechat-1"
    );
    assert_eq!(
        value["map"]["nodes"][1]["alternative_group_label"],
        "pechat"
    );
    assert_eq!(
        value["map"]["nodes"][1]["alternative_assigned_title"],
        "7 ta rangli pechat"
    );

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("response");
    assert_eq!(list.status(), StatusCode::OK);
    let listed = json_body(list).await;
    assert_eq!(
        listed[0]["map"]["nodes"][1]["alternative_group_id"],
        "alt-pechat-1"
    );
    assert_eq!(
        listed[0]["map"]["nodes"][1]["alternative_group_label"],
        "pechat"
    );
    assert_eq!(
        listed[0]["map"]["nodes"][1]["alternative_assigned_title"],
        "7 ta rangli pechat"
    );
}

#[tokio::test]
async fn production_map_duplicate_order_number_returns_structured_error() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let first = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json("zakaz-1234", "Old zakaz", "1234", "8 ta rangli pechat"),
        ))
        .await
        .expect("first save");
    assert_eq!(first.status(), StatusCode::OK);

    let duplicate = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json("zakaz-new", "New zakaz", "1234", "8 ta rangli pechat"),
        ))
        .await
        .expect("duplicate save");
    assert_eq!(duplicate.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(duplicate).await["error"],
        "duplicate_order_number"
    );
}

#[tokio::test]
async fn production_map_rejects_laminatsiya_when_rubber_above_1050() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &laminatsiya_order_map_json("zakaz-lamin-1051", 1051.0),
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(response).await["error"],
        "laminatsiya_rubber_too_large"
    );
}

#[tokio::test]
async fn production_map_allows_laminatsiya_at_1050_rubber() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &laminatsiya_order_map_json("zakaz-lamin-1050", 1050.0),
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn production_map_move_validates_pechat_rules_on_server() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let saved = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json(
                "zakaz-9001",
                "Nine color rubber order",
                "9001",
                "8 ta rangli pechat",
            ),
        ))
        .await
        .expect("save");
    assert_eq!(saved.status(), StatusCode::OK);

    let blocked = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move",
            &token,
            r#"{
                "map_id":"zakaz-9001",
                "from_apparatus":"8 ta rangli pechat",
                "to_apparatus":"7 ta rangli pechat"
            }"#,
        ))
        .await
        .expect("blocked move");
    assert_eq!(blocked.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(blocked).await["error"], "move_not_allowed");

    let allowed = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move",
            &token,
            r#"{
                "map_id":"zakaz-9001",
                "from_apparatus":"8 ta rangli pechat",
                "to_apparatus":"9 ta rangli pechat"
            }"#,
        ))
        .await
        .expect("allowed move");
    assert_eq!(allowed.status(), StatusCode::OK);
    let body = json_body(allowed).await;
    assert_eq!(body["ok"], true);
    assert_eq!(
        body["saved"]["map"]["nodes"][1]["title"],
        "9 ta rangli pechat"
    );

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    let maps = json_body(list).await;
    assert_eq!(maps[0]["map"]["nodes"][1]["title"], "9 ta rangli pechat");
}

#[tokio::test]
async fn production_map_save_with_order_saves_map_and_template() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let map_json =
        pechat_order_map_json("zakaz-7777", "Atomic zakaz", "7777", "8 ta rangli pechat");
    let body = format!(
        r#"{{
            "map":{map_json},
            "template":{{
                "name":"atomic mahsulot",
                "product":"atomic mahsulot",
                "width_mm":650,
                "waste_percent":5,
                "first_layer_material":"pet",
                "first_layer_micron":"12",
                "second_layer_material":"pe oq",
                "second_layer_micron":"30"
            }}
        }}"#
    );
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body,
        ))
        .await
        .expect("save with order");
    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["ok"], true);
    assert_eq!(value["saved"]["map"]["id"], "zakaz-7777");
    assert_eq!(value["template"]["name"], "atomic mahsulot");
    assert_eq!(value["template"]["source_map_id"], "zakaz-7777");
    let template_id = value["template"]["id"]
        .as_str()
        .expect("template id")
        .to_string();
    assert!(!template_id.is_empty());
    assert!(
        value["template"]["code"]
            .as_str()
            .map(|code| !code.trim().is_empty())
            .unwrap_or(false)
    );

    let fetched = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps?id=zakaz-7777",
            &token,
        ))
        .await
        .expect("fetch map by id");
    assert_eq!(fetched.status(), StatusCode::OK);
    let fetched_value = json_body(fetched).await;
    assert_eq!(fetched_value["map"]["id"], "zakaz-7777");

    let cleanup_body = format!(r#"{{"id":"{template_id}"}}"#);
    let cleanup = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/calculate/orders/delete",
            &token,
            &cleanup_body,
        ))
        .await
        .expect("cleanup");
    assert_eq!(cleanup.status(), StatusCode::OK);
}

#[tokio::test]
async fn production_map_save_with_order_records_mini_order_without_blocking_response() {
    let sink = Arc::new(FakeProductionOrderSink::fail_after(Duration::from_millis(
        200,
    )));
    let mut state = test_state();
    state.production_orders = sink.clone();
    let token = session(&state, PrincipalRole::Admin).await;

    let map_json =
        pechat_order_map_json("zakaz-7799", "Catalog zakaz", "7799", "8 ta rangli pechat");
    let body = format!(
        r#"{{
            "map":{map_json},
            "template":{{
                "name":"mini mahsulot",
                "product":"mini mahsulot",
                "item_code":"ITEM-MINI",
                "width_mm":650,
                "waste_percent":5,
                "roll_count":7,
                "first_layer_material":"pet",
                "first_layer_micron":"12",
                "second_layer_material":"pe oq",
                "second_layer_micron":"30",
                "kg":500
            }}
        }}"#
    );

    let response = tokio::time::timeout(
        Duration::from_millis(75),
        build_router(state).oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body,
        )),
    )
    .await
    .expect("response must not wait for mini order write")
    .expect("save with order");

    assert_eq!(response.status(), StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn production_map_save_with_order_does_not_store_cloned_order_as_quick_template() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let map_json = pechat_order_map_json("zakaz-5555", "Dolce order", "5555", "8 ta rangli pechat");
    let body = format!(
        r#"{{
            "map":{map_json},
            "template":{{
                "id":"",
                "code":"5555",
                "order_number":"5555",
                "name":"dolce cake",
                "product":"dolce cake",
                "item_code":"DOLCE-001",
                "source_map_id":"quick-dolce-map",
                "width_mm":730,
                "waste_percent":5,
                "roll_count":7,
                "first_layer_material":"pet",
                "first_layer_micron":"12",
                "second_layer_material":"pe oq",
                "second_layer_micron":"50",
                "kg":500
            }}
        }}"#
    );

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body,
        ))
        .await
        .expect("save with order");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["ok"], true);
    assert_eq!(value["saved"]["map"]["id"], "zakaz-5555");
    assert!(value["template"].is_null());

    let list_response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/calculate/orders", &token))
        .await
        .expect("list calculate orders");
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_value = json_body(list_response).await;
    let rows = list_value["templates"].as_array().expect("templates array");
    assert!(rows.iter().all(|row| row["code"] != "5555"));
}

#[tokio::test]
async fn production_map_sequence_round_trips_on_server() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let put = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/sequence",
            &token,
            r#"{
                "apparatus":"8 ta rangli pechat",
                "order_ids":["zakaz-1111","zakaz-2222"," "]
            }"#,
        ))
        .await
        .expect("put sequence");
    assert_eq!(put.status(), StatusCode::OK);

    let get = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &token,
        ))
        .await
        .expect("get sequence");
    assert_eq!(get.status(), StatusCode::OK);
    let body = json_body(get).await;
    assert_eq!(
        body["sequences"]["8 ta rangli pechat"],
        serde_json::json!(["zakaz-1111", "zakaz-2222"])
    );
}

#[tokio::test]
async fn production_map_calendar_routes_are_removed() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let removed_prefix = ['d', 'a', 'i', 'l', 'y'].iter().collect::<String>();

    for path in [
        format!("/v1/mobile/admin/production-maps/{removed_prefix}-sequence"),
        format!("/v1/mobile/admin/production-maps/{removed_prefix}-apparatus-sequence"),
    ] {
        let response = build_router(state.clone())
            .oneshot(request("GET", &path, &token))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn production_map_save_with_order_rejects_invalid_template_before_saving_map() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let map_json = pechat_order_map_json(
        "zakaz-5555",
        "Invalid template zakaz",
        "5555",
        "8 ta rangli pechat",
    );
    let body = format!(r#"{{"map":{map_json},"template":{{"name":"x","product":""}}}}"#);
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Map must not be saved when the template is invalid.
    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    assert_eq!(
        json_body(list).await.as_array().map(|maps| maps.len()),
        Some(0)
    );
}

#[tokio::test]
async fn production_maps_list_falls_back_to_order_number_as_code() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json("zakaz-3333", "Legacy zakaz", "3333", "8 ta rangli pechat"),
        ))
        .await
        .expect("save");
    assert_eq!(save.status(), StatusCode::OK);

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    let maps = json_body(list).await;
    assert_eq!(maps[0]["map"]["code"], "3333");
}

#[tokio::test]
async fn production_map_order_number_is_immutable_on_update() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json("zakaz-1234", "Locked zakaz", "1234", "Paket aparat"),
        ))
        .await
        .expect("save");
    assert_eq!(save.status(), StatusCode::OK);

    let changed = pechat_order_map_json("zakaz-1234", "Locked zakaz", "5678", "Paket aparat");
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &changed,
        ))
        .await
        .expect("update");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "order_number_immutable");

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    let maps = json_body(list).await;
    assert_eq!(maps[0]["map"]["order_number"], "1234");
}

#[tokio::test]
async fn production_map_save_with_order_rolls_back_map_when_template_store_fails() {
    let state = test_state_with_failing_calculate();
    let token = session(&state, PrincipalRole::Admin).await;

    let map_json = pechat_order_map_json("zakaz-8888", "Rollback zakaz", "8888", "Paket aparat");
    let body = format!(
        r#"{{"map":{map_json},"template":{{
            "name":"rollback mahsulot",
            "product":"rollback mahsulot",
            "width_mm":650,
            "waste_percent":5,
            "first_layer_material":"pet",
            "first_layer_micron":"12",
            "second_layer_material":"pe oq",
            "second_layer_micron":"30"
        }}}}"#
    );
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body,
        ))
        .await
        .expect("with-order");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    assert_eq!(
        json_body(list).await.as_array().map(|maps| maps.len()),
        Some(0)
    );
}

#[tokio::test]
async fn production_map_batch_move_allows_seven_to_eight_color_pechat() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json_with_dims(
                "zakaz-3030",
                "Dual pechat order",
                "3030",
                "7 ta rangli pechat - A",
                7.0,
                650.0,
            ),
        ))
        .await
        .expect("save");
    assert_eq!(save.status(), StatusCode::OK);

    let moved = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            r#"{
                "from_apparatus":"7 ta rangli pechat",
                "to_apparatus":"8 ta rangli pechat",
                "map_ids":["zakaz-3030"]
            }"#,
        ))
        .await
        .expect("batch move");
    assert_eq!(moved.status(), StatusCode::OK);

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    let maps = json_body(list).await;
    let apparatus = maps[0]["map"]["nodes"]
        .as_array()
        .and_then(|nodes| {
            nodes
                .iter()
                .find_map(|node| (node["kind"] == "apparatus").then(|| node["title"].as_str()))
        })
        .flatten()
        .unwrap_or("");
    assert_eq!(apparatus, "8 ta rangli pechat");
}

#[tokio::test]
async fn production_map_batch_move_blocks_flexo_order_to_color_pechat() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &production_order_map_json_with_product(
                "zakaz-flexo-3031",
                "vitagum flexo zip paket",
                "FLEXO-3031",
                "3031",
                "Flexo pechat - A",
                7.0,
                650.0,
            ),
        ))
        .await
        .expect("save");
    assert_eq!(save.status(), StatusCode::OK);

    let moved = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            r#"{
                "from_apparatus":"Flexo pechat - A",
                "to_apparatus":"8 ta rangli pechat",
                "map_ids":["zakaz-flexo-3031"]
            }"#,
        ))
        .await
        .expect("batch move");
    assert_eq!(moved.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(moved).await["error"], "move_not_allowed");
}

#[tokio::test]
async fn production_map_batch_move_reassigns_alternative_apparatus_assignment() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"zakaz-alt-move",
                "product_code":"ALT-MOVE",
                "title":"Alternative move order",
                "roll_count":7,
                "width_mm":650,
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {
                        "id":"apparatus-7",
                        "kind":"apparatus",
                        "title":"7 ta rangli pechat",
                        "alternative_group_id":"alt-pechat",
                        "alternative_group_label":"pechat",
                        "alternative_assigned_title":"7 ta rangli pechat"
                    },
                    {
                        "id":"apparatus-8",
                        "kind":"apparatus",
                        "title":"8 ta rangli pechat",
                        "alternative_group_id":"alt-pechat",
                        "alternative_group_label":"pechat",
                        "alternative_assigned_title":"7 ta rangli pechat"
                    },
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"apparatus-7"},
                    {"from":"apparatus-7","to":"end"},
                    {"from":"start","to":"apparatus-8"},
                    {"from":"apparatus-8","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("save");
    assert_eq!(save.status(), StatusCode::OK);

    let moved = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            r#"{
                "from_apparatus":"7 ta rangli pechat",
                "to_apparatus":"8 ta rangli pechat",
                "map_ids":["zakaz-alt-move"]
            }"#,
        ))
        .await
        .expect("batch move");
    assert_eq!(moved.status(), StatusCode::OK);

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    let maps = json_body(list).await;
    let nodes = maps[0]["map"]["nodes"].as_array().expect("nodes");
    let apparatus_titles: Vec<&str> = nodes
        .iter()
        .filter_map(|node| (node["kind"] == "apparatus").then(|| node["title"].as_str()))
        .flatten()
        .collect();
    let assigned_titles: Vec<&str> = nodes
        .iter()
        .filter_map(|node| {
            (node["kind"] == "apparatus").then(|| node["alternative_assigned_title"].as_str())
        })
        .flatten()
        .collect();
    assert_eq!(
        apparatus_titles,
        vec!["7 ta rangli pechat", "8 ta rangli pechat"]
    );
    assert_eq!(
        assigned_titles,
        vec!["8 ta rangli pechat", "8 ta rangli pechat"]
    );
}

#[tokio::test]
async fn production_map_batch_move_preserves_alternative_node_titles_when_target_is_absent() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"zakaz-alt-title-preserve",
                "product_code":"ALT-TITLE",
                "title":"Alternative title preserve order",
                "roll_count":7,
                "width_mm":630,
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {
                        "id":"apparatus-7-a",
                        "kind":"apparatus",
                        "title":"7 ta rangli pechat - A",
                        "alternative_group_id":"alt-pechat",
                        "alternative_group_label":"pechat",
                        "alternative_assigned_title":"7 ta rangli pechat - A"
                    },
                    {
                        "id":"apparatus-7-b",
                        "kind":"apparatus",
                        "title":"7 ta rangli pechat - A",
                        "alternative_group_id":"alt-pechat",
                        "alternative_group_label":"pechat",
                        "alternative_assigned_title":"7 ta rangli pechat - A"
                    },
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"apparatus-7-a"},
                    {"from":"apparatus-7-a","to":"end"},
                    {"from":"start","to":"apparatus-7-b"},
                    {"from":"apparatus-7-b","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("save");
    assert_eq!(save.status(), StatusCode::OK);

    let moved = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            r#"{
                "from_apparatus":"7 ta rangli pechat - A",
                "to_apparatus":"8 ta rangli pechat - A",
                "map_ids":["zakaz-alt-title-preserve"]
            }"#,
        ))
        .await
        .expect("batch move");
    assert_eq!(moved.status(), StatusCode::OK);

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    let maps = json_body(list).await;
    let nodes = maps[0]["map"]["nodes"].as_array().expect("nodes");
    let apparatus_titles: Vec<&str> = nodes
        .iter()
        .filter_map(|node| (node["kind"] == "apparatus").then(|| node["title"].as_str()))
        .flatten()
        .collect();
    let assigned_titles: Vec<&str> = nodes
        .iter()
        .filter_map(|node| {
            (node["kind"] == "apparatus").then(|| node["alternative_assigned_title"].as_str())
        })
        .flatten()
        .collect();
    assert_eq!(
        apparatus_titles,
        vec!["7 ta rangli pechat - A", "7 ta rangli pechat - A"]
    );
    assert_eq!(
        assigned_titles,
        vec!["8 ta rangli pechat - A", "8 ta rangli pechat - A"]
    );
}

#[tokio::test]
async fn production_map_batch_move_keeps_laminatsiya_alternatives_in_group() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"zakaz-lamin-alt-move",
                "product_code":"LAMIN-ALT",
                "title":"Laminatsiya alternative move",
                "roll_count":7,
                "width_mm":900,
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {
                        "id":"lamin-1",
                        "kind":"apparatus",
                        "title":"Laminatsiya 1 - A",
                        "alternative_group_id":"alt-lamin",
                        "alternative_group_label":"laminatsiya",
                        "alternative_assigned_title":"Laminatsiya 1 - A"
                    },
                    {
                        "id":"lamin-2",
                        "kind":"apparatus",
                        "title":"Laminatsiya 2 - A",
                        "alternative_group_id":"alt-lamin",
                        "alternative_group_label":"laminatsiya",
                        "alternative_assigned_title":"Laminatsiya 1 - A"
                    },
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"lamin-1"},
                    {"from":"lamin-1","to":"end"},
                    {"from":"start","to":"lamin-2"},
                    {"from":"lamin-2","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("save");
    assert_eq!(save.status(), StatusCode::OK);

    let moved = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            r#"{
                "from_apparatus":"Laminatsiya 1 - A",
                "to_apparatus":"Laminatsiya 2 - A",
                "map_ids":["zakaz-lamin-alt-move"]
            }"#,
        ))
        .await
        .expect("move to laminatsiya");
    assert_eq!(moved.status(), StatusCode::OK);
    let moved_body = json_body(moved).await;
    let assigned_titles: Vec<&str> = moved_body["saved"][0]["map"]["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .filter_map(|node| {
            (node["kind"] == "apparatus").then(|| node["alternative_assigned_title"].as_str())
        })
        .flatten()
        .collect();
    assert_eq!(
        assigned_titles,
        vec!["Laminatsiya 2 - A", "Laminatsiya 2 - A"]
    );

    let blocked = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            r#"{
                "from_apparatus":"Laminatsiya 2 - A",
                "to_apparatus":"Paket aparat - A",
                "map_ids":["zakaz-lamin-alt-move"]
            }"#,
        ))
        .await
        .expect("move to paket");
    assert_eq!(blocked.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(blocked).await["error"], "move_not_allowed");

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    let maps = json_body(list).await;
    let assigned_after_block: Vec<&str> = maps[0]["map"]["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .filter_map(|node| {
            (node["kind"] == "apparatus").then(|| node["alternative_assigned_title"].as_str())
        })
        .flatten()
        .collect();
    assert_eq!(
        assigned_after_block,
        vec!["Laminatsiya 2 - A", "Laminatsiya 2 - A"]
    );
}

#[tokio::test]
async fn production_map_batch_move_is_all_or_nothing() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    for number in ["1010", "2020"] {
        let save = build_router(state.clone())
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &token,
                &pechat_order_map_json_with_dims(
                    &format!("zakaz-{number}"),
                    &format!("Batch {number}"),
                    number,
                    "7 ta rangli pechat",
                    7.0,
                    650.0,
                ),
            ))
            .await
            .expect("save");
        assert_eq!(save.status(), StatusCode::OK);
    }

    let bad_batch = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            r#"{
                "from_apparatus":"7 ta rangli pechat",
                "to_apparatus":"8 ta rangli pechat",
                "map_ids":["zakaz-1010","zakaz-missing"]
            }"#,
        ))
        .await
        .expect("batch");
    assert_eq!(bad_batch.status(), StatusCode::NOT_FOUND);

    let verify = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    for map in json_body(verify).await.as_array().expect("maps") {
        let apparatus = map["map"]["nodes"]
            .as_array()
            .and_then(|nodes| {
                nodes
                    .iter()
                    .find_map(|node| (node["kind"] == "apparatus").then(|| node["title"].as_str()))
            })
            .flatten()
            .unwrap_or("");
        assert_eq!(apparatus, "7 ta rangli pechat");
    }

    let ok_batch = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            r#"{
                "from_apparatus":"7 ta rangli pechat",
                "to_apparatus":"8 ta rangli pechat",
                "map_ids":["zakaz-1010","zakaz-2020"]
            }"#,
        ))
        .await
        .expect("batch ok");
    assert_eq!(ok_batch.status(), StatusCode::OK);
    assert_eq!(
        json_body(ok_batch).await["saved"]
            .as_array()
            .map(|v| v.len()),
        Some(2)
    );
}

#[tokio::test]
async fn production_map_batch_move_stress_moves_many_orders_atomically() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    for index in 0..24 {
        let number = format!("{index:04}");
        let save = build_router(state.clone())
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &token,
                &pechat_order_map_json_with_dims(
                    &format!("zakaz-{number}"),
                    &format!("Stress {number}"),
                    &number,
                    "7 ta rangli pechat",
                    7.0,
                    650.0,
                ),
            ))
            .await
            .expect("save");
        assert_eq!(save.status(), StatusCode::OK);
    }

    let map_ids: Vec<String> = (0..24)
        .map(|index| format!("\"zakaz-{index:04}\""))
        .collect();
    let body = format!(
        r#"{{"from_apparatus":"7 ta rangli pechat","to_apparatus":"8 ta rangli pechat","map_ids":[{}]}}"#,
        map_ids.join(",")
    );
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            &body,
        ))
        .await
        .expect("stress batch");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["saved"]
            .as_array()
            .map(|v| v.len()),
        Some(24)
    );
}

struct FailCalculateUpsertStore;

#[async_trait]
impl CalculateOrderStorePort for FailCalculateUpsertStore {
    async fn list(
        &self,
        _owner_key: &str,
    ) -> Result<Vec<CalculateOrderTemplate>, CalculateOrderError> {
        Ok(Vec::new())
    }

    async fn upsert(
        &self,
        _owner_key: &str,
        template: CalculateOrderTemplate,
    ) -> Result<CalculateOrderTemplate, CalculateOrderError> {
        let _ = template;
        Err(CalculateOrderError::StoreFailed)
    }

    async fn delete(&self, _owner_key: &str, _id: &str) -> Result<(), CalculateOrderError> {
        Ok(())
    }

    async fn save_image(
        &self,
        _owner_key: &str,
        _image: CalculateOrderImage,
    ) -> Result<CalculateOrderImage, CalculateOrderError> {
        Err(CalculateOrderError::StoreFailed)
    }

    async fn get_image(
        &self,
        _owner_key: &str,
        _image_id: &str,
    ) -> Result<Option<CalculateOrderImage>, CalculateOrderError> {
        Err(CalculateOrderError::StoreFailed)
    }
}

#[derive(Debug)]
struct FakeProductionOrderSink {
    calls: AtomicUsize,
    fail: bool,
    delay: Option<Duration>,
}

impl FakeProductionOrderSink {
    fn fail_after(delay: Duration) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            fail: true,
            delay: Some(delay),
        }
    }
}

#[async_trait]
impl MiniOrderSink for FakeProductionOrderSink {
    async fn save_order(
        &self,
        _map: &crate::core::production_map::ProductionMapDefinition,
        _template: &CalculateOrderTemplate,
    ) -> Result<(), MiniOrderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        if self.fail {
            Err(MiniOrderError::StoreFailed)
        } else {
            Ok(())
        }
    }
}

fn test_state_with_failing_calculate() -> AppState {
    let mut state = test_state();
    state.calculate_orders = Arc::new(FailCalculateUpsertStore);
    state
}

fn pechat_order_map_json(id: &str, title: &str, order_number: &str, apparatus: &str) -> String {
    pechat_order_map_json_with_dims(id, title, order_number, apparatus, 7.0, 1250.0)
}

fn pechat_order_map_json_with_dims(
    id: &str,
    title: &str,
    order_number: &str,
    apparatus: &str,
    roll_count: f64,
    width_mm: f64,
) -> String {
    production_order_map_json_with_product(
        id,
        title,
        &format!("PECHAT-{order_number}"),
        order_number,
        apparatus,
        roll_count,
        width_mm,
    )
}

fn production_order_map_json_with_product(
    id: &str,
    title: &str,
    product_code: &str,
    order_number: &str,
    apparatus: &str,
    roll_count: f64,
    width_mm: f64,
) -> String {
    format!(
        r#"{{
            "id":"{id}",
            "product_code":"{product_code}",
            "title":"{title}",
            "order_number":"{order_number}",
            "roll_count":{roll_count},
            "width_mm":{width_mm},
            "nodes":[
                {{"id":"start","kind":"start","title":"Start"}},
                {{"id":"apparatus","kind":"apparatus","title":"{apparatus}"}},
                {{"id":"end","kind":"end","title":"End"}}
            ],
            "edges":[
                {{"from":"start","to":"apparatus"}},
                {{"from":"apparatus","to":"end"}}
            ]
        }}"#
    )
}

fn laminatsiya_order_map_json(id: &str, width_mm: f64) -> String {
    format!(
        r#"{{
            "id":"{id}",
            "product_code":"LAMIN-{id}",
            "title":"Laminatsiya order",
            "order_number":"{id}",
            "roll_count":7,
            "width_mm":{width_mm},
            "nodes":[
                {{"id":"start","kind":"start","title":"Start"}},
                {{"id":"laminatsiya","kind":"task","title":"Laminatsiya - A"}},
                {{"id":"end","kind":"end","title":"End"}}
            ],
            "edges":[
                {{"from":"start","to":"laminatsiya"}},
                {{"from":"laminatsiya","to":"end"}}
            ]
        }}"#
    )
}

#[tokio::test]
async fn admin_production_map_run_returns_calculated_tasks() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"hotlunch-test",
                "product_code":"HOTLUNCH",
                "title":"Hotlunch test",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {
                        "id":"formula",
                        "kind":"formula",
                        "title":"CPP hisob",
                        "formula":{"target":"cpp_kg","expression":"order_qty * 1.08"}
                    },
                    {
                        "id":"task",
                        "kind":"task",
                        "title":"Rezkaga yuborish",
                        "role_code":"rezkachi",
                        "qty_formula":"cpp_kg",
                        "from_location":"CPP ombor",
                        "to_location":"Rezka apparat"
                    },
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"formula"},
                    {"from":"formula","to":"task"},
                    {"from":"task","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("save response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/run",
            &token,
            r#"{"map_id":"hotlunch-test","order_qty":100}"#,
        ))
        .await
        .expect("run response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["variables"]["cpp_kg"], 108.0);
    assert_eq!(value["tasks"][0]["task_kind"], "create_task");
    assert_eq!(value["tasks"][0]["role_code"], "rezkachi");
    assert_eq!(value["tasks"][0]["qty"], 108.0);
    assert_eq!(value["tasks"][0]["from_location"], "CPP ombor");
    assert_eq!(value["tasks"][0]["to_location"], "Rezka apparat");
}

#[tokio::test]
async fn production_map_manage_capability_can_save_maps() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/roles",
            &admin_token,
            r#"{
                "id":"production_mapper",
                "label":"Production mapper",
                "capability_codes":["production.map.manage"]
            }"#,
        ))
        .await
        .expect("role response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/roles", &admin_token))
        .await
        .expect("roles response");
    assert_eq!(response.status(), StatusCode::OK);
    let roles = json_body(response).await;
    assert!(
        roles
            .as_array()
            .expect("roles")
            .iter()
            .any(|role| role["id"] == "aparatchi")
    );

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &admin_token,
            r#"{
                "principal_role":"werka",
                "principal_ref":"werka",
                "role_id":"production_mapper"
            }"#,
        ))
        .await
        .expect("assignment response");
    assert_eq!(response.status(), StatusCode::OK);

    let mapper_token = session_for(&state, PrincipalRole::Werka, "werka").await;
    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &mapper_token,
            r#"{
                "id":"hotlunch-test",
                "product_code":"HOTLUNCH",
                "title":"Hotlunch test",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[{"from":"start","to":"end"}]
            }"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn apparatus_queue_read_capability_can_only_read_production_maps() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            r#"{
                "id":"queue-test",
                "product_code":"HOTLUNCH",
                "title":"Queue test",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"apparatus","kind":"apparatus","title":"Godex aparat - DEMO"},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"apparatus"},
                    {"from":"apparatus","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("save map");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/roles", &admin_token))
        .await
        .expect("roles response");
    assert_eq!(response.status(), StatusCode::OK);
    let roles = json_body(response).await;
    assert!(
        roles
            .as_array()
            .expect("roles")
            .iter()
            .any(|role| role["id"] == "aparatchi"),
        "{roles}"
    );

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &admin_token,
            r#"{
                "principal_role":"werka",
                "principal_ref":"werka",
                "role_id":"aparatchi",
                "assigned_apparatus":["Godex aparat - DEMO"]
            }"#,
        ))
        .await
        .expect("assignment response");
    let status = response.status();
    let body = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["assigned_apparatus"],
        serde_json::json!(["Godex aparat - DEMO"])
    );

    let queue_token = session_for(&state, PrincipalRole::Werka, "werka").await;
    let response = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps",
            &queue_token,
        ))
        .await
        .expect("read response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await[0]["map"]["id"], "queue-test");

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &queue_token,
            r#"{
                "id":"queue-test-2",
                "product_code":"HOTLUNCH",
                "title":"Queue test 2",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[{"from":"start","to":"end"}]
            }"#,
        ))
        .await
        .expect("write response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_access_capability_can_save_production_maps() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/roles",
            &admin_token,
            r#"{
                "id":"admin_only",
                "label":"Admin only",
                "capability_codes":["admin.access"]
            }"#,
        ))
        .await
        .expect("role response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &admin_token,
            r#"{
                "principal_role":"werka",
                "principal_ref":"werka",
                "role_id":"admin_only"
            }"#,
        ))
        .await
        .expect("assignment response");
    assert_eq!(response.status(), StatusCode::OK);

    let admin_only_token = session_for(&state, PrincipalRole::Werka, "werka").await;
    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_only_token,
            r#"{
                "id":"hotlunch-test",
                "product_code":"HOTLUNCH",
                "title":"Hotlunch test",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[{"from":"start","to":"end"}]
            }"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn admin_settings_returns_config_shape_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/settings", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["default_uom"], "Kg");
    assert_eq!(value["werka_name"], "Werka");
    assert_eq!(value["admin_name"], "Admin");
}

#[tokio::test]
async fn admin_capabilities_returns_role_builder_catalog() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let supplier_token = session(&state, PrincipalRole::Supplier).await;

    let forbidden = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/capabilities",
            &supplier_token,
        ))
        .await
        .expect("response");
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(forbidden).await["error"], "forbidden");

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/capabilities",
            &admin_token,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    let items = value.as_array().expect("catalog array");

    assert!(items.iter().any(|item| item["code"] == "admin.access"));
    assert!(
        items
            .iter()
            .any(|item| item["code"] == "gscale.catalog.read")
    );
    assert!(items.iter().any(|item| {
        item["default_roles"]
            .as_array()
            .expect("roles")
            .contains(&serde_json::json!("werka"))
    }));
}

#[tokio::test]
async fn admin_roles_can_list_system_roles_and_save_custom_packages() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let supplier_token = session(&state, PrincipalRole::Supplier).await;

    let forbidden = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/roles", &supplier_token))
        .await
        .expect("response");
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(forbidden).await["error"], "forbidden");

    let response = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/roles", &admin_token))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    let roles = value.as_array().expect("roles array");
    assert!(roles.iter().any(|role| role["id"] == "admin"));
    assert!(roles.iter().any(|role| role["id"] == "werka"));

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/roles",
            &admin_token,
            r#"{
                "id":"scale_operator",
                "label":"Scale operator",
                "capability_codes":[
                    "gscale.catalog.read",
                    "gscale.print",
                    "rps.batch.manage",
                    "gscale.print"
                ]
            }"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let saved = json_body(response).await;
    assert_eq!(saved["id"], "scale_operator");
    assert_eq!(saved["system"], false);
    assert!(saved.get("base_role").is_none());
    assert_eq!(
        saved["capability_codes"],
        serde_json::json!(["gscale.catalog.read", "gscale.print", "rps.batch.manage"])
    );

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/roles", &admin_token))
        .await
        .expect("response");
    let value = json_body(response).await;
    assert!(value.as_array().expect("roles").iter().any(|role| {
        role["id"] == "scale_operator" && role["capability_codes"][0] == "gscale.catalog.read"
    }));
}

#[tokio::test]
async fn admin_roles_hide_legacy_custom_roles_that_conflict_with_system_roles() {
    let mut state = test_state();
    let role_store = Arc::new(MemoryRoleDefinitionStore::new());
    role_store
        .put_role_definition(RoleDefinition {
            id: "aparatchi".to_string(),
            label: "Custom aparatchi".to_string(),
            base_role: None,
            capability_codes: vec!["catalog.item.read".to_string()],
            system: false,
        })
        .await
        .expect("put legacy role");
    state.admin = state.admin.with_role_store(role_store);
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/roles", &admin_token))
        .await
        .expect("roles response");

    assert_eq!(response.status(), StatusCode::OK);
    let roles = json_body(response).await;
    let aparatchi_roles: Vec<_> = roles
        .as_array()
        .expect("roles")
        .iter()
        .filter(|role| role["id"] == "aparatchi")
        .collect();
    assert_eq!(aparatchi_roles.len(), 1, "{roles}");
    assert_eq!(aparatchi_roles[0]["label"], "Aparatchi");
    assert_eq!(aparatchi_roles[0]["system"], true);
}

#[tokio::test]
async fn admin_role_assignment_limits_runtime_capabilities() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/roles",
            &admin_token,
            r#"{
                "id":"catalog_only",
                "label":"Catalog only",
                "capability_codes":["gscale.catalog.read"]
            }"#,
        ))
        .await
        .expect("role response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &admin_token,
            r#"{
                "principal_role":"werka",
                "principal_ref":"werka",
                "role_id":"catalog_only"
            }"#,
        ))
        .await
        .expect("assignment response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["role_id"], "catalog_only");

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &admin_token,
            r#"{
                "principal_role":"supplier",
                "principal_ref":"SUP-001",
                "role_id":"catalog_only"
            }"#,
        ))
        .await
        .expect("supplier assignment response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["role_id"], "catalog_only");

    let werka_token = session_for(&state, PrincipalRole::Werka, "werka").await;
    let response = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/gscale/items?limit=1",
            &werka_token,
        ))
        .await
        .expect("gscale items response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state.clone())
        .oneshot(request("POST", "/v1/mobile/werka/summary", &werka_token))
        .await
        .expect("werka summary response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/rps/batch/start",
            &werka_token,
            r#"{"item_code":"ITEM-001","warehouse":"Stores - CH"}"#,
        ))
        .await
        .expect("rps start response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn login_returns_effective_capabilities_for_assigned_custom_role() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/roles",
            &admin_token,
            r#"{
                "id":"scale_only",
                "label":"Scale only",
                "capability_codes":["gscale.catalog.read","gscale.print","rps.batch.manage"]
            }"#,
        ))
        .await
        .expect("role response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &admin_token,
            r#"{
                "principal_role":"werka",
                "principal_ref":"werka",
                "role_id":"scale_only",
                "assigned_apparatus":["Paket aparat"]
            }"#,
        ))
        .await
        .expect("assignment response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            r#"{"phone":"+99888862440","code":"20ABCDEF1234"}"#,
        ))
        .await
        .expect("login response");
    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(
        value["capabilities"],
        serde_json::json!(["gscale.catalog.read", "gscale.print", "rps.batch.manage"])
    );
    assert_eq!(
        value["assigned_apparatus"],
        serde_json::json!(["Paket aparat"])
    );
}

#[tokio::test]
async fn admin_settings_ignores_state_read_failure_like_go() {
    let mut state = test_state();
    let admin_port = Arc::new(FakeAdminReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(admin_port.clone())
        .with_write_port(admin_port)
        .with_state_port(Arc::new(FailingAdminStatePort));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/settings", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["werka_code_locked"], false);
    assert_eq!(value["werka_code_retry_after_sec"], 0);
}

#[tokio::test]
async fn admin_suppliers_summary_failure_uses_go_error_text() {
    let mut state = test_state();
    let admin_port = Arc::new(FakeAdminReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(admin_port.clone())
        .with_write_port(admin_port)
        .with_state_port(Arc::new(FailingAdminStatePort));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/suppliers", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "supplier summary failed"
    );
}

#[tokio::test]
async fn admin_settings_put_updates_default_uom_like_go() {
    let mut state = test_state();
    let admin_port = Arc::new(FakeAdminReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(admin_port.clone())
        .with_write_port(admin_port)
        .with_state_port(Arc::new(FakeAdminStatePort::new()))
        .with_auth_config_sink(Arc::new(state.auth.clone()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/settings",
            &token,
            r#"{
                "default_target_warehouse":"Stores - NEW",
                "default_uom":"",
                "werka_phone":"+998881111111",
                "werka_name":"New Werka",
                "werka_code":"20NEW",
                "werka_code_locked":false,
                "werka_code_retry_after_sec":0,
                "admin_phone":"+998882222222",
                "admin_name":"New Admin"
            }"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["default_target_warehouse"], "Stores - NEW");
    assert_eq!(value["default_uom"], "Kg");
}

#[tokio::test]
async fn admin_suppliers_page_filters_removed_and_counts_blocked_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/suppliers", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["summary"]["total_suppliers"], 3);
    assert_eq!(value["summary"]["active_suppliers"], 1);
    assert_eq!(value["summary"]["blocked_suppliers"], 2);
    assert_eq!(value["suppliers"].as_array().expect("suppliers").len(), 2);
    assert_eq!(value["suppliers"][0]["ref"], "SUP-001");
    assert_eq!(value["suppliers"][0]["assigned_item_count"], 2);
    assert_eq!(value["customers"][0]["ref"], "CUST-001");
}

#[tokio::test]
async fn admin_supplier_detail_requires_ref_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/suppliers/detail", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "ref is required");
}

#[tokio::test]
async fn admin_supplier_detail_returns_assigned_items_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/suppliers/detail?ref=SUP-001",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["ref"], "SUP-001");
    assert_eq!(value["code"], "10CUSTOM");
    assert_eq!(value["assigned_items"][0]["code"], "ITEM-001");
}

#[tokio::test]
async fn admin_supplier_detail_uses_permission_fallback_like_go() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(AssignedItemsErrorReadPort::permission()))
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/suppliers/detail?ref=SUP-001",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["assigned_items"].as_array().expect("items").len(), 2);
    assert_eq!(value["assigned_items"][0]["code"], "ITEM-001");
}

#[tokio::test]
async fn admin_supplier_detail_does_not_fallback_on_non_permission_error() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(AssignedItemsErrorReadPort::lookup_failed()))
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/suppliers/detail?ref=SUP-001",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["error"], "supplier detail failed");
}

#[tokio::test]
async fn admin_assigned_supplier_items_permission_without_cache_is_empty_like_go() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(AssignedItemsErrorReadPort::permission()))
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/suppliers/items/assigned?ref=SUP-002",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        json_body(response)
            .await
            .as_array()
            .expect("items")
            .is_empty()
    );
}

#[tokio::test]
async fn admin_customers_and_items_read_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let customers = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/customers/list", &token))
        .await
        .expect("response");
    assert_eq!(customers.status(), StatusCode::OK);
    assert_eq!(json_body(customers).await[0]["ref"], "CUST-001");

    let items = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/items?q=rice&limit=5&offset=1",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(items.status(), StatusCode::OK);
    assert_eq!(json_body(items).await[0]["item_group"], "Products");

    let group_items = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/items?group=Products&limit=5",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(group_items.status(), StatusCode::OK);
    assert_eq!(json_body(group_items).await[0]["code"], "ITEM-001");

    let groups = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/item-groups", &token))
        .await
        .expect("response");
    assert_eq!(groups.status(), StatusCode::OK);
    assert_eq!(json_body(groups).await[0], "All Item Groups");
}

#[tokio::test]
async fn admin_customer_list_passes_query_to_read_port() {
    let mut state = test_state();
    let seen_query = Arc::new(Mutex::new(String::new()));
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(QueryCaptureReadPort {
            seen_query: seen_query.clone(),
        }))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/customers/list?q=ali&limit=5&offset=2",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await[0]["ref"], "CUST-QUERY");
    assert_eq!(&*seen_query.lock().await, "ali");
}

#[tokio::test]
async fn admin_warehouses_returns_real_warehouse_names() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses?q=Stores&limit=5",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body[0]["warehouse"], "Stores - CH");
    assert_eq!(body[0]["company"], "Company");
}

#[tokio::test]
async fn admin_warehouses_filters_by_parent() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses?parent=Aparat&limit=5",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body[0]["warehouse"], "Godex aparat - CH");
    assert_eq!(body[0]["parent_warehouse"], "aparat - A");
}

#[tokio::test]
async fn admin_apparatus_groups_round_trip_on_server() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let saved = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/apparatus-groups",
            &token,
            r#"{"name":" pechat ","apparatus":[" 7 ta rangli pechat ","8 ta rangli pechat","7 ta rangli pechat"]}"#,
        ))
        .await
        .expect("response");
    assert_eq!(saved.status(), StatusCode::OK);
    let saved_body = json_body(saved).await;
    assert_eq!(saved_body["name"], "pechat");
    assert_eq!(
        saved_body["apparatus"],
        serde_json::json!(["7 ta rangli pechat", "8 ta rangli pechat"])
    );

    let listed = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/apparatus-groups", &token))
        .await
        .expect("response");
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = json_body(listed).await;
    assert_eq!(listed_body[0]["name"], "pechat");
    assert_eq!(
        listed_body[0]["apparatus"],
        serde_json::json!(["7 ta rangli pechat", "8 ta rangli pechat"])
    );
}

#[tokio::test]
async fn admin_can_create_apparatus_and_list_it_as_apparat_warehouse() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            r#"{"warehouse":" Bobst 1 "}"#,
        ))
        .await
        .expect("create apparatus");
    assert_eq!(created.status(), StatusCode::OK);
    let created_body = json_body(created).await;
    assert_eq!(created_body["warehouse"], "Bobst 1");
    assert_eq!(created_body["parent_warehouse"], "aparat - A");

    let listed = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses?parent=aparat%20-%20A&limit=50",
            &token,
        ))
        .await
        .expect("list apparatus");
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = json_body(listed).await;
    assert!(
        listed_body
            .as_array()
            .expect("array")
            .iter()
            .any(|item| item["warehouse"] == "Bobst 1")
    );
}

#[tokio::test]
async fn admin_item_group_tree_returns_parent_shape() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/item-groups/tree", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value[0]["name"], "All Item Groups");
    assert_eq!(value[1]["name"], "Xomashyo");
    assert_eq!(value[1]["parent_item_group"], "All Item Groups");
    assert_eq!(value[2]["name"], "plyonka");
    assert_eq!(value[2]["parent_item_group"], "Xomashyo");
}

#[tokio::test]
async fn admin_item_group_create_returns_item_group_shape() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let group = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"Kley","parent":"Kraska","is_group":false}"#,
        ))
        .await
        .expect("response");
    assert_eq!(group.status(), StatusCode::OK);
    let value = json_body(group).await;
    assert_eq!(value["name"], "Kley");
    assert_eq!(value["item_group_name"], "Kley");
    assert_eq!(value["parent_item_group"], "Kraska");
    assert_eq!(value["is_group"], false);

    let invalid = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"","parent":"All Item Groups","is_group":true}"#,
        ))
        .await
        .expect("response");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid).await["error"],
        "item group name is required"
    );
}

#[tokio::test]
async fn admin_item_group_parent_move_returns_item_group_shape() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let moved = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"Xomashyo","parent":"All Item Groups"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(moved.status(), StatusCode::OK);
    let value = json_body(moved).await;
    assert_eq!(value["name"], "Xomashyo");
    assert_eq!(value["item_group_name"], "Xomashyo");
    assert_eq!(value["parent_item_group"], "All Item Groups");
    assert_eq!(value["is_group"], true);

    let invalid_root = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"All Item Groups","parent":"Products"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(invalid_root.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid_root).await["error"],
        "root item group cannot be moved"
    );

    let invalid_cycle = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"Xomashyo","parent":"Xomashyo"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(invalid_cycle.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid_cycle).await["error"],
        "item group cannot be its own parent"
    );
}

#[tokio::test]
async fn admin_customer_detail_errors_are_500_like_go() {
    let mut state = test_state();
    let failing_read_port = Arc::new(CustomerItemsFailReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(failing_read_port)
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/customers/detail?ref=CUST-001",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["error"], "customer detail failed");
}

#[tokio::test]
async fn admin_customer_code_regenerate_cooldown_is_500_like_go() {
    let mut state = test_state();
    let admin_port = Arc::new(FakeAdminReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(admin_port.clone())
        .with_write_port(admin_port)
        .with_state_port(Arc::new(LockedCustomerStatePort));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/customers/code/regenerate?ref=CUST-001",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "customer code regenerate failed"
    );
}

#[tokio::test]
async fn admin_supplier_phone_not_found_is_404_like_go() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(FakeAdminReadPort))
        .with_write_port(Arc::new(MissingSupplierWritePort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/suppliers/phone?ref=SUP-MISSING",
            &token,
            r#"{"phone":"+998901111111"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(response).await["error"], "supplier not found");
}

#[tokio::test]
async fn admin_supplier_phone_skips_write_for_removed_supplier_like_go() {
    let mut state = test_state();
    let writes = Arc::new(CountingSupplierWritePort::default());
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(FakeAdminReadPort))
        .with_write_port(writes.clone())
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/suppliers/phone?ref=SUP-003",
            &token,
            r#"{"phone":"+998901111111"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(response).await["error"], "supplier not found");
    assert_eq!(writes.supplier_phone_updates.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn admin_supplier_items_invalid_item_is_500_like_go() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(MissingItemsReadPort))
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/suppliers/items?ref=SUP-001",
            &token,
            r#"{"item_codes":["ITEM-MISSING"]}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "supplier items update failed"
    );
}

#[tokio::test]
async fn admin_activity_returns_empty_without_history_provider() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/activity", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response)
            .await
            .as_array()
            .expect("activity")
            .len(),
        0
    );
}

#[tokio::test]
async fn admin_activity_limits_history_to_30_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(ActivityLookup));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/activity", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    let items = value.as_array().expect("activity array");
    assert_eq!(items.len(), 30);
    assert_eq!(items[0]["id"], "REC-000");
    assert_eq!(items[29]["id"], "REC-029");
}

#[tokio::test]
async fn admin_settings_put_updates_auth_runtime_like_go() {
    let mut state = test_state();
    state.admin = state
        .admin
        .clone()
        .with_auth_config_sink(Arc::new(state.auth.clone()));
    let token = session(&state, PrincipalRole::Admin).await;

    let update = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/settings",
            &token,
            r#"{
                "default_target_warehouse":"Stores - CH",
                "default_uom":"Kg",
                "werka_phone":"+998881111111",
                "werka_name":"Updated Werka",
                "werka_code":"20UPDATED",
                "werka_code_locked":false,
                "werka_code_retry_after_sec":0,
                "admin_phone":"+998882222222",
                "admin_name":"Updated Admin"
            }"#,
        ))
        .await
        .expect("response");
    assert_eq!(update.status(), StatusCode::OK);

    let old = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            r#"{"phone":"+998881111111","code":"20ABCDEF1234"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(old.status(), StatusCode::UNAUTHORIZED);

    let new = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            r#"{"phone":"+998881111111","code":"20UPDATED"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(new.status(), StatusCode::OK);
    let value = json_body(new).await;
    assert_eq!(value["profile"]["role"], "werka");
    assert_eq!(value["profile"]["display_name"], "Updated Werka");
}

#[tokio::test]
async fn admin_create_supplier_and_customer_mutations_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let supplier = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/suppliers",
            &token,
            r#"{"name":"New Supplier","phone":"+998909999999"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(supplier.status(), StatusCode::OK);
    let value = json_body(supplier).await;
    assert_eq!(value["ref"], "SUP-NEW");
    assert_eq!(value["phone"], "+998909999999");

    let customer = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/customers",
            &token,
            r#"{"name":"New Customer","phone":"+998901234567"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(customer.status(), StatusCode::OK);
    let value = json_body(customer).await;
    assert_eq!(value["ref"], "CUST-NEW");
    assert_eq!(value["name"], "New Customer");
}

#[tokio::test]
async fn admin_create_customer_rejects_duplicate_phone() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/customers",
            &token,
            r#"{"name":"Duplicate Customer","phone":"+998904444444"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "phone already exists");
}

#[tokio::test]
async fn admin_create_customer_rejects_local_format_duplicate_phone() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(LocalPhoneDuplicateReadPort))
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/customers",
            &token,
            r#"{"name":"Duplicate Customer","phone":"110000011"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "phone already exists");
}

#[tokio::test]
async fn admin_supplier_status_and_remove_mutations_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let status = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/suppliers/status?ref=SUP-001",
            &token,
            r#"{"blocked":true}"#,
        ))
        .await
        .expect("response");
    assert_eq!(status.status(), StatusCode::OK);
    assert_eq!(json_body(status).await["blocked"], true);

    let remove = build_router(state)
        .oneshot(request(
            "DELETE",
            "/v1/mobile/admin/suppliers/remove?ref=SUP-001",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(remove.status(), StatusCode::OK);
    assert_eq!(json_body(remove).await["ok"], true);
}

#[tokio::test]
async fn admin_item_mutation_errors_match_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let missing = build_router(state.clone())
        .oneshot(request(
            "DELETE",
            "/v1/mobile/admin/customers/items/remove?ref=CUST-001",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing).await["error"],
        "ref and item_code are required"
    );

    let invalid = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/items/bulk-move-group",
            &token,
            r#"{"item_codes":[],"item_group":"Products"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(invalid).await["error"], "item codes are required");
}

#[tokio::test]
async fn admin_item_create_and_werka_regenerate_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let missing_customer = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/items",
            &token,
            r#"{"code":"ITEM-FINISHED","name":"Finished Item","uom":"Kg","item_group":"tayyor mahsulot"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(missing_customer.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_customer).await["error"],
        "customer_ref is required for tayyor mahsulot"
    );

    let item = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/items",
            &token,
            r#"{"code":"ITEM-NEW","name":"New Item","uom":"Kg","item_group":"Products"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(item.status(), StatusCode::OK);
    let value = json_body(item).await;
    assert_eq!(value["code"], "ITEM-NEW");
    assert_eq!(value["item_group"], "Products");

    let finished_item = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/items",
            &token,
            r#"{"code":"ITEM-FINISHED","name":"Finished Item","uom":"Kg","item_group":"tayyor mahsulot","customer_ref":"CUST-001"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(finished_item.status(), StatusCode::OK);
    let value = json_body(finished_item).await;
    assert_eq!(value["code"], "ITEM-FINISHED");
    assert_eq!(value["item_group"], "tayyor mahsulot");

    let settings = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/werka/code/regenerate",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(settings.status(), StatusCode::OK);
    let value = json_body(settings).await;
    assert!(
        value["werka_code"]
            .as_str()
            .expect("code")
            .starts_with("20")
    );
}

fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        default_target_warehouse: "Stores - CH".to_string(),
        http_timeout: std::time::Duration::from_secs(15),
        session_store_path: "data/mobile_sessions.json".into(),
        profile_store_path: "data/mobile_profile_prefs.json".into(),
        push_token_store_path: "data/mobile_push_tokens.json".into(),
        session_ttl_seconds: Some(30 * 24 * 60 * 60),
        supplier_prefix: "10".to_string(),
        werka_prefix: "20".to_string(),
        werka_code: "20ABCDEF1234".to_string(),
        werka_name: "Werka".to_string(),
        werka_phone: "+99888862440".to_string(),
        admin_phone: "+998880000000".to_string(),
        admin_name: "Admin".to_string(),
        admin_code: "19621978".to_string(),
    });
    state.sessions = SessionManager::memory(Some(30 * 24 * 60 * 60));
    let admin_port = Arc::new(FakeAdminReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(admin_port.clone())
        .with_write_port(admin_port)
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    state.production_maps = ProductionMapService::new(Arc::new(MemoryProductionMapStore::new()));
    state.apparatus_groups = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));
    state.workers = WorkerService::new(Arc::new(MemoryWorkerStore::new()));
    state.worker_groups = WorkerGroupService::new(Arc::new(MemoryWorkerGroupStore::new()));
    state.production_orders = Arc::new(NoopMiniOrderSink);
    state
}

async fn session(state: &AppState, role: PrincipalRole) -> String {
    session_for(state, role, "admin").await
}

async fn session_for(state: &AppState, role: PrincipalRole, ref_: &str) -> String {
    state
        .sessions
        .create(Principal {
            role,
            display_name: "Admin".to_string(),
            legal_name: "Admin".to_string(),
            ref_: ref_.to_string(),
            phone: "+998880000000".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

fn request(method: &str, uri: &str, token: &str) -> Request<Body> {
    request_with_body(method, uri, token, "")
}

fn request_with_body(method: &str, uri: &str, token: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

struct FakeAdminReadPort;

struct QueryCaptureReadPort {
    seen_query: Arc<Mutex<String>>,
}

struct LocalPhoneDuplicateReadPort;

#[async_trait]
impl AdminReadPort for QueryCaptureReadPort {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.suppliers_page(query, limit, offset).await
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.supplier_by_ref(ref_).await
    }

    async fn customers_page(
        &self,
        query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        *self.seen_query.lock().await = query.to_string();
        Ok(vec![entry("CUST-QUERY", "Customer Query", "+998904444444")])
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.customer_by_ref(ref_).await
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_page(query, limit, offset).await
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_by_codes(item_codes).await
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        FakeAdminReadPort.item_groups(query, limit).await
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .assigned_supplier_items(supplier_ref, limit)
            .await
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .customer_items(customer_ref, query, limit)
            .await
    }
}

#[async_trait]
impl AdminReadPort for LocalPhoneDuplicateReadPort {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.suppliers_page(query, limit, offset).await
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.supplier_by_ref(ref_).await
    }

    async fn customers_page(
        &self,
        query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        if query == "110000011" {
            Ok(vec![entry("CUST-LOCAL", "Customer Local", "110000011")])
        } else {
            Ok(vec![])
        }
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.customer_by_ref(ref_).await
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_page(query, limit, offset).await
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_by_codes(item_codes).await
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        FakeAdminReadPort.item_groups(query, limit).await
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .assigned_supplier_items(supplier_ref, limit)
            .await
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .customer_items(customer_ref, query, limit)
            .await
    }
}

#[async_trait]
impl AdminReadPort for FakeAdminReadPort {
    async fn suppliers_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        Ok(vec![
            entry("SUP-001", "Supplier One", "+998901111111"),
            entry("SUP-002", "Supplier Two", "+998902222222"),
            entry("SUP-003", "Supplier Removed", "+998903333333"),
        ])
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        Ok(entry(ref_, "Supplier One", "+998901111111"))
    }

    async fn customers_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        Ok(vec![entry("CUST-001", "Customer One", "+998904444444")])
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        Ok(entry(ref_, "Customer One", "+998904444444"))
    }

    async fn items_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(vec![item("ITEM-001")])
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(item_codes.iter().map(|code| item(code)).collect())
    }

    async fn item_groups(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<String>, AdminPortError> {
        Ok(vec![
            "All Item Groups".to_string(),
            "All Item Groups".to_string(),
        ])
    }

    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        _limit: usize,
    ) -> Result<Vec<crate::core::admin::models::AdminWarehouse>, AdminPortError> {
        let warehouses = vec![
            crate::core::admin::models::AdminWarehouse {
                warehouse: "Stores - CH".to_string(),
                company: "Company".to_string(),
                is_group: false,
                parent_warehouse: String::new(),
            },
            crate::core::admin::models::AdminWarehouse {
                warehouse: "Finished Goods - CH".to_string(),
                company: "Company".to_string(),
                is_group: false,
                parent_warehouse: String::new(),
            },
            crate::core::admin::models::AdminWarehouse {
                warehouse: "Godex aparat - CH".to_string(),
                company: "Company".to_string(),
                is_group: false,
                parent_warehouse: "aparat - A".to_string(),
            },
        ];
        let query = query.trim().to_lowercase();
        let parent = parent.trim().to_lowercase();
        Ok(warehouses
            .into_iter()
            .filter(|warehouse| {
                (query.is_empty() || warehouse.warehouse.to_lowercase().contains(&query))
                    && (parent.is_empty() || warehouse.parent_warehouse.to_lowercase() == parent)
            })
            .collect())
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        Ok(vec![
            AdminItemGroup {
                name: "All Item Groups".to_string(),
                item_group_name: "All Item Groups".to_string(),
                parent_item_group: String::new(),
                is_group: true,
            },
            AdminItemGroup {
                name: "Xomashyo".to_string(),
                item_group_name: "Xomashyo".to_string(),
                parent_item_group: "All Item Groups".to_string(),
                is_group: true,
            },
            AdminItemGroup {
                name: "plyonka".to_string(),
                item_group_name: "plyonka".to_string(),
                parent_item_group: "Xomashyo".to_string(),
                is_group: true,
            },
        ])
    }

    async fn assigned_supplier_items(
        &self,
        _supplier_ref: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(vec![item("ITEM-001"), item("ITEM-002")])
    }

    async fn customer_items(
        &self,
        _customer_ref: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(vec![item("ITEM-001")])
    }
}

struct CustomerItemsFailReadPort;

#[async_trait]
impl AdminReadPort for CustomerItemsFailReadPort {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.suppliers_page(query, limit, offset).await
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.supplier_by_ref(ref_).await
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.customers_page(query, limit, offset).await
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.customer_by_ref(ref_).await
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_page(query, limit, offset).await
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_by_codes(item_codes).await
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        FakeAdminReadPort.item_groups(query, limit).await
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .assigned_supplier_items(supplier_ref, limit)
            .await
    }

    async fn customer_items(
        &self,
        _customer_ref: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }
}

struct AssignedItemsErrorReadPort {
    permission: bool,
}

impl AssignedItemsErrorReadPort {
    fn permission() -> Self {
        Self { permission: true }
    }

    fn lookup_failed() -> Self {
        Self { permission: false }
    }
}

#[async_trait]
impl AdminReadPort for AssignedItemsErrorReadPort {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.suppliers_page(query, limit, offset).await
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.supplier_by_ref(ref_).await
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.customers_page(query, limit, offset).await
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.customer_by_ref(ref_).await
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_page(query, limit, offset).await
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_by_codes(item_codes).await
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        FakeAdminReadPort.item_groups(query, limit).await
    }

    async fn assigned_supplier_items(
        &self,
        _supplier_ref: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        if self.permission {
            Err(AdminPortError::PermissionDenied)
        } else {
            Err(AdminPortError::LookupFailed)
        }
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .customer_items(customer_ref, query, limit)
            .await
    }
}

struct MissingItemsReadPort;

#[async_trait]
impl AdminReadPort for MissingItemsReadPort {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.suppliers_page(query, limit, offset).await
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.supplier_by_ref(ref_).await
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.customers_page(query, limit, offset).await
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.customer_by_ref(ref_).await
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_page(query, limit, offset).await
    }

    async fn items_by_codes(
        &self,
        _item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(Vec::new())
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        FakeAdminReadPort.item_groups(query, limit).await
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .assigned_supplier_items(supplier_ref, limit)
            .await
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .customer_items(customer_ref, query, limit)
            .await
    }
}

struct ActivityLookup;

#[async_trait]
impl WerkaHomeLookup for ActivityLookup {
    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok((0..35)
            .map(|index| DispatchRecord {
                id: format!("REC-{index:03}"),
                supplier_name: "Supplier".to_string(),
                item_code: "ITEM-001".to_string(),
                item_name: "Rice".to_string(),
                uom: "Kg".to_string(),
                status: "confirmed".to_string(),
                created_label: "2026-02-08 12:00".to_string(),
                ..DispatchRecord::default()
            })
            .collect())
    }
}

#[async_trait]
impl AdminWritePort for FakeAdminReadPort {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Ok(entry("SUP-NEW", name, phone))
    }

    async fn update_supplier_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn assign_supplier_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn unassign_supplier_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Ok(entry("CUST-NEW", name, phone))
    }

    async fn update_customer_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn update_customer_code(&self, _ref_: &str, _code: &str) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn assign_customer_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn unassign_customer_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        Ok(SupplierItem {
            code: code.to_string(),
            name: name.to_string(),
            uom: uom.to_string(),
            warehouse: "Stores - CH".to_string(),
            item_group: item_group.to_string(),
        })
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        Ok(AdminItemGroup {
            name: name.trim().to_string(),
            item_group_name: name.trim().to_string(),
            parent_item_group: parent.trim().to_string(),
            is_group,
        })
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        Ok(AdminItemGroup {
            name: name.trim().to_string(),
            item_group_name: name.trim().to_string(),
            parent_item_group: parent.trim().to_string(),
            is_group: true,
        })
    }

    async fn update_item_group(
        &self,
        _item_code: &str,
        _item_group: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }
}

struct MissingSupplierWritePort;

#[async_trait]
impl AdminWritePort for MissingSupplierWritePort {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.create_supplier(name, phone).await
    }

    async fn update_supplier_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Err(AdminPortError::NotFound)
    }

    async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .assign_supplier_item(ref_, item_code)
            .await
    }

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .unassign_supplier_item(ref_, item_code)
            .await
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.create_customer(name, phone).await
    }

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        FakeAdminReadPort.update_customer_phone(ref_, phone).await
    }

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError> {
        FakeAdminReadPort.update_customer_code(ref_, code).await
    }

    async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .assign_customer_item(ref_, item_code)
            .await
    }

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .unassign_customer_item(ref_, item_code)
            .await
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        FakeAdminReadPort
            .create_item(code, name, uom, item_group)
            .await
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        FakeAdminReadPort
            .create_item_group(name, parent, is_group)
            .await
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        FakeAdminReadPort.move_item_group_parent(name, parent).await
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .update_item_group(item_code, item_group)
            .await
    }
}

#[derive(Default)]
struct CountingSupplierWritePort {
    supplier_phone_updates: AtomicUsize,
}

#[async_trait]
impl AdminWritePort for CountingSupplierWritePort {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.create_supplier(name, phone).await
    }

    async fn update_supplier_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        self.supplier_phone_updates.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .assign_supplier_item(ref_, item_code)
            .await
    }

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .unassign_supplier_item(ref_, item_code)
            .await
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.create_customer(name, phone).await
    }

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        FakeAdminReadPort.update_customer_phone(ref_, phone).await
    }

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError> {
        FakeAdminReadPort.update_customer_code(ref_, code).await
    }

    async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .assign_customer_item(ref_, item_code)
            .await
    }

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .unassign_customer_item(ref_, item_code)
            .await
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        FakeAdminReadPort
            .create_item(code, name, uom, item_group)
            .await
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        FakeAdminReadPort
            .create_item_group(name, parent, is_group)
            .await
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        FakeAdminReadPort.move_item_group_parent(name, parent).await
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .update_item_group(item_code, item_group)
            .await
    }
}

struct FakeAdminStatePort {
    states: Mutex<BTreeMap<String, AdminState>>,
}

impl FakeAdminStatePort {
    fn new() -> Self {
        Self {
            states: Mutex::new(BTreeMap::from([
                (
                    "SUP-001".to_string(),
                    AdminState {
                        custom_code: "10CUSTOM".to_string(),
                        assigned_item_codes: vec!["ITEM-001".to_string(), "ITEM-002".to_string()],
                        ..AdminState::default()
                    },
                ),
                (
                    "SUP-002".to_string(),
                    AdminState {
                        blocked: true,
                        ..AdminState::default()
                    },
                ),
                (
                    "SUP-003".to_string(),
                    AdminState {
                        removed: true,
                        ..AdminState::default()
                    },
                ),
            ])),
        }
    }
}

#[async_trait]
impl AdminStatePort for FakeAdminStatePort {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        Ok(self.states.lock().await.clone())
    }

    async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError> {
        self.states.lock().await.insert(ref_.to_string(), state);
        Ok(())
    }
}

struct FailingAdminStatePort;

#[async_trait]
impl AdminStatePort for FailingAdminStatePort {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn put_state(&self, _ref_: &str, _state: AdminState) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }
}

struct LockedCustomerStatePort;

#[async_trait]
impl AdminStatePort for LockedCustomerStatePort {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        Ok(BTreeMap::from([(
            "CUST-001".to_string(),
            AdminState {
                custom_code: "30LOCKED".to_string(),
                cooldown_until: Some(time::OffsetDateTime::now_utc() + time::Duration::hours(1)),
                ..AdminState::default()
            },
        )]))
    }

    async fn put_state(&self, _ref_: &str, _state: AdminState) -> Result<(), AdminPortError> {
        Ok(())
    }
}

fn entry(ref_: &str, name: &str, phone: &str) -> AdminDirectoryEntry {
    AdminDirectoryEntry {
        ref_: ref_.to_string(),
        name: name.to_string(),
        phone: phone.to_string(),
    }
}

fn item(code: &str) -> SupplierItem {
    SupplierItem {
        code: code.to_string(),
        name: "Rice".to_string(),
        uom: "Kg".to_string(),
        warehouse: "Stores - CH".to_string(),
        item_group: "Products".to_string(),
    }
}
