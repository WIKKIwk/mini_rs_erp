//! Deterministic inputs shared by the opt-in index benchmark and its regressions.

use super::ProductionMapDefinition;

pub(super) fn map(index: usize) -> ProductionMapDefinition {
    serde_json::from_value(serde_json::json!({
        "id": format!("zakaz-research-2026-{index:08}"),
        "product_code": format!("PRODUCT-{:04}", index % 79),
        "title": format!("Research package {index}"),
        "order_number": format!("2600{index:08}"),
        "customer_name": format!("Research customer {}", index % 17),
        "roll_count": 24,
        "width_mm": 720.0,
        "order_kg": 1200.0,
        "base_length": 4000.0,
        "nodes": [
            {"id": "start", "kind": "start", "title": "Start"},
            {"id": "film", "kind": "material", "title": "Film",
             "item_code": "RESEARCH-FILM", "qty_formula": "order_qty * 1.02"},
            {"id": "press", "kind": "apparatus", "title": "Press",
             "apparatus_id": "apparatus:default:bosma_9"},
            {"id": "lamination", "kind": "apparatus", "title": "Lamination",
             "apparatus_id": "apparatus:default:asset-007"},
            {"id": "cut", "kind": "apparatus", "title": "Cut",
             "apparatus_id": "apparatus:default:asset-010"},
            {"id": "end", "kind": "end", "title": "End"}
        ],
        "edges": [
            {"from": "start", "to": "film"},
            {"from": "film", "to": "press"},
            {"from": "press", "to": "lamination"},
            {"from": "lamination", "to": "cut"},
            {"from": "cut", "to": "end"}
        ]
    }))
    .expect("the synthetic snapshot fixture has the production map schema")
}

pub(super) fn maps(size: usize) -> Vec<ProductionMapDefinition> {
    let mut maps = (0..size).map(map).collect::<Vec<_>>();
    // Fixed Fisher-Yates permutation: construction/search order is not sorted
    // by ID, and both independently built variants receive the same sequence.
    let mut state = 0x6b8b_4567_327b_23c6_u64;
    for index in (1..maps.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        maps.swap(index, (state as usize) % (index + 1));
    }
    maps
}
