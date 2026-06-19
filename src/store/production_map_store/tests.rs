use std::sync::Arc;

use super::*;
use crate::core::production_map::{ProductionMapNode, ProductionMapNodeKind, ProductionMapService};

#[tokio::test]
async fn production_map_store_persists_maps_in_sqlite() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mobile_production_maps.sqlite");
    let service = ProductionMapService::new(Arc::new(ProductionMapStore::new(path.clone())));

    service
        .upsert_map(ProductionMapDefinition {
            id: "map-1".to_string(),
            product_code: "HOT".to_string(),
            title: "Hot".to_string(),
            code: "Z-HOT-1".to_string(),
            order_number: "1234".to_string(),
            roll_count: Some(7.0),
            width_mm: Some(650.0),
            order_kg: None,
            base_length: None,
            nodes: vec![
                ProductionMapNode {
                    id: "start".to_string(),
                    kind: ProductionMapNodeKind::Start,
                    title: "Start".to_string(),
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
                },
                ProductionMapNode {
                    id: "apparatus".to_string(),
                    kind: ProductionMapNodeKind::Apparatus,
                    title: "Extrujen aparat - A".to_string(),
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
                    y: 132.0,
                },
                ProductionMapNode {
                    id: "end".to_string(),
                    kind: ProductionMapNodeKind::End,
                    title: "End".to_string(),
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
                    y: 264.0,
                },
            ],
            edges: vec![
                crate::core::production_map::ProductionMapEdge {
                    from: "start".to_string(),
                    to: "apparatus".to_string(),
                    branch: String::new(),
                },
                crate::core::production_map::ProductionMapEdge {
                    from: "apparatus".to_string(),
                    to: "end".to_string(),
                    branch: String::new(),
                },
            ],
        })
        .await
        .expect("save map");
    drop(service);

    let conn = rusqlite::Connection::open(&path).expect("open sqlite");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM production_maps", [], |row| row.get(0))
        .expect("count maps");
    assert_eq!(count, 1);
    drop(conn);

    let reloaded = ProductionMapService::new(Arc::new(ProductionMapStore::new(path)));
    let maps = reloaded.maps().await.expect("maps");
    assert_eq!(maps.len(), 1);
    assert_eq!(maps[0].map.product_code, "HOT");
    assert_eq!(maps[0].map.order_number, "1234");
    assert_eq!(maps[0].map.roll_count, Some(7.0));
    assert_eq!(maps[0].map.width_mm, Some(650.0));
    assert_eq!(maps[0].program.operations.len(), 3);
    assert_eq!(maps[0].program.operations[1].op_code, "apparatus");

    let duplicate = reloaded
        .upsert_map(ProductionMapDefinition {
            id: "map-2".to_string(),
            product_code: "OTHER".to_string(),
            title: "Other".to_string(),
            code: String::new(),
            order_number: "1234".to_string(),
            roll_count: None,
            width_mm: None,
            order_kg: None,
            base_length: None,
            nodes: maps[0].map.nodes.clone(),
            edges: maps[0].map.edges.clone(),
        })
        .await;
    assert_eq!(duplicate, Err(ProductionMapError::DuplicateOrderNumber));
}
