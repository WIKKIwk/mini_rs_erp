use super::*;
use crate::core::formula::{CalculateRequest, LayerInput, calculate};

pub(crate) fn products() -> Vec<OrderProduct> {
    [
        ("Normavitus", 115.0, 1),
        ("Baktronorm", 115.0, 1),
        ("Collagen", 80.0, 2),
        ("Minavitos", 85.0, 2),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (name, width, count))| OrderProduct {
        id: format!("p{i}"),
        item_code: format!("SKU-{i}"),
        product: name.into(),
        frame_product_size_mm: width,
        frame_count: count,
    })
    .collect()
}

#[test]
fn multi_product_width_adds_edge_only_once_and_preserves_order() {
    let products = products();
    validate_products(&products).unwrap();
    assert_eq!(useful_width(&products), 560.0);
    assert_eq!(frame_count(&products), 6);
    let lanes = lanes(&products, "zakaz-1001", "1001");
    assert_eq!(
        lanes.iter().map(|l| l.width_mm).collect::<Vec<_>>(),
        vec![115.0, 115.0, 80.0, 80.0, 85.0, 85.0]
    );
    assert_eq!(lanes[2].product_id, "zakaz-1001:product:p2");
    assert_eq!(lanes[2].order_number, "1001/3");
    let calculated = calculate(CalculateRequest {
        kg: Some(300.0),
        products,
        edge_allowance_mm: Some(15.0),
        layers: vec![LayerInput::new("pet", "12")],
        ..Default::default()
    })
    .unwrap();
    assert_eq!(calculated.width_mm, 575.0);
    assert_eq!(calculated.frame_count, 6.0);
    assert_eq!(calculated.frame_product_size_mm, 0.0);
    let expected = 300.0 * 1_000_000.0 / (575.0 * calculated.results[0].total_gsm);
    assert!((calculated.results[0].base_length - expected).abs() < 0.00001);
}

#[test]
fn multi_product_groups_use_real_widths() {
    let widths = vec![115.0, 115.0, 80.0, 80.0, 85.0, 85.0];
    assert_eq!(
        group_widths(&widths, &[2, 2, 2]),
        Some(vec![230.0, 160.0, 170.0])
    );
    assert_eq!(group_widths(&widths, &[3, 3]), Some(vec![310.0, 250.0]));
    assert!(group_widths(&widths, &[3, 2]).is_none());
    assert!(group_widths(&widths, &[0, 6]).is_none());
}

#[test]
fn multi_product_rejects_invalid_or_duplicate_rows_but_allows_repeated_sku() {
    let mut p = products();
    p[1].item_code = p[0].item_code.clone();
    validate_products(&p).unwrap();
    p[1].id = p[0].id.clone();
    assert!(validate_products(&p).is_err());
    p = products();
    p[0].frame_count = 0;
    assert!(validate_products(&p).is_err());
    p = products();
    p[0].frame_product_size_mm = f64::NAN;
    assert!(validate_products(&p).is_err());
    p = products();
    p[0].frame_count = 1024;
    assert!(validate_products(&p).is_err());
}

#[test]
fn multi_product_template_roundtrip_keeps_layout_dimensions() {
    let template = crate::core::calculate_orders::hydrate_template_dimensions(
        crate::core::calculate_orders::CalculateOrderTemplate {
            products: products(),
            customer_ref: "C1".into(),
            edge_allowance_mm: 15.0,
            layers: vec![LayerInput::new("pet", "12")],
            ..Default::default()
        },
    );
    crate::core::calculate_orders::validate_template(&template).unwrap();
    let decoded = serde_json::from_value(serde_json::to_value(&template).unwrap()).unwrap();
    assert_eq!(
        template,
        crate::core::calculate_orders::hydrate_template_dimensions(decoded)
    );
    assert_eq!(template.width_mm, 575.0);
}
