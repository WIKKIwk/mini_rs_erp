use std::collections::BTreeSet;

use serde_json::Value;

use super::rows::{json_number, missing_order_rows, order_sheet_row};
use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapEdge, ProductionMapNode, ProductionMapNodeKind,
};

#[test]
fn order_sheet_row_matches_legacy_excel_columns() {
    let map = ProductionMapDefinition {
        id: "zakaz-7775".to_string(),
        product_code: "ITEM-1".to_string(),
        title: "fibre Mahsulot: XXL pack 70 sht".to_string(),
        code: "7775".to_string(),
        order_number: "7775".to_string(),
        roll_count: Some(8.0),
        width_mm: Some(735.0),
        order_kg: None,
        base_length: None,
        nodes: vec![ProductionMapNode {
            id: "apparatus".to_string(),
            kind: ProductionMapNodeKind::Apparatus,
            title: "7 ta rangli pechat".to_string(),
            formula: None,
            role_code: String::new(),
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: String::new(),
            alternative_group_label: String::new(),
            alternative_assigned_title: "8 ta rangli pechat".to_string(),
            rezka_kadr_count: None,
            rezka_label_length: None,
            x: 0.0,
            y: 0.0,
        }],
        edges: Vec::<ProductionMapEdge>::new(),
    };
    let template = CalculateOrderTemplate {
        name: "fibre Mahsulot: XXL pack 70 sht".to_string(),
        product: "fibre Mahsulot: XXL pack 70 sht".to_string(),
        frame_product_size_mm: 720.0,
        frame_count: 1.0,
        edge_allowance_mm: 15.0,
        width_mm: 735.0,
        waste_percent: 5.0,
        roll_count: Some(8.0),
        first_layer_material: "pet".to_string(),
        first_layer_micron: "12".to_string(),
        second_layer_material: "pe oq".to_string(),
        second_layer_micron: "55".to_string(),
        kg: 600.0,
        ..CalculateOrderTemplate::default()
    };

    let row = order_sheet_row(&map, &template).expect("row");

    assert_eq!(row.len(), 16);
    assert_eq!(row[0], Value::String("8".to_string()));
    assert_eq!(row[3], Value::String("/7775".to_string()));
    assert_eq!(
        row[4],
        Value::String("fibre Mahsulot: XXL pack 70 sht".to_string())
    );
    assert_eq!(row[6], Value::String("pet".to_string()));
    assert_eq!(row[7], Value::String("pe oq".to_string()));
    assert_eq!(row[9], json_number(735.0));
    assert_eq!(row[10], Value::String("12".to_string()));
    assert_eq!(row[11], Value::String("55".to_string()));
    assert_eq!(row[14], json_number(8.0));
    assert_eq!(row[15], json_number(750.0));
}

#[test]
fn order_sheet_row_marks_flexo_orders_with_f() {
    let map = ProductionMapDefinition {
        id: "zakaz-1123".to_string(),
        product_code: "ITEM-F".to_string(),
        title: "fleksa lec Mahsulot".to_string(),
        code: String::new(),
        order_number: "1123".to_string(),
        roll_count: None,
        width_mm: Some(1190.0),
        order_kg: None,
        base_length: None,
        nodes: Vec::new(),
        edges: Vec::new(),
    };
    let template = CalculateOrderTemplate {
        name: "fleksa lec Mahsulot".to_string(),
        product: "fleksa lec Mahsulot".to_string(),
        frame_product_size_mm: 1175.0,
        frame_count: 1.0,
        edge_allowance_mm: 15.0,
        width_mm: 1190.0,
        waste_percent: 5.0,
        first_layer_material: "pe pr".to_string(),
        first_layer_micron: "50".to_string(),
        second_layer_material: "pe pr".to_string(),
        second_layer_micron: "30".to_string(),
        kg: 500.0,
        ..CalculateOrderTemplate::default()
    };

    let row = order_sheet_row(&map, &template).expect("row");

    assert_eq!(row[0], Value::String("F".to_string()));
    assert_eq!(row[3], Value::String("/1123".to_string()));
}

#[test]
fn missing_order_rows_skips_existing_sheet_codes() {
    let maps = vec![
        test_map("zakaz-7775", "7775", "8 ta rangli pechat"),
        test_map("zakaz-7776", "7776", "7 ta rangli pechat"),
    ];
    let templates = vec![
        test_template("zakaz-7775", "7775"),
        test_template("zakaz-7776", "7776"),
    ];
    let existing = BTreeSet::from(["7775".to_string()]);

    let rows = missing_order_rows(&maps, &templates, &existing);

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][3], Value::String("/7776".to_string()));
}

fn test_map(id: &str, order_number: &str, apparatus: &str) -> ProductionMapDefinition {
    ProductionMapDefinition {
        id: id.to_string(),
        product_code: "ITEM-1".to_string(),
        title: "Test order".to_string(),
        code: order_number.to_string(),
        order_number: order_number.to_string(),
        roll_count: Some(7.0),
        width_mm: Some(650.0),
        order_kg: None,
        base_length: None,
        nodes: vec![ProductionMapNode {
            id: "apparatus".to_string(),
            kind: ProductionMapNodeKind::Apparatus,
            title: apparatus.to_string(),
            formula: None,
            role_code: String::new(),
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: String::new(),
            alternative_group_label: String::new(),
            alternative_assigned_title: String::new(),
            rezka_kadr_count: None,
            rezka_label_length: None,
            x: 0.0,
            y: 0.0,
        }],
        edges: Vec::new(),
    }
}

fn test_template(source_map_id: &str, order_number: &str) -> CalculateOrderTemplate {
    CalculateOrderTemplate {
        code: order_number.to_string(),
        order_number: order_number.to_string(),
        source_map_id: source_map_id.to_string(),
        name: "Test order".to_string(),
        product: "Test order".to_string(),
        frame_product_size_mm: 635.0,
        frame_count: 1.0,
        edge_allowance_mm: 15.0,
        width_mm: 650.0,
        waste_percent: 5.0,
        roll_count: Some(7.0),
        first_layer_material: "pet".to_string(),
        first_layer_micron: "12".to_string(),
        second_layer_material: "pe oq".to_string(),
        second_layer_micron: "30".to_string(),
        kg: 500.0,
        ..CalculateOrderTemplate::default()
    }
}
