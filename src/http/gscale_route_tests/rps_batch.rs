use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use tower::ServiceExt;

use crate::core::admin::service::AdminService;
use crate::core::auth::models::PrincipalRole;
use crate::core::authz::RoleAssignmentUpsert;
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
    assert_eq!(started_body["batch"]["revision"], 1);
    let batch_code = started_body["batch"]["batch_code"]
        .as_str()
        .expect("batch code")
        .to_string();
    assert_eq!(batch_code.len(), 24);
    assert!(batch_code.starts_with("42"));
    assert!(
        batch_code
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
    );
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
    assert_eq!(current_body["batch"]["batch_code"], batch_code);
    assert_eq!(current_body["batch"]["item_name"], "Green Tea");

    let stopped = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/stop",
            &token,
            r#"{"batch_id":"batch-1","expected_revision":1}"#,
        ))
        .await
        .expect("stop response");
    let stopped_body = json_body(stopped).await;

    assert_eq!(stopped_body["batch"]["active"], false);
    assert_eq!(stopped_body["batch"]["revision"], 2);
    assert_eq!(stopped_body["batch"]["batch_code"], batch_code);
    assert_eq!(stopped_body["batch"]["item_code"], "ITEM-1");

    let stopped_again = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/stop",
            &token,
            r#"{"batch_id":"batch-1","expected_revision":1}"#,
        ))
        .await
        .expect("idempotent stop response");
    assert_eq!(stopped_again.status(), StatusCode::OK);
    assert_eq!(json_body(stopped_again).await["batch"]["revision"], 2);

    let next = router
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{
                "client_batch_id":"batch-2",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-2",
                "warehouse":"Stores - B"
            }"#,
        ))
        .await
        .expect("next start response");
    let next_body = json_body(next).await;
    assert_eq!(next_body["batch"]["active"], true);
    assert_eq!(next_body["batch"]["id"], "batch-2");
}

#[tokio::test]
async fn rps_batch_rejects_stale_print_context_before_any_side_effect() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_receipt_store(Arc::new(FakeReceiptStore {
            events: events.clone(),
            receipt_actors: Arc::new(Mutex::new(Vec::new())),
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
                "client_batch_id":"batch-context-1",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-1",
                "warehouse":"Stores - A"
            }"#,
        ))
        .await
        .expect("start response");
    assert_eq!(started.status(), StatusCode::OK);

    let legacy_print = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/print",
            &token,
            r#"{"gross_qty":10,"unit":"kg"}"#,
        ))
        .await
        .expect("legacy request response");
    let legacy_status = legacy_print.status();
    let legacy_body = json_body(legacy_print).await;
    assert_eq!(legacy_status, StatusCode::BAD_REQUEST);
    assert_eq!(legacy_body["error"], "invalid_input");
    assert!(events.lock().unwrap().is_empty());

    let printed = router
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/print",
            &token,
            r#"{"batch_id":"batch-context-1","expected_revision":1,"expected_item_code":"ITEM-2","expected_warehouse":"Stores - A","gross_qty":10,"unit":"kg"}"#,
        ))
        .await
        .expect("conflict response");
    let status = printed.status();
    let body = json_body(printed).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "batch_context_conflict");
    assert!(events.lock().unwrap().is_empty());
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
            r#"{"batch_id":"batch-print-1","expected_revision":1,"expected_item_code":"ITEM-1","expected_warehouse":"Stores - A","gross_qty":2.5,"unit":"kg"}"#,
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

    let current = router
        .clone()
        .oneshot(request("GET", "/v1/mobile/rps/batch/state", &token, ""))
        .await
        .expect("state response");
    let current_body = json_body(current).await;
    let prints = current_body["batch"]["prints"]
        .as_array()
        .expect("batch prints");

    assert_eq!(prints.len(), 1);
    assert_eq!(prints[0]["status"], "printed");
    assert_eq!(prints[0]["gross_qty"], 2.5);
    assert!(!prints[0]["epc"].as_str().unwrap_or_default().is_empty());

    let stopped = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/stop",
            &token,
            r#"{"batch_id":"batch-print-1","expected_revision":1}"#,
        ))
        .await
        .expect("stop response");
    let stopped_body = json_body(stopped).await;
    assert_eq!(stopped_body["batch"]["active"], false);
    assert_eq!(
        stopped_body["batch"]["prints"].as_array().map(Vec::len),
        Some(1)
    );

    let history = router
        .oneshot(request(
            "GET",
            "/v1/mobile/rps/batch/history?limit=50",
            &token,
            "",
        ))
        .await
        .expect("history response");
    assert_eq!(history.status(), StatusCode::OK);
    let history_body = json_body(history).await;
    assert_eq!(history_body["batches"].as_array().map(Vec::len), Some(1));
    assert_eq!(history_body["batches"][0]["id"], "batch-print-1");
    assert_eq!(
        history_body["batches"][0]["batch_code"],
        stopped_body["batch"]["batch_code"]
    );
    assert_eq!(
        history_body["batches"][0]["prints"][0]["epc"],
        stopped_body["batch"]["prints"][0]["epc"]
    );
}

#[tokio::test]
async fn rps_batch_duplicate_count_records_distinct_products_and_epcs() {
    let events = Arc::new(Mutex::new(Vec::new()));
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
        .expect("material item scope");
    state.gscale = GscaleService::new()
        .with_receipt_store(Arc::new(FakeReceiptStore {
            events: events.clone(),
            receipt_actors: Arc::new(Mutex::new(Vec::new())),
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
    let router = build_router(state);

    let started = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{
                "client_batch_id":"batch-unique-products",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"GSCALE-ITEM-001",
                "item_name":"GScale Film",
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
            r#"{"batch_id":"batch-unique-products","expected_revision":1,"expected_item_code":"GSCALE-ITEM-001","expected_warehouse":"Stores - A","gross_qty":2.5,"unit":"kg","print_count":3}"#,
        ))
        .await
        .expect("print response");
    let printed_body = json_body(printed).await;
    assert_eq!(printed_body["ok"], true);
    assert_eq!(printed_body["print_count"], 3);

    tokio::time::sleep(Duration::from_millis(25)).await;
    let current = router
        .oneshot(request("GET", "/v1/mobile/rps/batch/state", &token, ""))
        .await
        .expect("state response");
    let current_body = json_body(current).await;
    let prints = current_body["batch"]["prints"]
        .as_array()
        .expect("batch prints");
    let epcs = prints
        .iter()
        .filter_map(|entry| entry["epc"].as_str())
        .collect::<HashSet<_>>();

    assert_eq!(prints.len(), 3);
    assert_eq!(epcs.len(), 3);
    assert!(prints.iter().all(|entry| entry["print_count"] == 1));
    assert_eq!(
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.as_str() == "print")
            .count(),
        3
    );
}

#[tokio::test]
async fn rps_batch_client_print_prepares_then_confirms_without_driver() {
    const EPC: &str = "303132333435363738394142";
    let events = Arc::new(Mutex::new(Vec::new()));
    let receipt_actors = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_receipt_store(Arc::new(FakeReceiptStore {
            events: events.clone(),
            receipt_actors: receipt_actors.clone(),
        }))
        .with_epc_source(Arc::new(FixedEpc(EPC)));
    let token = session(&state, PrincipalRole::Werka).await;
    let router = build_router(state);

    let started = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{
                "client_batch_id":"batch-client-print-1",
                "driver_url":"usb://local",
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

    let prepared = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/client-print/prepare",
            &token,
            r#"{"batch_id":"batch-client-print-1","expected_revision":1,"expected_item_code":"ITEM-1","expected_warehouse":"Stores - A","gross_qty":2.5,"unit":"kg"}"#,
        ))
        .await
        .expect("prepare response");
    let prepared_body = json_body(prepared).await;

    assert_eq!(prepared_body["status"], "prepared");
    assert_eq!(prepared_body["epc"], EPC);
    assert!(events.lock().unwrap().is_empty());

    let preparing_state = router
        .clone()
        .oneshot(request("GET", "/v1/mobile/rps/batch/state", &token, ""))
        .await
        .expect("state response");
    assert_eq!(
        json_body(preparing_state).await["batch"]["prints"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );

    let confirmed = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/client-print/confirm",
            &token,
            &format!(r#"{{"batch_id":"batch-client-print-1","expected_revision":1,"expected_item_code":"ITEM-1","expected_warehouse":"Stores - A","gross_qty":2.5,"unit":"kg","epc":"{EPC}"}}"#),
        ))
        .await
        .expect("confirm response");
    let confirmed_body = json_body(confirmed).await;

    assert_eq!(confirmed_body["status"], "printed");
    assert_eq!(confirmed_body["draft_name"], "MAT-STE-ROUTE");
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["create:2.500", "submit:MAT-STE-ROUTE"]
    );
    assert_eq!(
        receipt_actors.lock().unwrap().as_slice(),
        ["werka:admin:Admin"]
    );

    let confirmed_state = router
        .oneshot(request("GET", "/v1/mobile/rps/batch/state", &token, ""))
        .await
        .expect("state response");
    let confirmed_body = json_body(confirmed_state).await;
    assert_eq!(confirmed_body["batch"]["prints"][0]["epc"], EPC);
}

#[tokio::test]
async fn rps_batch_start_rejects_overwriting_active_cycle() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Werka).await;
    let router = build_router(state);
    let body = r#"{
        "client_batch_id":"batch-protected",
        "driver_url":"http://127.0.0.1:39117",
        "item_code":"ITEM-1",
        "warehouse":"Stores - A"
    }"#;

    let first = router
        .clone()
        .oneshot(request("POST", "/v1/mobile/rps/batch/start", &token, body))
        .await
        .expect("first start response");
    assert_eq!(first.status(), StatusCode::OK);

    let second = router
        .oneshot(request("POST", "/v1/mobile/rps/batch/start", &token, body))
        .await
        .expect("second start response");
    let status = second.status();
    let second_body = json_body(second).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(second_body["error"], "batch_already_active");
}

#[tokio::test]
async fn rps_batch_print_waits_for_receipt_submit_before_success() {
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
            r#"{"batch_id":"batch-fast-print-1","expected_revision":1,"expected_item_code":"ITEM-1","expected_warehouse":"Stores - A","gross_qty":2.5,"unit":"kg"}"#,
        ))
        .await
        .expect("print response");
    let elapsed = started_at.elapsed();
    let body = json_body(printed).await;

    assert!(
        elapsed >= Duration::from_millis(750),
        "RPS print returned early: {elapsed:?}"
    );
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "printed");
    assert_eq!(body["epc"], "FAST-EPC-1");
    assert_eq!(body["item_code"], "ITEM-1");
    assert_eq!(body["warehouse"], "Stores - A");
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["print", "create:2.500", "submit:MAT-STE-ROUTE"]
    );
}

#[tokio::test]
async fn rps_batch_stop_waits_for_in_flight_print_and_then_closes_exact_batch() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_receipt_store(Arc::new(SlowReceiptStore {
            events: events.clone(),
            delay: Duration::from_millis(500),
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
                "client_batch_id":"batch-stop-during-print",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-1",
                "warehouse":"Stores - A"
            }"#,
        ))
        .await
        .expect("start response");
    assert_eq!(started.status(), StatusCode::OK);

    let print_router = router.clone();
    let print_token = token.clone();
    let print_task = tokio::spawn(async move {
        print_router
            .oneshot(request(
                "POST",
                "/v1/mobile/rps/batch/print",
                &print_token,
                r#"{"batch_id":"batch-stop-during-print","expected_revision":1,"expected_item_code":"ITEM-1","expected_warehouse":"Stores - A","gross_qty":2.5,"unit":"kg"}"#,
            ))
            .await
            .expect("print response")
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let stop_started_at = Instant::now();
    let stopped = router
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/stop",
            &token,
            r#"{"batch_id":"batch-stop-during-print","expected_revision":1}"#,
        ))
        .await
        .expect("stop response");
    let stop_elapsed = stop_started_at.elapsed();
    let printed = print_task.await.expect("print task");

    assert_eq!(printed.status(), StatusCode::OK);
    assert!(stop_elapsed >= Duration::from_millis(400));
    let stopped_body = json_body(stopped).await;
    assert_eq!(stopped_body["batch"]["active"], false);
    assert_eq!(stopped_body["batch"]["revision"], 2);
    assert_eq!(
        stopped_body["batch"]["prints"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["print", "create:2.500", "submit:MAT-STE-ROUTE"]
    );
}

#[tokio::test]
async fn rps_batch_print_failure_is_returned_and_stops_the_batch() {
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
            r#"{"batch_id":"batch-print-fail-1","expected_revision":1,"expected_item_code":"ABCD Family","expected_warehouse":"Stores - A","gross_qty":2.5,"unit":"kg"}"#,
        ))
        .await
        .expect("print response");
    let status = printed.status();
    let body = json_body(printed).await;

    assert_eq!(status, StatusCode::FAILED_DEPENDENCY);
    assert_eq!(body["ok"], false);
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["print", "create:2.500", "submit:MAT-STE-ROUTE"]
    );

    let state = router
        .oneshot(request("GET", "/v1/mobile/rps/batch/state", &token, ""))
        .await
        .expect("state response");
    let body = json_body(state).await;

    assert_eq!(body["batch"]["active"], false);
    assert_eq!(body["batch"]["revision"], 2);
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
            r#"{"batch_id":"live-rps-driver-test","expected_revision":1,"expected_item_code":"TEST-GODEX","expected_warehouse":"5070 Lab","gross_qty":2.5,"unit":"kg"}"#,
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
            assigned_item_groups: vec!["Rulon".to_string()],
        })
        .await
        .expect("material item scope");
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
                "item_code":"ROLL-1000",
                "item_name":"client supplied name",
                "warehouse":"Stores - A",
                "printer":"godex",
                "print_mode":"label",
                "width_mm":615,
                "micron":13
            }"#,
        ))
        .await
        .expect("response");
    let status = response.status();
    let body = json_body(response).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["batch"]["warehouse"], "Stores - A");
    assert_eq!(body["batch"]["item_code"], "ROLL-1000");
    assert_eq!(body["batch"]["item_name"], "CPP 1000/35");
    assert_eq!(body["batch"]["width_mm"], 615.0);
    assert_eq!(body["batch"]["micron"], 13.0);
}

#[tokio::test]
async fn material_taminotchi_rps_batch_start_requires_roll_dimensions() {
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
            assigned_item_groups: vec!["Rulon".to_string()],
        })
        .await
        .expect("material item scope");
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
                "client_batch_id":"material-missing-dimensions",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ROLL-1000",
                "item_name":"CPP",
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
    assert_eq!(body["error"], "material_dimensions_required");
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
