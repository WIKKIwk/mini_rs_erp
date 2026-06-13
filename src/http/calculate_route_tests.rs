use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::manager::SessionManager;

#[tokio::test]
async fn calculate_endpoint_returns_formula_result() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/calculate",
            &token,
            r#"{
                "product":"cpp / 20 mikron / 600",
                "kg":300,
                "width_mm":530,
                "first_layer":{"material":"pet","micron":"12"},
                "second_layer":{"material":"pe oq","micron":"30"}
            }"#,
        ))
        .await
        .expect("response");
    let status = response.status();
    let body = json_body(response).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["results"][0]["rounded_length"], 12000.0);
    assert_eq!(body["results"][0]["width_sm"], 53.0);
    assert_eq!(body["rubber_size_mm"], 550);
}

#[tokio::test]
async fn calculate_endpoint_rejects_supplier() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Supplier).await;

    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/calculate",
            &token,
            r#"{"kg":300,"width_mm":530}"#,
        ))
        .await
        .expect("response");
    let body = json_body(response).await;

    assert_eq!(body["error"], "forbidden");
}

#[tokio::test]
async fn calculate_orders_round_trip_on_server_without_kg() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let create = build_router(state.clone())
        .oneshot(request(
            "POST",
            "/v1/mobile/calculate/orders",
            &token,
            r#"{
                "name":"CPP 600",
                "order_number":"ORD-1",
                "customer_ref":"CUST-001",
                "customer":"Mijoz",
                "item_code":"ITEM-001",
                "product":"cpp / 20 mikron / 600",
                "status":"Ready",
                "material_display":"pet 12 / pe oq 30",
                "color":"oq",
                "image_id":"img-test",
                "image_name":"rang.jpg",
                "image_mime":"image/jpeg",
                "image_size_bytes":3,
                "image_url":"/v1/mobile/calculate/orders/image/view?id=img-test",
                "kg":999,
                "width_mm":530,
                "waste_percent":3,
                "roll_count":7,
                "first_layer_material":"pet",
                "first_layer_micron":"12",
                "second_layer_material":"pe oq",
                "second_layer_micron":"30",
                "note":"test"
            }"#,
        ))
        .await
        .expect("create response");
    let create_body = json_body(create).await;
    assert_eq!(create_body["ok"], true);
    assert_eq!(create_body["template"]["name"], "CPP 600");
    assert_eq!(create_body["template"]["kg"], serde_json::json!(999.0));

    let list = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/calculate/orders", &token, ""))
        .await
        .expect("list response");
    let list_body = json_body(list).await;
    assert_eq!(list_body["ok"], true);
    assert_eq!(
        list_body["templates"].as_array().expect("templates").len(),
        1
    );
    assert_eq!(list_body["templates"][0]["waste_percent"], 3.0);
    assert_eq!(list_body["templates"][0]["image_id"], "img-test");
    assert_eq!(list_body["templates"][0]["customer_ref"], "CUST-001");
    assert_eq!(list_body["templates"][0]["item_code"], "ITEM-001");
    assert_eq!(list_body["templates"][0]["kg"], serde_json::json!(999.0));

    let id = list_body["templates"][0]["id"].as_str().expect("id");
    let delete_body = format!(r#"{{"id":"{id}"}}"#);
    let delete = build_router(state.clone())
        .oneshot(request(
            "POST",
            "/v1/mobile/calculate/orders/delete",
            &token,
            &delete_body,
        ))
        .await
        .expect("delete response");
    assert_eq!(json_body(delete).await["ok"], true);

    let empty = build_router(state)
        .oneshot(request("GET", "/v1/mobile/calculate/orders", &token, ""))
        .await
        .expect("empty response");
    assert_eq!(
        json_body(empty).await["templates"]
            .as_array()
            .expect("templates")
            .len(),
        0
    );
}

#[tokio::test]
async fn calculate_order_image_upload_and_view_are_owner_scoped() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let upload = build_router(state.clone())
        .oneshot(image_request(
            "POST",
            "/v1/mobile/calculate/orders/image",
            &token,
            b"fake-jpeg".to_vec(),
        ))
        .await
        .expect("upload response");
    let upload_status = upload.status();
    let upload_body = json_body(upload).await;

    assert_eq!(upload_status, StatusCode::OK, "{upload_body}");
    assert_eq!(upload_body["ok"], true);
    assert_eq!(upload_body["image"]["image_mime"], "image/jpeg");
    assert_eq!(upload_body["image"]["image_size_bytes"], 9);
    let image_url = upload_body["image"]["image_url"].as_str().expect("url");

    let view = build_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(image_url)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("view request"),
        )
        .await
        .expect("view response");
    let status = view.status();
    let content_type = view
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = to_bytes(view.into_body(), usize::MAX)
        .await
        .expect("view body");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "image/jpeg");
    assert_eq!(&bytes[..], b"fake-jpeg");
}

#[tokio::test]
async fn calculate_orders_reject_supplier() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Supplier).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/calculate/orders", &token, ""))
        .await
        .expect("response");
    let body = json_body(response).await;

    assert_eq!(body["error"], "forbidden");
}

fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        default_target_warehouse: String::new(),
        erp_timeout: std::time::Duration::from_secs(15),
        session_store_path: "data/mobile_sessions.json".into(),
        profile_store_path: "data/mobile_profile_prefs.json".into(),
        push_token_store_path: "data/mobile_push_tokens.json".into(),
        admin_supplier_store_path: "data/mobile_admin_suppliers.json".into(),
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

async fn session(state: &AppState, role: PrincipalRole) -> String {
    state
        .sessions
        .create(Principal {
            role,
            display_name: "Admin".to_string(),
            legal_name: "Admin".to_string(),
            ref_: "admin".to_string(),
            phone: "+998880000000".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

fn request(method: &str, uri: &str, token: &str, body: &str) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if !token.trim().is_empty() {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

fn image_request(method: &str, uri: &str, token: &str, body: Vec<u8>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if !token.trim().is_empty() {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder
        .header(header::CONTENT_TYPE, "image/jpeg")
        .header("x-file-name", "rang.jpg")
        .body(Body::from(body))
        .expect("request")
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}
