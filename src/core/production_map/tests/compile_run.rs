use std::collections::BTreeMap;

use crate::core::production_map::compiler::{compile_map, run_map, run_map_with_variables};
use crate::core::production_map::*;

use super::fixtures::{condition_map, sample_map};

#[test]
fn compile_map_turns_visual_nodes_into_ordered_operations() {
    let map = sample_map();
    let program = compile_map(&map).expect("compile");

    assert_eq!(program.map_id, "hotlunch-test");
    assert_eq!(program.operations.len(), 4);
    assert_eq!(program.operations[1].op_code, "calculate");
    assert_eq!(
        program.operations[1]
            .args
            .get("expression")
            .map(String::as_str),
        Some("order_qty * 1.08")
    );
    assert_eq!(program.operations[2].op_code, "create_task");
}

#[test]
fn compile_map_accepts_location_markers_without_task_drafts() {
    let mut map = sample_map();
    map.nodes.insert(
        1,
        ProductionMapNode {
            id: "cpp_warehouse".to_string(),
            kind: ProductionMapNodeKind::Location,
            title: "CPP ombor".to_string(),
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
    );
    map.edges[0].to = "cpp_warehouse".to_string();
    map.edges.insert(
        1,
        ProductionMapEdge {
            from: "cpp_warehouse".to_string(),
            to: "formula".to_string(),
            branch: String::new(),
        },
    );

    let program = compile_map(&map).expect("compile");
    assert_eq!(program.operations[1].op_code, "warehouse_location");

    let result = run_map(&map, 100.0).expect("run map");
    assert_eq!(result.tasks.len(), 1);
    assert_eq!(result.tasks[0].node_id, "task");
}

#[test]
fn compile_map_rejects_cycles() {
    let mut map = sample_map();
    map.edges.push(ProductionMapEdge {
        from: "task".to_string(),
        to: "formula".to_string(),
        branch: String::new(),
    });

    assert_eq!(compile_map(&map), Err(ProductionMapError::Cycle));
}

#[test]
fn compile_map_rejects_invalid_formula_expression() {
    let mut map = sample_map();
    map.nodes[1].formula = Some(ProductionFormula {
        target: "cpp_kg".to_string(),
        expression: "order_qty; drop".to_string(),
    });

    assert_eq!(
        compile_map(&map),
        Err(ProductionMapError::InvalidFormulaExpression(
            "order_qty; drop".to_string()
        ))
    );
}

#[test]
fn run_map_evaluates_formulas_and_generates_task_drafts() {
    let result = run_map(&sample_map(), 100.0).expect("run map");

    assert_eq!(result.map_id, "hotlunch-test");
    assert_eq!(result.variables.get("order_qty"), Some(&100.0));
    assert_eq!(result.variables.get("cpp_kg"), Some(&108.0));
    assert_eq!(result.tasks.len(), 1);
    assert_eq!(result.tasks[0].task_kind, "create_task");
    assert_eq!(result.tasks[0].role_code, "rezkachi");
    assert_eq!(result.tasks[0].qty, 108.0);
    assert_eq!(result.tasks[0].from_location, "CPP ombor");
    assert_eq!(result.tasks[0].to_location, "Rezka apparat");
    assert_eq!(result.visited_node_ids, ["start", "formula", "task", "end"]);
}

#[test]
fn run_map_follows_condition_branch() {
    let result = run_map(&condition_map(), 120.0).expect("run map");

    assert_eq!(result.variables.get("large_order"), Some(&1.0));
    assert_eq!(result.tasks.len(), 1);
    assert_eq!(result.tasks[0].node_id, "large_task");

    let result = run_map(&condition_map(), 60.0).expect("run map");
    assert_eq!(result.variables.get("large_order"), Some(&0.0));
    assert_eq!(result.tasks.len(), 1);
    assert_eq!(result.tasks[0].node_id, "small_task");
}

#[test]
fn run_map_conditions_can_use_runtime_variables() {
    let mut map = condition_map();
    map.nodes[1].formula = Some(ProductionFormula {
        target: String::new(),
        expression: "pechat_ok == 1".to_string(),
    });

    let result = run_map_with_variables(
        &map,
        100.0,
        BTreeMap::from([("pechat_ok".to_string(), 1.0)]),
    )
    .expect("run map with ok result");

    assert_eq!(result.variables.get("pechat_ok"), Some(&1.0));
    assert!(result.awaiting_variable.is_empty());
    assert_eq!(result.tasks[0].node_id, "large_task");

    let result = run_map_with_variables(
        &map,
        100.0,
        BTreeMap::from([("pechat_ok".to_string(), 0.0)]),
    )
    .expect("run map with failed result");
    assert_eq!(result.tasks[0].node_id, "small_task");
}

#[test]
fn run_map_stops_at_condition_when_runtime_variable_is_missing() {
    let mut map = condition_map();
    map.nodes[1].formula = Some(ProductionFormula {
        target: String::new(),
        expression: "pechat_ok == 1".to_string(),
    });

    let result = run_map(&map, 100.0).expect("run map waiting for variable");

    assert_eq!(result.tasks.len(), 0);
    assert_eq!(result.awaiting_node_id, "large_order");
    assert_eq!(result.awaiting_variable, "pechat_ok");
    assert_eq!(result.awaiting_expression, "pechat_ok == 1");
    assert_eq!(result.visited_node_ids, ["start", "large_order"]);
}

#[test]
fn run_map_rejects_non_positive_node_qty() {
    let mut map = sample_map();
    map.nodes[2].qty_formula = "order_qty - 100".to_string();

    assert_eq!(
        run_map(&map, 100.0),
        Err(ProductionMapError::InvalidNodeQty("task".to_string()))
    );
}
