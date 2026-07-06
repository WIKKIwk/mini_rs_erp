use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use crate::http::router::build_router;

use super::support::{json_body, test_state};

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
        "/v1/mobile/iroh-ticket",
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
        "/v1/mobile/admin/raw-material-rules",
        "/v1/mobile/admin/raw-material-assignments",
        "/v1/mobile/admin/role-assignments",
        "/v1/mobile/admin/suppliers",
        "/v1/mobile/admin/users/list",
        "/v1/mobile/admin/suppliers/list",
        "/v1/mobile/admin/customers",
        "/v1/mobile/admin/material-taminotchilar",
        "/v1/mobile/admin/material-taminotchilar/detail",
        "/v1/mobile/admin/material-taminotchilar/phone",
        "/v1/mobile/admin/material-taminotchilar/code/regenerate",
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
        "/v1/mobile/admin/apparatus",
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
async fn browser_preview_cors_headers_are_registered() {
    let app = build_router(test_state());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .header(header::ORIGIN, "http://127.0.0.1:61896")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&"*".parse().expect("header value"))
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/healthz")
                .header(header::ORIGIN, "http://127.0.0.1:61896")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&"*".parse().expect("header value"))
    );
}
