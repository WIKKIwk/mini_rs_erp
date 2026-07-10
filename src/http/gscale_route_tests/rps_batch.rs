use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use tower::ServiceExt;

use crate::core::auth::models::PrincipalRole;
use crate::core::gscale::GscaleService;
use crate::http::router::build_router;
use crate::rps::RpsDriverClient;

use super::support::*;

#[tokio::test]
async fn rps_batch_start_state_stop_is_persisted_by_rs() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Werka).await;
    let router = build_router(state);

    let started = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{
                "client_batch_id":"batch-1",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-1",
                "item_name":"Green Tea",
                "warehouse":"Stores - A",
                "printer":"godex",
                "print_mode":"label",
                "quantity_source":"scale",
                "tare_enabled":true,
                "tare_kg":0.78
            }"#,
        ))
        .await
        .expect("start response");
    let started_body = json_body(started).await;

    assert_eq!(started_body["ok"], true);
    assert_eq!(started_body["batch"]["active"], true);
    assert_eq!(started_body["batch"]["id"], "batch-1");
    assert_eq!(started_body["batch"]["item_code"], "ITEM-1");
    assert_eq!(started_body["batch"]["warehouse"], "Stores - A");
    assert_eq!(started_body["batch"]["tare_kg"], 0.78);

    let current = router
        .clone()
        .oneshot(request("GET", "/v1/mobile/rps/batch/state", &token, ""))
        .await
        .expect("state response");
    let current_body = json_body(current).await;

    assert_eq!(current_body["batch"]["active"], true);
    assert_eq!(current_body["batch"]["item_name"], "Green Tea");

    let stopped = router
        .oneshot(request("POST", "/v1/mobile/rps/batch/stop", &token, ""))
        .await
        .expect("stop response");
    let stopped_body = json_body(stopped).await;

    assert_eq!(stopped_body["batch"]["active"], false);
    assert_eq!(stopped_body["batch"]["item_code"], "ITEM-1");
}

#[tokio::test]
async fn rps_batch_print_uses_active_rs_batch_and_transaction_flow() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let receipt_actors = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_receipt_store(Arc::new(FakeReceiptStore {
            events: events.clone(),
            receipt_actors: receipt_actors.clone(),
        }))
        .with_driver(Arc::new(FakeDriver {
            events: events.clone(),
        }));
    let token = session(&state, PrincipalRole::Werka).await;
    let router = build_router(state);

    let _ = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{
                "client_batch_id":"batch-print-1",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-1",
                "item_name":"Green Tea",
                "warehouse":"Stores - A",
                "printer":"zebra",
                "print_mode":"rfid",
                "tare_enabled":true,
                "tare_kg":0.78
            }"#,
        ))
        .await
        .expect("start response");

    let printed = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/print",
            &token,
            r#"{"gross_qty":2.5,"unit":"kg"}"#,
        ))
        .await
        .expect("print response");
    let body = json_body(printed).await;

    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "printed");
    assert_eq!(body["item_code"], "ITEM-1");
    assert_eq!(body["warehouse"], "Stores - A");
    assert_eq!(body["gross_qty"], 2.5);
    assert_eq!(body["qty"], 1.72);
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["print", "create:1.720", "submit:MAT-STE-ROUTE"]
    );
    assert_eq!(
        receipt_actors.lock().unwrap().as_slice(),
        ["werka:admin:Admin"]
    );
}

#[tokio::test]
async fn rps_batch_print_returns_after_driver_without_waiting_for_receipt_submit() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_receipt_store(Arc::new(SlowReceiptStore {
            events: events.clone(),
            delay: Duration::from_millis(800),
        }))
        .with_driver(Arc::new(FakeDriver {
            events: events.clone(),
        }))
        .with_epc_source(Arc::new(FixedEpc("FAST-EPC-1")));
    let token = session(&state, PrincipalRole::Werka).await;
    let router = build_router(state);

    let started = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{
                "client_batch_id":"batch-fast-print-1",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-1",
                "item_name":"Green Tea",
                "warehouse":"Stores - A",
                "printer":"godex",
                "print_mode":"label"
            }"#,
        ))
        .await
        .expect("start response");
    assert_eq!(json_body(started).await["ok"], true);

    let started_at = Instant::now();
    let printed = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/print",
            &token,
            r#"{"gross_qty":2.5,"unit":"kg"}"#,
        ))
        .await
        .expect("print response");
    let elapsed = started_at.elapsed();
    let body = json_body(printed).await;

    assert!(
        elapsed < Duration::from_millis(500),
        "RPS print response took {elapsed:?}"
    );
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "printed");
    assert_eq!(body["epc"], "FAST-EPC-1");
    assert_eq!(body["item_code"], "ITEM-1");
    assert_eq!(body["warehouse"], "Stores - A");
    assert_eq!(events.lock().unwrap().as_slice(), ["print"]);

    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["print", "create:2.500", "submit:MAT-STE-ROUTE"]
    );
}

#[tokio::test]
async fn rps_batch_print_returns_printed_before_late_receipt_store_failure() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_receipt_store(Arc::new(FailingSubmitStore {
            events: events.clone(),
        }))
        .with_driver(Arc::new(FakeDriver {
            events: events.clone(),
        }));
    let token = session(&state, PrincipalRole::Werka).await;
    let router = build_router(state);

    let started = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{
                "client_batch_id":"batch-print-fail-1",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ABCD Family",
                "item_name":"ABCD Family",
                "warehouse":"Stores - A",
                "printer":"godex",
                "print_mode":"label"
            }"#,
        ))
        .await
        .expect("start response");
    assert_eq!(json_body(started).await["ok"], true);

    let printed = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/print",
            &token,
            r#"{"gross_qty":2.5,"unit":"kg"}"#,
        ))
        .await
        .expect("print response");
    let status = printed.status();
    let body = json_body(printed).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "printed");
    assert_eq!(body["item_code"], "ABCD Family");
    assert_eq!(events.lock().unwrap().as_slice(), ["print"]);

    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["print", "create:2.500", "submit:MAT-STE-ROUTE"]
    );

    let state = router
        .oneshot(request("GET", "/v1/mobile/rps/batch/state", &token, ""))
        .await
        .expect("state response");
    let body = json_body(state).await;

    assert_eq!(body["batch"]["active"], true);
    assert_eq!(
        body["batch"]["last_error"],
        "submit failed: NegativeStockError: insufficient stock"
    );
    assert!(
        body["batch"]["last_error_at"]
            .as_str()
            .unwrap_or("")
            .contains('T')
    );
}

#[tokio::test]
async fn live_rps_batch_print_routes_through_rs_to_driver_when_env_is_set() {
    let driver_url = std::env::var("RPS_LIVE_DRIVER_URL").unwrap_or_default();
    if driver_url.trim().is_empty() {
        eprintln!("skipping live RPS driver test; set RPS_LIVE_DRIVER_URL");
        return;
    }

    let events = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_receipt_store(Arc::new(FakeReceiptStore {
            events: events.clone(),
            receipt_actors: Arc::new(Mutex::new(Vec::new())),
        }))
        .with_driver(Arc::new(RpsDriverClient::new(
            Duration::from_secs(15),
            driver_url.clone(),
        )))
        .with_epc_source(Arc::new(FixedEpc("300833B2DDD90140000000A1")));
    let token = session(&state, PrincipalRole::Werka).await;
    let router = build_router(state);

    let started = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            &format!(
                r#"{{
                    "client_batch_id":"live-rps-driver-test",
                    "driver_url":"{}",
                    "item_code":"TEST-GODEX",
                    "item_name":"GoDEX RS Route Test",
                    "warehouse":"5070 Lab",
                    "printer":"godex",
                    "print_mode":"label",
                    "quantity_source":"scale"
                }}"#,
                driver_url.trim().trim_end_matches('/')
            ),
        ))
        .await
        .expect("start response");
    let started_body = json_body(started).await;
    assert_eq!(started_body["ok"], true);

    let printed = router
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/print",
            &token,
            r#"{"gross_qty":2.5,"unit":"kg"}"#,
        ))
        .await
        .expect("print response");
    let status = printed.status();
    let body = json_body(printed).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "printed");
    assert_eq!(body["item_code"], "TEST-GODEX");
    assert_eq!(body["warehouse"], "5070 Lab");
    assert_eq!(body["printer"], "godex");
    assert_eq!(body["print_mode"], "label");
    assert_eq!(body["printer_status"], "sent");
    assert_eq!(body["gross_qty"], 2.5);
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["create:2.500", "submit:MAT-STE-ROUTE"]
    );
}

#[tokio::test]
async fn rps_batch_print_requires_active_batch() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Werka).await;
    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/print",
            &token,
            r#"{"gross_qty":2.5}"#,
        ))
        .await
        .expect("response");
    let body = json_body(response).await;

    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "batch_not_active");
}

#[tokio::test]
async fn rps_batch_start_requires_item_and_warehouse() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Werka).await;
    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{"item_code":"ITEM-1"}"#,
        ))
        .await
        .expect("response");
    let body = json_body(response).await;

    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "invalid_input");
}

#[tokio::test]
async fn material_taminotchi_rps_batch_start_rejects_unassigned_warehouse() {
    let state = test_state();
    let token = session(&state, PrincipalRole::MaterialTaminotchi).await;

    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{
                "client_batch_id":"material-unassigned",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-1",
                "item_name":"Green Tea",
                "warehouse":"Stores - A",
                "printer":"godex",
                "print_mode":"label"
            }"#,
        ))
        .await
        .expect("response");
    let status = response.status();
    let body = json_body(response).await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "warehouse_not_assigned");
}

#[tokio::test]
async fn material_taminotchi_rps_batch_start_allows_assigned_warehouse() {
    let state = test_state();
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
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{
                "client_batch_id":"material-assigned",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-1",
                "item_name":"Green Tea",
                "warehouse":"Stores - A",
                "printer":"godex",
                "print_mode":"label"
            }"#,
        ))
        .await
        .expect("response");
    let status = response.status();
    let body = json_body(response).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["batch"]["warehouse"], "Stores - A");
}

#[tokio::test]
async fn rps_batch_start_requires_driver_url() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Werka).await;
    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{
                "item_code":"ITEM-1",
                "item_name":"Green Tea",
                "warehouse":"Stores - A",
                "printer":"godex",
                "print_mode":"label"
            }"#,
        ))
        .await
        .expect("response");
    let status = response.status();
    let body = json_body(response).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "invalid_input");
    assert!(
        body["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("driver_url_required"),
        "{body}"
    );
}
