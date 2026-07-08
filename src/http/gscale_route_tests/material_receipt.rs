use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::StatusCode;
use tower::ServiceExt;

use crate::core::admin::service::AdminService;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::authz::RoleAssignmentUpsert;
use crate::core::gscale::GscaleService;
use crate::http::router::build_router;

use super::support::*;

#[tokio::test]
async fn material_receipt_print_requires_auth() {
    let response = build_router(test_state())
        .oneshot(request(
            "POST",
            "/v1/mobile/gscale/material-receipt/print",
            "",
            "{}",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_body(response).await["error"], "unauthorized");
}

#[tokio::test]
async fn material_receipt_print_rejects_wrong_method() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/gscale/material-receipt/print",
            &token,
            "",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method_not_allowed");
}

#[tokio::test]
async fn material_receipt_print_uses_parallel_driver_first_flow() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_receipt_store(Arc::new(FakeReceiptStore {
            events: events.clone(),
        }))
        .with_driver(Arc::new(FakeDriver {
            events: events.clone(),
        }));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/gscale/material-receipt/print",
            &token,
            r#"{
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-1",
                "item_name":"Green Tea",
                "warehouse":"Stores - A",
                "printer":"zebra",
                "print_mode":"rfid",
                "gross_qty":2.5,
                "tare_enabled":true,
                "tare_kg":0.78
            }"#,
        ))
        .await
        .expect("response");
    let body = json_body(response).await;

    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "printed");
    assert_eq!(body["draft_name"], "");
    assert_eq!(body["qty"], 1.72);
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["print", "create:1.720", "submit:MAT-STE-ROUTE"]
    );
}

#[tokio::test]
async fn material_taminotchi_can_print_material_receipt() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_receipt_store(Arc::new(FakeReceiptStore {
            events: events.clone(),
        }))
        .with_driver(Arc::new(FakeDriver {
            events: events.clone(),
        }));
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "admin",
        "Stores - A",
    )
    .await;
    let token = session(&state, PrincipalRole::MaterialTaminotchi).await;

    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/gscale/material-receipt/print",
            &token,
            r#"{
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-1",
                "item_name":"Green Tea",
                "warehouse":"Stores - A",
                "printer":"zebra",
                "print_mode":"rfid",
                "gross_qty":2.5,
                "tare_enabled":true,
                "tare_kg":0.78
            }"#,
        ))
        .await
        .expect("response");
    let body = json_body(response).await;

    assert_eq!(body["ok"], true);
    assert_eq!(body["qty"], 1.72);
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["print", "create:1.720", "submit:MAT-STE-ROUTE"]
    );
}

#[tokio::test]
async fn material_taminotchi_receipt_print_rejects_unassigned_warehouse() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_receipt_store(Arc::new(FakeReceiptStore {
            events: events.clone(),
        }))
        .with_driver(Arc::new(FakeDriver { events }));
    let token = session(&state, PrincipalRole::MaterialTaminotchi).await;

    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/gscale/material-receipt/print",
            &token,
            r#"{
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-1",
                "item_name":"Green Tea",
                "warehouse":"Stores - A",
                "printer":"zebra",
                "print_mode":"rfid",
                "gross_qty":2.5
            }"#,
        ))
        .await
        .expect("response");
    let status = response.status();
    let body = json_body(response).await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "warehouse_not_assigned");
}

#[tokio::test]
async fn gscale_items_use_admin_catalog_without_customer_scope() {
    let mut state = test_state();
    state.admin =
        AdminService::new(&state.config).with_read_port(Arc::new(FakeAdminCatalogReadPort));
    state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "admin".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec!["Products".to_string()],
        })
        .await
        .expect("material catalog scope");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let werka_token = session(&state, PrincipalRole::Werka).await;
    let material_token = session(&state, PrincipalRole::MaterialTaminotchi).await;
    let supplier_token = session(&state, PrincipalRole::Supplier).await;
    let router = build_router(state);

    for token in [&admin_token, &werka_token, &material_token] {
        let response = router
            .clone()
            .oneshot(request(
                "GET",
                "/v1/mobile/gscale/items?q=film&group=Products&limit=20",
                token,
                "",
            ))
            .await
            .expect("response");
        let status = response.status();
        let body = json_body(response).await;

        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body.as_array().expect("array").len(), 1);
        assert_eq!(body[0]["code"], "GSCALE-ITEM-001");
        assert_eq!(body[0]["item_group"], "Products");
    }

    let forbidden = router
        .oneshot(request(
            "GET",
            "/v1/mobile/gscale/items",
            &supplier_token,
            "",
        ))
        .await
        .expect("response");
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(forbidden).await["error"], "forbidden");
}

#[tokio::test]
async fn material_taminotchi_gscale_items_are_limited_to_assigned_item_groups() {
    let mut state = test_state();
    state.admin =
        AdminService::new(&state.config).with_read_port(Arc::new(FakeAdminCatalogReadPort));
    state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-scope".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec!["Kraska".to_string()],
        })
        .await
        .expect("material scope");
    let material_token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::MaterialTaminotchi,
            display_name: "Materialchi".to_string(),
            legal_name: "Materialchi".to_string(),
            ref_: "material-scope".to_string(),
            phone: "+998901006060".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session");
    let router = build_router(state);

    let response = router
        .oneshot(request(
            "GET",
            "/v1/mobile/gscale/items?limit=20",
            &material_token,
            "",
        ))
        .await
        .expect("response");
    let status = response.status();
    let body = json_body(response).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body.as_array().expect("items");
    assert_eq!(items.len(), 1, "{body}");
    assert_eq!(items[0]["code"], "INK-BLACK");
    assert_eq!(items[0]["item_group"], "Kraska");
}

#[tokio::test]
async fn material_taminotchi_gscale_items_include_child_groups_from_assigned_parent() {
    let mut state = test_state();
    state.admin =
        AdminService::new(&state.config).with_read_port(Arc::new(FakeAdminCatalogReadPort));
    state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-rulon-scope".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec!["Rulon".to_string()],
        })
        .await
        .expect("material scope");
    let material_token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::MaterialTaminotchi,
            display_name: "Materialchi".to_string(),
            legal_name: "Materialchi".to_string(),
            ref_: "material-rulon-scope".to_string(),
            phone: "+998901006060".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session");
    let router = build_router(state);

    let response = router
        .oneshot(request(
            "GET",
            "/v1/mobile/gscale/items?limit=20",
            &material_token,
            "",
        ))
        .await
        .expect("response");
    let status = response.status();
    let body = json_body(response).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body.as_array().expect("items");
    assert_eq!(items.len(), 1, "{body}");
    assert_eq!(items[0]["code"], "ROLL-1000");
    assert_eq!(items[0]["item_group"], "Rulon eni");
}
