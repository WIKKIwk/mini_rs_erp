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
async fn qolip_move_changes_only_the_location_across_existing_blocks() {
    let mut state = test_state();
    let store = Arc::new(crate::core::qolip::MemoryQolipStore::new());
    store
        .seed_blocks(vec![
            crate::core::qolip::QolipBlock {
                name: "A".to_string(),
                warehouse: "Qolip ombor".to_string(),
            },
            crate::core::qolip::QolipBlock {
                name: "B".to_string(),
                warehouse: "Qolip ombor".to_string(),
            },
        ])
        .await;
    state.qolip = crate::core::qolip::QolipService::new(store);
    let token = session(&state, PrincipalRole::Admin).await;

    let source = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/locations",
            &token,
            r#"{
                "block":"A",
                "warehouse":"Qolip ombor",
                "item_code":"ITEM-CROSS-BLOCK",
                "item_name":"Cross-block qolip",
                "qolip_code":"Q-CROSS-BLOCK",
                "size":44,
                "quantity":2,
                "row_letter":"A",
                "column_number":1
            }"#,
        ))
        .await
        .expect("create source location");
    assert_eq!(source.status(), StatusCode::OK);
    let source_body = json_body(source).await;
    let source_id = source_body["location"]["id"]
        .as_str()
        .expect("source id");

    let moved = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/locations/move",
            &token,
            &format!(
                r#"{{"location_id":"{source_id}","block":"B","warehouse":"ignored-client-value","quantity":2,"row_letter":"D","column_number":13}}"#
            ),
        ))
        .await
        .expect("move across blocks");
    assert_eq!(moved.status(), StatusCode::OK);
    let moved_body = json_body(moved).await;
    assert_eq!(moved_body["location"]["block"], "B");
    assert_eq!(moved_body["location"]["warehouse"], "Qolip ombor");
    assert_eq!(moved_body["location"]["location_label"], "D13");
    assert_eq!(moved_body["location"]["qolip_code"], "Q-CROSS-BLOCK");

    let source_locations = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/qolip/locations?block=A",
            &token,
        ))
        .await
        .expect("list source block");
    assert_eq!(source_locations.status(), StatusCode::OK);
    assert_eq!(
        json_body(source_locations).await["locations"]
            .as_array()
            .expect("source locations")
            .len(),
        0
    );

    let target_locations = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/qolip/locations?block=B",
            &token,
        ))
        .await
        .expect("list target block");
    assert_eq!(target_locations.status(), StatusCode::OK);
    let target_body = json_body(target_locations).await;
    assert_eq!(target_body["locations"][0]["qolip_code"], "Q-CROSS-BLOCK");

    let blocks = build_router(state)
        .oneshot(request("GET", "/v1/mobile/qolip/blocks", &token))
        .await
        .expect("list unchanged blocks");
    assert_eq!(blocks.status(), StatusCode::OK);
    assert_eq!(
        json_body(blocks).await["blocks"]
            .as_array()
            .expect("blocks")
            .len(),
        2
    );
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
