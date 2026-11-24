use super::{AdminErrorResponse, optional_search_limit};

#[test]
fn optional_search_limit_matches_go_defaults_and_clamp() {
    assert_eq!(optional_search_limit(None, 20, 50), 20);
    assert_eq!(optional_search_limit(Some(""), 20, 50), 20);
    assert_eq!(optional_search_limit(Some("abc"), 20, 50), 20);
    assert_eq!(optional_search_limit(Some("0"), 20, 50), 20);
    assert_eq!(optional_search_limit(Some("5"), 20, 50), 5);
    assert_eq!(optional_search_limit(Some("500"), 20, 50), 50);
}

#[test]
fn admin_error_response_stays_flat_and_pointer_sized() {
    let response = AdminErrorResponse::roll_size_mismatch(100.0, 110.0, 100.0, 120.0);
    let value = serde_json::to_value(&response).expect("admin error json");

    assert_eq!(value["error"], "raw_material_roll_size_mismatch");
    assert_eq!(value["order_width_mm"], 100.0);
    assert_eq!(value["roll_width_mm"], 110.0);
    assert_eq!(value["minimum_width_mm"], 100.0);
    assert_eq!(value["maximum_width_mm"], 120.0);
    assert!(value.get("details").is_none());
    assert!(std::mem::size_of::<AdminErrorResponse>() <= 32);
}

#[test]
fn admin_error_optional_fields_remain_top_level() {
    let response = AdminErrorResponse::with_apparatus_options(
        "raw_material_group_ambiguous",
        vec!["apparatus:catalog:pechat-001".to_string()],
    );
    let value = serde_json::to_value(response).expect("admin error json");

    assert_eq!(value["error"], "raw_material_group_ambiguous");
    assert_eq!(
        value["apparatus_options"],
        serde_json::json!(["apparatus:catalog:pechat-001"])
    );
    assert!(value.get("details").is_none());
    assert!(value.get("blockers").is_none());
}
