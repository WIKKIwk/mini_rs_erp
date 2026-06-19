use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use super::router::build_router;
use crate::core::push::service::PushService;
use crate::core::werka::service::WerkaService;

#[path = "werka_customer_issue_support.rs"]
mod werka_customer_issue_support;

use werka_customer_issue_support::*;

#[tokio::test]
async fn customer_issue_create_requires_auth() {
    let response = build_router(test_state())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/customer-issue/create")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn customer_issue_create_rejects_non_post_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/mobile/werka/customer-issue/create")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn customer_issue_create_rejects_invalid_json_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/customer-issue/create")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from("{"))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "invalid json");
}

#[tokio::test]
async fn customer_issue_create_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(create_request(&token, request_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "werka customer issue create failed"
    );
}

#[tokio::test]
async fn customer_issue_create_returns_record_and_source_metadata() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_customer_issue_writer(Arc::new(FakeIssueWriter::ok()));
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(create_request(&token, request_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["entry_id"], "DN-001");
    assert_eq!(value["customer_ref"], "CUST-001");
    assert_eq!(value["item_code"], "ITEM-001");
    assert_eq!(value["uom"], "Kg");
    assert_eq!(value["qty"], 2.0);
}

#[tokio::test]
async fn customer_issue_create_sends_customer_push_like_go() {
    let sender = Arc::new(RecordingPushSender::default());
    let mut state = test_state();
    state.werka = WerkaService::new().with_customer_issue_writer(Arc::new(FakeIssueWriter::ok()));
    state.push = PushService::new(state.push.store_for_tests()).with_sender(sender.clone());
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(create_request(&token, request_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let calls = sender.calls.lock().expect("calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].key, "customer:CUST-001");
    assert_eq!(calls[0].title, "Werka mahsulot jo'natdi");
    assert_eq!(calls[0].body, "ITEM-001 2 Kg jo'natildi");
    assert_eq!(calls[0].data["target_role"], "customer");
    assert_eq!(calls[0].data["target_ref"], "CUST-001");
    assert_eq!(calls[0].data["id"], "DN-001");
}

#[tokio::test]
async fn customer_issue_create_rejects_duplicate_source() {
    let mut state = test_state();
    state.werka =
        WerkaService::new().with_customer_issue_writer(Arc::new(FakeIssueWriter::duplicate()));
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(create_request(&token, request_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let value = json_body(response).await;
    assert_eq!(value["error"], "duplicate customer issue source");
    assert_eq!(value["error_code"], "duplicate_customer_issue_source");
}

#[tokio::test]
async fn customer_issue_create_maps_negative_stock_to_conflict() {
    let mut state = test_state();
    state.werka = WerkaService::new()
        .with_customer_issue_writer(Arc::new(FakeIssueWriter::insufficient_stock()));
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(create_request(&token, request_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let value = json_body(response).await;
    assert_eq!(value["error"], "insufficient stock");
    assert_eq!(value["error_code"], "insufficient_stock");
}

#[tokio::test]
async fn customer_issue_batch_create_rejects_empty_lines_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(batch_request(
            &token,
            r#"{"client_batch_id":"batch-1","lines":[]}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "lines are required");
}

#[tokio::test]
async fn customer_issue_batch_create_rejects_non_post_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/mobile/werka/customer-issue/batch-create")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn customer_issue_batch_create_rejects_invalid_json_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(batch_request(&token, "{"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "invalid json");
}

#[tokio::test]
async fn customer_issue_batch_create_returns_created_lines_like_go() {
    let mut state = test_state();
    state.werka =
        WerkaService::new().with_customer_issue_writer(Arc::new(FakeIssueWriter::batch_ok()));
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(batch_request(
            &token,
            r#"{"client_batch_id":" batch-1 ","lines":[{"customer_ref":"CUST-001","item_code":"ITEM-001","qty":2}]}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["client_batch_id"], "batch-1");
    assert_eq!(value["created"][0]["line_index"], 0);
    assert_eq!(value["created"][0]["record"]["entry_id"], "DN-ITEM-001");
    assert_eq!(value["failed"].as_array().expect("failed").len(), 0);
}

#[tokio::test]
async fn customer_issue_batch_create_sends_push_for_created_lines_like_go() {
    let sender = Arc::new(RecordingPushSender::default());
    let mut state = test_state();
    state.werka =
        WerkaService::new().with_customer_issue_writer(Arc::new(FakeIssueWriter::batch_partial()));
    state.push = PushService::new(state.push.store_for_tests()).with_sender(sender.clone());
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(batch_request(
            &token,
            r#"{"client_batch_id":"batch-2","lines":[{"customer_ref":"CUST-001","item_code":"ITEM-001","qty":2},{"customer_ref":"CUST-001","item_code":"ITEM-FAIL","qty":3}]}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let calls = sender.calls.lock().expect("calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].key, "customer:CUST-001");
    assert_eq!(calls[0].data["id"], "DN-ITEM-001");
    assert_eq!(calls[0].data["item_code"], "ITEM-001");
}

#[tokio::test]
async fn customer_issue_batch_create_keeps_partial_failures_in_body_like_go() {
    let mut state = test_state();
    state.werka =
        WerkaService::new().with_customer_issue_writer(Arc::new(FakeIssueWriter::batch_partial()));
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(batch_request(
            &token,
            r#"{"client_batch_id":"batch-2","lines":[{"customer_ref":"CUST-001","item_code":"ITEM-001","qty":2},{"customer_ref":"CUST-001","item_code":"ITEM-FAIL","qty":3}]}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["created"][0]["line_index"], 0);
    assert_eq!(value["failed"][0]["line_index"], 1);
    assert_eq!(value["failed"][0]["error"], "insufficient stock");
    assert_eq!(value["failed"][0]["error_code"], "insufficient_stock");
}
