use super::*;

fn window(version: &str, revision: u64, rows: Value) -> Value {
    json!({"ok":true,"projection":PROJECTION,"scope":"master-scope","epoch":"server-epoch",
        "rev":revision,"version":version,"apparatus_id":"apparatus:default:bosma_7",
        "apparatus":[{"id":"apparatus:default:bosma_7","name":"Bosma"}],
        "search":"","limit":30,"total":rows.as_array().unwrap().len(),
        "next_cursor":null,"rows":rows})
}

fn live_query(version: &str) -> OrderListQuery {
    OrderListQuery {
        limit: 30,
        scope: "master-scope".into(),
        epoch: "server-epoch".into(),
        version: version.into(),
        length: Some(30),
        ..Default::default()
    }
}

#[test]
fn preparation_cache_validates_metadata_and_shares_concurrent_forced_refreshes() {
    let requested = Instant::now();
    let cached = CompactRows {
        rows: Vec::new(),
        version: "content".into(),
        metadata_stamp: "map-product-template-tuples".into(),
        built_at: requested,
    };
    assert!(cached.reusable("map-product-template-tuples", false, Instant::now()));
    assert!(!cached.reusable("changed-template-label", false, requested));
    assert!(cached.reusable("map-product-template-tuples", true, requested));
    assert!(!cached.reusable(
        "map-product-template-tuples",
        true,
        requested + Duration::from_millis(1)
    ));
}

#[test]
fn preparation_live_uses_checkpoints_for_unrelated_production_revisions() {
    let before = window(
        "same-content",
        7,
        json!([{"id":"zakaz-one","title":"One","position":2}]),
    );
    let mut after = before.clone();
    after["rev"] = 99.into();
    let query = live_query("same-content");
    let ready = list_live_frame(None, before.clone(), &query);
    assert_eq!(ready["type"], "ready");
    assert!(ready.get("rows").is_none());
    let checkpoint = list_live_frame(Some(&before), after, &query);
    assert_eq!(checkpoint["type"], "checkpoint");
    assert_eq!(checkpoint["rev"], 99);
    assert!(checkpoint.get("rows").is_none());
    assert!(checkpoint.get("upserts").is_none());
}

#[test]
fn preparation_live_delta_retains_exact_prefix_order_and_changed_positions() {
    let before = window(
        "old",
        1,
        json!([
            {"id":"zakaz-a","title":"A","position":1},
            {"id":"zakaz-b","title":"B","position":2},
            {"id":"zakaz-c","title":"C","position":3}
        ]),
    );
    let after = window(
        "new",
        2,
        json!([
            {"id":"zakaz-b","title":"Renamed B","position":1},
            {"id":"zakaz-d","title":"D","position":2},
            {"id":"zakaz-c","title":"C","position":3}
        ]),
    );
    let delta = list_live_frame(Some(&before), after, &live_query("old"));
    assert_eq!(delta["type"], "list_delta");
    assert_eq!(delta["base_version"], "old");
    assert_eq!(delta["order_ids"], json!(["zakaz-b", "zakaz-d", "zakaz-c"]));
    assert_eq!(delta["removed_ids"], json!(["zakaz-a"]));
    assert_eq!(
        delta["upserts"],
        json!([
            {"id":"zakaz-b","title":"Renamed B","position":1},
            {"id":"zakaz-d","title":"D","position":2}
        ])
    );
    assert!(delta.get("rows").is_none());
}

#[test]
fn preparation_live_replaces_baselines_after_scope_or_server_changes() {
    let before = window("old", 1, json!([{"id":"zakaz-a"}]));
    let mut reassigned = window("new", 2, json!([]));
    reassigned["scope"] = "different-material-responsibilities".into();
    let snapshot = list_live_frame(Some(&before), reassigned, &live_query("old"));
    assert_eq!(snapshot["type"], "list_snapshot");
    assert_eq!(snapshot["rows"], json!([]));
    assert!(snapshot.get("base_version").is_none());
    let mut restarted = before.clone();
    restarted["epoch"] = "other-process".into();
    assert_eq!(
        list_live_frame(None, restarted, &live_query("old"))["type"],
        "list_snapshot"
    );
}

#[test]
fn preparation_conditional_response_updates_metadata_without_resending_rows() {
    let mut query = live_query("content");
    query.if_version = "content".into();
    let payload = window("content", 42, json!([{"id":"zakaz-a"}]));
    let response = list_response(&query, payload.clone());
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(response.headers()["x-production-revision"], "42");
    assert_eq!(response.headers()["x-preparation-scope"], "master-scope");
    query.if_version = "another-account-or-content".into();
    assert_eq!(
        list_response(&query, payload.clone()).status(),
        StatusCode::OK
    );
    query.if_version = "content".into();
    query.cursor = "next-page".into();
    assert_eq!(list_response(&query, payload).status(), StatusCode::OK);
}

#[test]
fn preparation_order_row_does_not_replace_missing_customer_with_product_code() {
    let map: ProductionMapDefinition = serde_json::from_value(json!({
        "id":"zakaz-row","title":"Order","product_code":"PRODUCT-42",
        "nodes":[{"id":"end","kind":"end","title":"Product title"}],"edges":[]
    }))
    .unwrap();
    let value = row(&map, 7);
    assert!(value.customer_name.is_empty());
    assert_eq!(value.title, "Order");
    assert_eq!(value.position, 7);
}

#[test]
fn preparation_order_cursor_rejects_scope_and_revision_reuse() {
    let cursor = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&("version-a", 30usize)).unwrap());
    assert_eq!(offset(&cursor, "version-a").unwrap(), 30);
    assert_eq!(
        offset(&cursor, "version-b").unwrap_err().0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        offset("invalid", "v").unwrap_err().0,
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn preparation_order_query_is_bounded_and_responsibility_scope_is_stable() {
    let q = OrderListQuery {
        limit: 30,
        search: " Film ".into(),
        ..Default::default()
    }
    .normalized()
    .unwrap();
    assert_eq!(q.search, "film");
    assert!(
        OrderListQuery {
            limit: 51,
            ..Default::default()
        }
        .normalized()
        .is_err()
    );
    let a = responsibilities(
        &json!({"materials":[{"material_id":"B","material_name":"Film"},{"material_id":"A","material_name":"Glue"}]}),
    );
    let b = responsibilities(
        &json!({"materials":[{"material_id":"a","material_name":"glue"},{"material_id":"b","material_name":"film"}]}),
    );
    assert_eq!(a, b);
    assert_ne!(digest(&a), digest(&a[..1]));
}
