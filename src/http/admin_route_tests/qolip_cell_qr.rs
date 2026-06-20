use super::*;

#[tokio::test]
async fn qolip_cell_qr_print_reuses_same_payload_for_same_cell() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests.clone(),
        fail: false,
    }));
    let token = session(&state, PrincipalRole::Admin).await;

    let body = r#"{
        "warehouse":"Qolip ombor",
        "block":"A",
        "row_letter":"B",
        "column_number":7,
        "driver_url":"http://127.0.0.1:39117",
        "printer":"zebra",
        "print_mode":"rfid"
    }"#;

    let first = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/cell-qr/print",
            &token,
            body,
        ))
        .await
        .expect("first print");
    assert_eq!(first.status(), StatusCode::OK);
    let first_body = json_body(first).await;
    let first_qr = first_body["cell_qr"]["qr_payload"]
        .as_str()
        .expect("first qr")
        .to_string();
    assert!(first_qr.starts_with("4002"), "{first_qr}");

    let second = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/cell-qr/print",
            &token,
            body,
        ))
        .await
        .expect("second print");
    assert_eq!(second.status(), StatusCode::OK);
    let second_body = json_body(second).await;
    assert_eq!(second_body["cell_qr"]["qr_payload"], first_qr);

    let printed = print_requests.lock().await;
    assert_eq!(printed.len(), 2);
    assert_eq!(printed[0].epc, first_qr);
    assert_eq!(printed[1].epc, first_qr);
    assert_eq!(printed[0].label_kind, "progress");
    assert_eq!(printed[0].item_name, "Qolip yachayka B7");
}
