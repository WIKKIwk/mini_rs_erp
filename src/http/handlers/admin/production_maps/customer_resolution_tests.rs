use super::*;

fn test_map() -> ProductionMapDefinition {
    serde_json::from_value(serde_json::json!({
        "id": "zakaz-8768",
        "product_code": "YASHIL",
        "title": "yashil",
        "order_number": "8768"
    }))
    .expect("test production map")
}

#[test]
fn label_order_code_matches_the_order_screen_fallbacks() {
    let mut map = test_map();
    map.code = " WORK-42 ".into();
    assert_eq!(production_map_print_order_code(&map).as_deref(), Some("WORK-42"));
    map.code.clear();
    assert_eq!(production_map_print_order_code(&map).as_deref(), Some("8768"));
    map.order_number.clear();
    assert_eq!(production_map_print_order_code(&map).as_deref(), Some("8768"));
    map.id = "internal-id".into();
    assert_eq!(production_map_print_order_code(&map), None);
}

#[test]
fn resolves_legacy_order_from_its_template_map() {
    let template = CalculateOrderTemplate {
        customer: "555 kukuruz".to_string(),
        source_map_id: "template-zakaz-8768".to_string(),
        ..CalculateOrderTemplate::default()
    };

    assert_eq!(
        resolve_production_map_customer(&test_map(), &[template]).as_deref(),
        Some("555 kukuruz")
    );
}

#[test]
fn refuses_ambiguous_product_customer_fallback() {
    let templates = [
        CalculateOrderTemplate {
            customer: "Customer A".to_string(),
            product: "yashil".to_string(),
            ..CalculateOrderTemplate::default()
        },
        CalculateOrderTemplate {
            customer: "Customer B".to_string(),
            product: "yashil".to_string(),
            ..CalculateOrderTemplate::default()
        },
    ];

    assert_eq!(
        resolve_production_map_customer(&test_map(), &templates),
        None
    );
}
