use super::*;
fn asset(id: &str, name: &str) -> Value {
    json!({"kind":"raw_material","asset_ref":id,"item_name":name,"order_assignment":null})
}
#[test]
fn inventory_live_delta_replaces_only_changed_rows_and_backfills_window() {
    let previous = vec![asset("a", "A"), asset("b", "B")];
    let current = vec![asset("b", "B edited"), asset("c", "C")];
    let change = delta(ViewKind::Assets, &previous, &current).unwrap();
    assert_eq!(change["removed"], json!(["raw_material:a"]));
    assert_eq!(change["upserts"], json!([current[0], current[1]]));
    assert_eq!(change["order"], json!(["raw_material:b", "raw_material:c"]));
}
#[test]
fn inventory_live_delta_ignores_unchanged_and_supports_reorder_and_badges() {
    let original = vec![asset("a", "A"), asset("b", "B")];
    assert!(delta(ViewKind::Assets, &original, &original).is_none());
    let reordered = vec![original[1].clone(), original[0].clone()];
    let change = delta(ViewKind::Assets, &original, &reordered).unwrap();
    assert_eq!(change["upserts"], json!([]));
    assert_eq!(change["order"], json!(["raw_material:b", "raw_material:a"]));
    let mut updated = original.clone();
    updated[0]["order_assignment"] =
        json!({"order_label":"Customer order","order_control":"frozen"});
    assert_eq!(
        delta(ViewKind::Assets, &original, &updated).unwrap()["upserts"],
        json!([updated[0]])
    );
}
#[test]
fn inventory_live_bounds_and_fingerprints_are_scope_and_query_sensitive() {
    let mut subscription: Subscription =
        serde_json::from_value(json!({"id":"a","kind":"assets","query":{"limit":100}}))
            .unwrap();
    assert!(validate_subscriptions(&[subscription.clone()]).is_ok());
    assert!(validate_subscriptions(&[subscription.clone(), subscription.clone()]).is_err());
    let first = version(&json!({"warehouse":"one"}), &subscription, &[]);
    assert_ne!(
        first,
        version(&json!({"warehouse":"two"}), &subscription, &[])
    );
    subscription.query["warehouse_id"] = json!("new");
    assert_ne!(
        first,
        version(&json!({"warehouse":"one"}), &subscription, &[])
    );
    subscription.query["limit"] = json!(501);
    assert!(validate_subscriptions(&[subscription]).is_err());
}
