#![cfg(feature = "verification")]

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use mini_rs_erp::{
    app::AppState,
    config::AppConfig,
    core::{
        auth::models::{Principal, PrincipalRole},
        authz::RoleAssignmentUpsert,
        production_map::*,
        session::manager::SessionManager,
    },
    http::router::build_router,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;

const PRINT: &str = "apparatus:default:bosma_9";
const LAMINATION: &str = "apparatus:default:asset-007";
const ORDER: &str = "zakaz-frozen-worker";
const TITLE: &str = "Day poroshok 3 kg avtomat limon";
const QR: &str = "400118DBFB0582697630F305";

#[tokio::test]
async fn frozen_worker_qr_returns_order_title_before_routing_and_keeps_history_readable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path();
    let mut state = AppState::verification(AppConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        default_target_warehouse: String::new(),
        http_timeout: Duration::from_secs(2),
        session_store_path: path.join("sessions.json"),
        profile_store_path: path.join("profiles.json"),
        push_token_store_path: path.join("push.json"),
        session_ttl_seconds: Some(3600),
        supplier_prefix: "10".into(),
        werka_prefix: "20".into(),
        werka_code: String::new(),
        werka_name: String::new(),
        werka_phone: String::new(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: String::new(),
        material_taminotchi_phone: String::new(),
        admin_phone: String::new(),
        admin_name: String::new(),
        admin_code: String::new(),
    });
    state.apparatus.bootstrap_factory_defaults().await.unwrap();
    state.sessions = SessionManager::memory(Some(3600));
    let store = Arc::new(MemoryProductionMapStore::new());
    state.production_maps = ProductionMapService::new(
        store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(
            state.apparatus.clone(),
        )),
    );
    state
        .production_maps
        .upsert_map(
            serde_json::from_value(json!({
                "id": ORDER, "title": TITLE, "product_code": "DAY-3KG",
                "nodes": [
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"print","kind":"apparatus","title":"Bosma","apparatus_id":PRINT},
                    {"id":"lam","kind":"apparatus","title":"Laminatsiya","apparatus_id":LAMINATION},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges": [{"from":"start","to":"print"},
                    {"from":"print","to":"lam"},{"from":"lam","to":"end"}]
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    store.put_order_progress_batch(serde_json::from_value(json!({
        "batch_id":"frozen-output", "session_id":"print-session",
        "started_at_unix":1,"completed_at_unix":2,
        "apparatus":PRINT,"order_id":ORDER,"action":"detach_roll",
        "status":"roll_detached","produced_qty":464,"uom":"m","qr_payload":QR,
        "label_item_code":"DAY-3KG",
        "label_item_name":format!("{TITLE} yarim tayyor mahsulot, apparat: {PRINT}, rulon yechildi"),
        "executor_name":"Xurshid","worker_role":"aparatchi","worker_ref":"print-worker",
        "worker_display_name":"Xurshid","wip_status":"waiting",
        "next_apparatus":LAMINATION,
        // An unresolved historical route must not obscure the global freeze.
        "payload_json":{"production_stage_node_id":"removed-node","next_stage_node_id":"removed-target"}
    })).unwrap()).await.unwrap();
    state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "lam-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![LAMINATION.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Aparatchi,
            ref_: "lam-worker".into(),
            display_name: "Worker".into(),
            legal_name: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .unwrap();
    let router = build_router(state.clone());
    for (control_state, error_code) in [
        (OrderControlState::Frozen, "order_frozen"),
        (OrderControlState::FreezeRequested, "order_freeze_requested"),
    ] {
        let mut control = OrderControlRecord::active(ORDER);
        control.state = control_state;
        store.put_order_control_state(control).await.unwrap();
        for scoped in [false, true] {
            let mut body = json!({"qr_payload":QR,"require_active_order":true});
            if scoped {
                body["apparatus"] = LAMINATION.into();
                body["order_id"] = ORDER.into();
            }
            let request = Request::builder()
                .method("POST")
                .uri("/v1/mobile/admin/production-maps/progress-qr/lookup")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap();
            let response = router.clone().oneshot(request).await.unwrap();
            let status = response.status();
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                    .unwrap();
            assert_eq!(
                status,
                StatusCode::CONFLICT,
                "scoped={scoped}, state={control_state:?}: {body}"
            );
            assert_eq!(body["error"], error_code);
            assert_eq!(body["order_title"], TITLE);
            assert!(body.get("batch").is_none());
        }
    }
    // Reporting remains possible, and the lookup has not changed production state.
    for control_state in [OrderControlState::Frozen, OrderControlState::Active] {
        let mut control = OrderControlRecord::active(ORDER);
        control.state = control_state;
        store.put_order_control_state(control).await.unwrap();
        let request = Request::builder()
            .method("POST")
            .uri("/v1/mobile/admin/production-maps/progress-qr/lookup")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"qr_payload":QR,
                "require_active_order":control_state==OrderControlState::Active})
                .to_string(),
            ))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(body["batch"]["qr_payload"], QR);
        assert_eq!(body["batch"]["wip_status"], "waiting");
        assert_eq!(
            state
                .production_maps
                .order_control_state(ORDER)
                .await
                .unwrap()
                .state,
            control_state
        );
    }
}
