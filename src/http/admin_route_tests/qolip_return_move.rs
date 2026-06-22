use super::*;

#[tokio::test]
async fn qolip_return_restores_stock_and_move_changes_cell() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let _ = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"id":"return_worker_1","name":"Return worker","phone":"+998901112278","level":"Master"}"#,
        ))
        .await
        .expect("create worker");

    let unplaced = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/locations",
            &token,
            r#"{
                "block":"A",
                "warehouse":"Qolip ombor",
                "item_code":"ITEM-2",
                "item_name":"Move qolip",
                "qolip_code":"Q-200",
                "size":40,
                "quantity":6
            }"#,
        ))
        .await
        .expect("create unplaced");
    assert_eq!(unplaced.status(), StatusCode::OK);
    let unplaced_body = json_body(unplaced).await;
    let unplaced_id = unplaced_body["location"]["id"]
        .as_str()
        .expect("unplaced id")
        .to_string();

    let placed = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/locations/move",
            &token,
            &format!(
                r#"{{"location_id":"{unplaced_id}","quantity":6,"row_letter":"C","column_number":2}}"#
            ),
        ))
        .await
        .expect("place unplaced");
    assert_eq!(placed.status(), StatusCode::OK);
    let placed_body = json_body(placed).await;
    assert_eq!(placed_body["location"]["location_label"], "C2");
    assert_eq!(placed_body["location"]["quantity"], 6);
    let placed_id = placed_body["location"]["id"]
        .as_str()
        .expect("placed id")
        .to_string();

    let checkout = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/checkouts",
            &token,
            &format!(
                r#"{{"location_id":"{placed_id}","quantity":2,"worker_id":"return_worker_1"}}"#
            ),
        ))
        .await
        .expect("issue checkout");
    assert_eq!(checkout.status(), StatusCode::OK);
    let checkout_body = json_body(checkout).await;
    let checkout_id = checkout_body["checkout"]["id"]
        .as_str()
        .expect("checkout id")
        .to_string();

    let returned = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/checkouts/return",
            &token,
            &format!(r#"{{"checkout_id":"{checkout_id}"}}"#),
        ))
        .await
        .expect("return checkout");
    assert_eq!(returned.status(), StatusCode::OK);
    let returned_body = json_body(returned).await;
    assert_eq!(returned_body["checkout"]["status"], "returned");

    let locations = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/qolip/locations?block=A", &token))
        .await
        .expect("list locations");
    let locations_body = json_body(locations).await;
    let qty = locations_body["locations"]
        .as_array()
        .expect("locations")
        .iter()
        .find(|entry| entry["id"] == placed_id)
        .and_then(|entry| entry["quantity"].as_i64())
        .expect("restored quantity");
    assert_eq!(qty, 6);

    let moved = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/locations/move",
            &token,
            &format!(
                r#"{{"location_id":"{placed_id}","quantity":3,"row_letter":"D","column_number":1}}"#
            ),
        ))
        .await
        .expect("move partial");
    assert_eq!(moved.status(), StatusCode::OK);
    let moved_body = json_body(moved).await;
    assert_eq!(moved_body["location"]["location_label"], "D1");
    assert_eq!(moved_body["location"]["quantity"], 3);
}

#[tokio::test]
async fn qolip_return_can_restore_to_selected_cell() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let _ = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"id":"return_worker_2","name":"Return worker 2","phone":"+998901112279","level":"Master"}"#,
        ))
        .await
        .expect("create worker");

    let placed = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/locations",
            &token,
            r#"{
                "block":"A",
                "warehouse":"Qolip ombor",
                "item_code":"ITEM-3",
                "item_name":"Return target qolip",
                "qolip_code":"Q-300",
                "size":42,
                "quantity":4,
                "row_letter":"C",
                "column_number":2
            }"#,
        ))
        .await
        .expect("create location");
    assert_eq!(placed.status(), StatusCode::OK);
    let placed_body = json_body(placed).await;
    let placed_id = placed_body["location"]["id"]
        .as_str()
        .expect("placed id")
        .to_string();

    let checkout = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/checkouts",
            &token,
            &format!(
                r#"{{"location_id":"{placed_id}","quantity":3,"worker_id":"return_worker_2"}}"#
            ),
        ))
        .await
        .expect("issue checkout");
    assert_eq!(checkout.status(), StatusCode::OK);
    let checkout_body = json_body(checkout).await;
    let checkout_id = checkout_body["checkout"]["id"]
        .as_str()
        .expect("checkout id")
        .to_string();

    let returned = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/checkouts/return",
            &token,
            &format!(r#"{{"checkout_id":"{checkout_id}","row_letter":"E","column_number":4}}"#),
        ))
        .await
        .expect("return checkout");
    assert_eq!(returned.status(), StatusCode::OK);
    let returned_body = json_body(returned).await;
    assert_eq!(returned_body["checkout"]["status"], "returned");

    let locations = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/qolip/locations?block=A", &token))
        .await
        .expect("list locations");
    let locations_body = json_body(locations).await;
    let locations = locations_body["locations"].as_array().expect("locations");
    let restored = locations
        .iter()
        .find(|entry| entry["location_label"] == "E4")
        .expect("restored target location");
    assert_eq!(restored["quantity"], 3);
    assert_eq!(restored["item_code"], "ITEM-3");

    let original = locations
        .iter()
        .find(|entry| entry["location_label"] == "C2")
        .expect("original source location");
    assert_eq!(original["quantity"], 1);
}
