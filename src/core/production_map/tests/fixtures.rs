use std::sync::Arc;

use crate::core::apparatus_groups::{
    ApparatusGroupService, ApparatusMasterData, ApparatusUpsert, MemoryApparatusGroupStore,
};
use crate::core::production_map::*;

pub(super) fn sample_map() -> ProductionMapDefinition {
    ProductionMapDefinition {
        id: "hotlunch-test".to_string(),
        product_code: "HOTLUNCH".to_string(),
        title: "Hotlunch test".to_string(),
        code: String::new(),
        order_number: String::new(),
        customer_name: String::new(),
        roll_count: None,
        width_mm: None,
        order_kg: None,
        base_length: None,
        nodes: vec![
            ProductionMapNode {
                id: "start".to_string(),
                kind: ProductionMapNodeKind::Start,
                title: "Start".to_string(),
                apparatus_id: String::new(),
                formula: None,
                role_code: String::new(),
                item_code: String::new(),
                qty_formula: String::new(),
                from_location: String::new(),
                to_location: String::new(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: String::new(),
                alternative_assigned_apparatus_id: String::new(),
                rezka_kadr_count: None,
                rezka_label_length: None,
                x: 0.0,
                y: 0.0,
            },
            ProductionMapNode {
                id: "formula".to_string(),
                kind: ProductionMapNodeKind::Formula,
                title: "CPP hisob".to_string(),
                apparatus_id: String::new(),
                formula: Some(ProductionFormula {
                    target: "cpp_kg".to_string(),
                    expression: "order_qty * 1.08".to_string(),
                }),
                role_code: String::new(),
                item_code: "CPP".to_string(),
                qty_formula: String::new(),
                from_location: String::new(),
                to_location: String::new(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: String::new(),
                alternative_assigned_apparatus_id: String::new(),
                rezka_kadr_count: None,
                rezka_label_length: None,
                x: 0.0,
                y: 0.0,
            },
            ProductionMapNode {
                id: "task".to_string(),
                kind: ProductionMapNodeKind::Task,
                title: "Rezkaga yuborish".to_string(),
                apparatus_id: String::new(),
                formula: None,
                role_code: "rezkachi".to_string(),
                item_code: String::new(),
                qty_formula: "cpp_kg".to_string(),
                from_location: "CPP ombor".to_string(),
                to_location: "Rezka apparat".to_string(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: String::new(),
                alternative_assigned_apparatus_id: String::new(),
                rezka_kadr_count: None,
                rezka_label_length: None,
                x: 0.0,
                y: 0.0,
            },
            ProductionMapNode {
                id: "end".to_string(),
                kind: ProductionMapNodeKind::End,
                title: "End".to_string(),
                apparatus_id: String::new(),
                formula: None,
                role_code: String::new(),
                item_code: String::new(),
                qty_formula: String::new(),
                from_location: String::new(),
                to_location: String::new(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: String::new(),
                alternative_assigned_apparatus_id: String::new(),
                rezka_kadr_count: None,
                rezka_label_length: None,
                x: 0.0,
                y: 0.0,
            },
        ],
        edges: vec![
            ProductionMapEdge {
                from: "start".to_string(),
                to: "formula".to_string(),
                branch: String::new(),
            },
            ProductionMapEdge {
                from: "formula".to_string(),
                to: "task".to_string(),
                branch: String::new(),
            },
            ProductionMapEdge {
                from: "task".to_string(),
                to: "end".to_string(),
                branch: String::new(),
            },
        ],
    }
}

pub(super) fn apparatus_stage_map(id: &str, apparatus: &str) -> ProductionMapDefinition {
    ProductionMapDefinition {
        id: id.to_string(),
        product_code: format!("{id}-product"),
        title: id.to_string(),
        code: String::new(),
        order_number: String::new(),
        customer_name: String::new(),
        roll_count: None,
        width_mm: None,
        order_kg: None,
        base_length: None,
        nodes: vec![
            ProductionMapNode {
                id: "start".to_string(),
                kind: ProductionMapNodeKind::Start,
                title: "Start".to_string(),
                apparatus_id: String::new(),
                formula: None,
                role_code: String::new(),
                item_code: String::new(),
                qty_formula: String::new(),
                from_location: String::new(),
                to_location: String::new(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: String::new(),
                alternative_assigned_apparatus_id: String::new(),
                rezka_kadr_count: None,
                rezka_label_length: None,
                x: 0.0,
                y: 0.0,
            },
            ProductionMapNode {
                id: "apparatus".to_string(),
                kind: ProductionMapNodeKind::Apparatus,
                title: apparatus.to_string(),
                apparatus_id: apparatus.to_string(),
                formula: None,
                role_code: String::new(),
                item_code: String::new(),
                qty_formula: String::new(),
                from_location: String::new(),
                to_location: String::new(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: String::new(),
                alternative_assigned_apparatus_id: String::new(),
                rezka_kadr_count: None,
                rezka_label_length: None,
                x: 0.0,
                y: 132.0,
            },
            ProductionMapNode {
                id: "end".to_string(),
                kind: ProductionMapNodeKind::End,
                title: "End".to_string(),
                apparatus_id: String::new(),
                formula: None,
                role_code: String::new(),
                item_code: String::new(),
                qty_formula: String::new(),
                from_location: String::new(),
                to_location: String::new(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: String::new(),
                alternative_assigned_apparatus_id: String::new(),
                rezka_kadr_count: None,
                rezka_label_length: None,
                x: 0.0,
                y: 264.0,
            },
        ],
        edges: vec![
            ProductionMapEdge {
                from: "start".to_string(),
                to: "apparatus".to_string(),
                branch: String::new(),
            },
            ProductionMapEdge {
                from: "apparatus".to_string(),
                to: "end".to_string(),
                branch: String::new(),
            },
        ],
    }
}

pub(super) fn canonical_apparatus_stage_map(
    id: &str,
    apparatus_id: &str,
    display_name: &str,
) -> ProductionMapDefinition {
    let mut map = apparatus_stage_map(id, display_name);
    map.nodes
        .iter_mut()
        .find(|node| node.kind == ProductionMapNodeKind::Apparatus)
        .expect("apparatus stage node")
        .apparatus_id = apparatus_id.to_string();
    map
}

pub(super) async fn service_with_default_apparatus(
    store: Arc<MemoryProductionMapStore>,
) -> ProductionMapService {
    let apparatus_groups = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));
    for (id, name) in [
        ("apparatus:default:bosma_7", "7 ta rangli bosma aparat"),
        ("apparatus:default:bosma_8", "8 ta rangli bosma aparat"),
        ("apparatus:default:asset-007", "Laminatsiya 1"),
        ("apparatus:default:asset-010", "Rezka"),
    ] {
        apparatus_groups
            .upsert_apparatus(ApparatusUpsert {
                id: Some(id.to_string()),
                name: name.to_string(),
                master: ApparatusMasterData::default(),
            })
            .await
            .expect("seed default canonical apparatus");
    }
    ProductionMapService::new(store).with_canonical_apparatus_resolver(Arc::new(
        ApparatusGroupCanonicalResolver::new(apparatus_groups),
    ))
}

pub(super) fn canonical_two_stage_map(
    id: &str,
    first_id: &str,
    first_display_name: &str,
    second_id: &str,
    second_display_name: &str,
) -> ProductionMapDefinition {
    let mut map = apparatus_stage_map(id, first_display_name);
    map.nodes
        .iter_mut()
        .find(|node| node.id == "apparatus")
        .expect("first apparatus stage")
        .apparatus_id = first_id.to_string();
    map.nodes.insert(
        2,
        ProductionMapNode {
            id: "second".to_string(),
            kind: ProductionMapNodeKind::Apparatus,
            title: second_display_name.to_string(),
            apparatus_id: second_id.to_string(),
            formula: None,
            role_code: String::new(),
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: String::new(),
            alternative_group_label: String::new(),
            alternative_assigned_title: String::new(),
            alternative_assigned_apparatus_id: String::new(),
            rezka_kadr_count: None,
            rezka_label_length: None,
            x: 0.0,
            y: 264.0,
        },
    );
    map.nodes
        .iter_mut()
        .find(|node| node.id == "end")
        .expect("end stage")
        .y = 396.0;
    map.edges = vec![
        ProductionMapEdge {
            from: "start".to_string(),
            to: "apparatus".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "apparatus".to_string(),
            to: "second".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "second".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
    ];
    map
}

pub(super) fn condition_map() -> ProductionMapDefinition {
    ProductionMapDefinition {
        id: "branch-test".to_string(),
        product_code: "HOTLUNCH".to_string(),
        title: "Branch test".to_string(),
        code: String::new(),
        order_number: String::new(),
        customer_name: String::new(),
        roll_count: None,
        width_mm: None,
        order_kg: None,
        base_length: None,
        nodes: vec![
            ProductionMapNode {
                id: "start".to_string(),
                kind: ProductionMapNodeKind::Start,
                title: "Start".to_string(),
                apparatus_id: String::new(),
                formula: None,
                role_code: String::new(),
                item_code: String::new(),
                qty_formula: String::new(),
                from_location: String::new(),
                to_location: String::new(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: String::new(),
                alternative_assigned_apparatus_id: String::new(),
                rezka_kadr_count: None,
                rezka_label_length: None,
                x: 0.0,
                y: 0.0,
            },
            ProductionMapNode {
                id: "large_order".to_string(),
                kind: ProductionMapNodeKind::Condition,
                title: "Katta partiyami".to_string(),
                apparatus_id: String::new(),
                formula: Some(ProductionFormula {
                    target: String::new(),
                    expression: "order_qty >= 100".to_string(),
                }),
                role_code: String::new(),
                item_code: String::new(),
                qty_formula: String::new(),
                from_location: String::new(),
                to_location: String::new(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: String::new(),
                alternative_assigned_apparatus_id: String::new(),
                rezka_kadr_count: None,
                rezka_label_length: None,
                x: 0.0,
                y: 0.0,
            },
            ProductionMapNode {
                id: "large_task".to_string(),
                kind: ProductionMapNodeKind::Task,
                title: "Katta partiya".to_string(),
                apparatus_id: String::new(),
                formula: None,
                role_code: "rezkachi".to_string(),
                item_code: String::new(),
                qty_formula: "order_qty / 6".to_string(),
                from_location: "CPP ombor".to_string(),
                to_location: "Rezka apparat".to_string(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: String::new(),
                alternative_assigned_apparatus_id: String::new(),
                rezka_kadr_count: None,
                rezka_label_length: None,
                x: 0.0,
                y: 0.0,
            },
            ProductionMapNode {
                id: "small_task".to_string(),
                kind: ProductionMapNodeKind::Task,
                title: "Oddiy partiya".to_string(),
                apparatus_id: String::new(),
                formula: None,
                role_code: "operator".to_string(),
                item_code: String::new(),
                qty_formula: String::new(),
                from_location: String::new(),
                to_location: String::new(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: String::new(),
                alternative_assigned_apparatus_id: String::new(),
                rezka_kadr_count: None,
                rezka_label_length: None,
                x: 0.0,
                y: 0.0,
            },
            ProductionMapNode {
                id: "end".to_string(),
                kind: ProductionMapNodeKind::End,
                title: "End".to_string(),
                apparatus_id: String::new(),
                formula: None,
                role_code: String::new(),
                item_code: String::new(),
                qty_formula: String::new(),
                from_location: String::new(),
                to_location: String::new(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: String::new(),
                alternative_assigned_apparatus_id: String::new(),
                rezka_kadr_count: None,
                rezka_label_length: None,
                x: 0.0,
                y: 0.0,
            },
        ],
        edges: vec![
            ProductionMapEdge {
                from: "start".to_string(),
                to: "large_order".to_string(),
                branch: String::new(),
            },
            ProductionMapEdge {
                from: "large_order".to_string(),
                to: "large_task".to_string(),
                branch: "true".to_string(),
            },
            ProductionMapEdge {
                from: "large_order".to_string(),
                to: "small_task".to_string(),
                branch: "false".to_string(),
            },
            ProductionMapEdge {
                from: "large_task".to_string(),
                to: "end".to_string(),
                branch: String::new(),
            },
            ProductionMapEdge {
                from: "small_task".to_string(),
                to: "end".to_string(),
                branch: String::new(),
            },
        ],
    }
}
