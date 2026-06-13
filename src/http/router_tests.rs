use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::manager::SessionManager;
use crate::core::werka::models::{
    DispatchRecord, SupplierDirectoryEntry, WerkaArchiveResponse, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
use crate::core::werka::service::WerkaService;

fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        default_target_warehouse: String::new(),
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
    state
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("json")
}

#[tokio::test]
async fn me_route_matches_go_contract() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/me")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_me_route_is_not_registered() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/auth/me")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn go_mobile_route_inventory_is_registered() {
    const ROUTES: &[&str] = &[
        "/healthz",
        "/v1/mobile/auth/login",
        "/v1/mobile/auth/logout",
        "/v1/mobile/me",
        "/v1/mobile/profile",
        "/v1/mobile/profile/avatar",
        "/v1/mobile/profile/avatar/view",
        "/v1/mobile/calculate/orders/image",
        "/v1/mobile/calculate/orders/image/view",
        "/v1/mobile/push/token",
        "/v1/mobile/gscale/items",
        "/v1/mobile/gscale/material-receipt/print",
        "/v1/mobile/stock-entry/lookup",
        "/v1/mobile/customer/summary",
        "/v1/mobile/customer/history",
        "/v1/mobile/customer/status-details",
        "/v1/mobile/customer/detail",
        "/v1/mobile/customer/respond",
        "/v1/mobile/notifications/detail",
        "/v1/mobile/notifications/comments",
        "/v1/mobile/supplier/unannounced/respond",
        "/v1/mobile/supplier/summary",
        "/v1/mobile/supplier/status-breakdown",
        "/v1/mobile/supplier/status-details",
        "/v1/mobile/supplier/history",
        "/v1/mobile/supplier/items",
        "/v1/mobile/supplier/dispatch",
        "/v1/mobile/werka/summary",
        "/v1/mobile/werka/home",
        "/v1/mobile/werka/customers",
        "/v1/mobile/werka/suppliers",
        "/v1/mobile/werka/ai-search-suggestion",
        "/v1/mobile/werka/supplier-items",
        "/v1/mobile/werka/customer-items",
        "/v1/mobile/werka/customer-item-options",
        "/v1/mobile/werka/customer-issue/create",
        "/v1/mobile/werka/customer-issue/batch-create",
        "/v1/mobile/werka/unannounced/create",
        "/v1/mobile/werka/status-breakdown",
        "/v1/mobile/werka/status-details",
        "/v1/mobile/werka/pending",
        "/v1/mobile/werka/history",
        "/v1/mobile/werka/notifications",
        "/v1/mobile/werka/archive",
        "/v1/mobile/werka/archive/pdf",
        "/v1/mobile/werka/confirm",
        "/v1/mobile/admin/settings",
        "/v1/mobile/admin/capabilities",
        "/v1/mobile/admin/roles",
        "/v1/mobile/admin/production-maps",
        "/v1/mobile/admin/role-assignments",
        "/v1/mobile/admin/suppliers",
        "/v1/mobile/admin/suppliers/list",
        "/v1/mobile/admin/customers",
        "/v1/mobile/admin/customers/list",
        "/v1/mobile/admin/customers/detail",
        "/v1/mobile/admin/customers/phone",
        "/v1/mobile/admin/customers/code/regenerate",
        "/v1/mobile/admin/customers/items/add",
        "/v1/mobile/admin/customers/items/remove",
        "/v1/mobile/admin/customers/remove",
        "/v1/mobile/admin/suppliers/summary",
        "/v1/mobile/admin/suppliers/detail",
        "/v1/mobile/admin/suppliers/inactive",
        "/v1/mobile/admin/suppliers/status",
        "/v1/mobile/admin/suppliers/phone",
        "/v1/mobile/admin/suppliers/items",
        "/v1/mobile/admin/suppliers/items/assigned",
        "/v1/mobile/admin/suppliers/items/add",
        "/v1/mobile/admin/suppliers/items/remove",
        "/v1/mobile/admin/suppliers/code/regenerate",
        "/v1/mobile/admin/suppliers/remove",
        "/v1/mobile/admin/suppliers/restore",
        "/v1/mobile/admin/item-groups",
        "/v1/mobile/admin/items",
        "/v1/mobile/admin/warehouses",
        "/v1/mobile/admin/items/bulk-move-group",
        "/v1/mobile/admin/activity",
        "/v1/mobile/admin/werka/code/regenerate",
    ];

    for route in ROUTES {
        let response = build_router(test_state())
            .oneshot(
                Request::builder()
                    .uri(*route)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_ne!(response.status(), StatusCode::NOT_FOUND, "{route}");
    }
}

#[tokio::test]
async fn healthz_accepts_any_method_like_go() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/healthz")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["ok"], true);
}

#[tokio::test]
async fn auth_login_rejects_non_post_with_json_like_go() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/mobile/auth/login")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn auth_login_rejects_werka_code_with_wrong_phone() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"phone":"+998880000000","code":"20ABCDEF1234"}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_body(response).await["error"], "invalid credentials");
}

#[tokio::test]
async fn auth_logout_rejects_non_post_with_json_like_go() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/mobile/auth/logout")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn me_accepts_post_like_go() {
    let state = test_state();
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Supplier,
            display_name: "Supplier".to_string(),
            legal_name: "Supplier".to_string(),
            ref_: "SUP-001".to_string(),
            phone: "+998901234567".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session");
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/me")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["role"], "supplier");
    assert_eq!(value["ref"], "SUP-001");
}

#[tokio::test]
async fn werka_home_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/home")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn werka_home_forbids_non_werka() {
    let state = test_state();
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Supplier,
            display_name: "Supplier".to_string(),
            legal_name: "Supplier".to_string(),
            ref_: "SUP-001".to_string(),
            phone: "+998901234567".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session");
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/home")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn werka_home_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/home")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn werka_home_returns_provider_payload() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/home")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(value["summary"]["pending_count"], 2);
    assert_eq!(value["summary"]["confirmed_count"], 3);
    assert_eq!(value["summary"]["returned_count"], 1);
    assert_eq!(value["pending_items"], serde_json::json!([]));
}

#[tokio::test]
async fn werka_home_accepts_post_like_go_handler() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/home")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn werka_summary_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/summary")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn werka_summary_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/summary")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn werka_summary_returns_provider_payload() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/summary")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(
        value,
        serde_json::json!({
            "pending_count": 2,
            "confirmed_count": 3,
            "returned_count": 1
        })
    );
}

#[tokio::test]
async fn werka_summary_accepts_post_like_go_handler() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/summary")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn werka_pending_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/pending")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn werka_pending_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/pending")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn werka_pending_returns_provider_payload() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/pending")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(value[0]["id"], "PR-001");
    assert_eq!(value[0]["status"], "pending");
}

#[tokio::test]
async fn werka_pending_accepts_post_like_go_handler() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/pending")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

async fn werka_session(state: &AppState) -> String {
    state
        .sessions
        .create(Principal {
            role: PrincipalRole::Werka,
            display_name: "Werka".to_string(),
            legal_name: "Werka".to_string(),
            ref_: "werka".to_string(),
            phone: "+99888862440".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

struct FakeWerkaHomeLookup;

#[async_trait]
impl WerkaHomeLookup for FakeWerkaHomeLookup {
    async fn werka_summary(&self) -> Result<WerkaHomeSummary, WerkaPortError> {
        Ok(WerkaHomeSummary {
            pending_count: 2,
            confirmed_count: 3,
            returned_count: 1,
        })
    }

    async fn werka_home(&self, pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError> {
        assert_eq!(pending_limit, 20);
        Ok(WerkaHomeData {
            summary: WerkaHomeSummary {
                pending_count: 2,
                confirmed_count: 3,
                returned_count: 1,
            },
            pending_items: Vec::new(),
        })
    }

    async fn werka_pending(&self, limit: usize) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        assert_eq!(limit, 0);
        Ok(vec![DispatchRecord {
            id: "PR-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty: 0.0,
            status: "pending".to_string(),
            created_label: "2026-01-16".to_string(),
            ..DispatchRecord::default()
        }])
    }

    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(Vec::new())
    }

    async fn werka_status_breakdown(
        &self,
        _kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError> {
        Ok(Vec::new())
    }

    async fn werka_status_details(
        &self,
        _kind: &str,
        _supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(Vec::new())
    }

    async fn werka_archive(
        &self,
        _kind: &str,
        _period: &str,
        _from: Option<time::Date>,
        _to: Option<time::Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        Ok(WerkaArchiveResponse::default())
    }

    async fn werka_suppliers(
        &self,
        _: &str,
        _: usize,
        _: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, WerkaPortError> {
        Ok(Vec::new())
    }
}
