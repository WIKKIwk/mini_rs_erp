use std::collections::{BTreeMap, BTreeSet};

use super::super::formula::{evaluate_condition, evaluate_formula};
use super::super::types::*;
use super::compile_map;
use super::normalize::normalize_branch;
use super::operation::compile_node;

#[cfg(test)]
pub fn run_map(
    map: &ProductionMapDefinition,
    order_qty: f64,
) -> Result<ProductionMapRunResult, ProductionMapError> {
    run_map_with_variables(map, order_qty, BTreeMap::new())
}

pub fn run_map_with_variables(
    map: &ProductionMapDefinition,
    order_qty: f64,
    run_variables: BTreeMap<String, f64>,
) -> Result<ProductionMapRunResult, ProductionMapError> {
    if order_qty <= 0.0 {
        return Err(ProductionMapError::InvalidOrderQty);
    }
    compile_map(map)?;
    let node_by_id: BTreeMap<&str, &ProductionMapNode> = map
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut outgoing = BTreeMap::<&str, Vec<&ProductionMapEdge>>::new();
    for edge in &map.edges {
        outgoing.entry(edge.from.as_str()).or_default().push(edge);
    }
    let mut variables = input_variables(order_qty, run_variables);
    let mut tasks = Vec::new();
    let Some(mut current_id) = map
        .nodes
        .iter()
        .find(|node| node.kind == ProductionMapNodeKind::Start)
        .map(|node| node.id.as_str())
    else {
        return Err(ProductionMapError::MissingStart);
    };
    let mut visited = BTreeSet::new();
    let mut visited_node_ids = Vec::new();
    while visited.insert(current_id.to_string()) {
        let node = node_by_id
            .get(current_id)
            .expect("compiled map only contains known node ids");
        visited_node_ids.push(node.id.clone());
        if node.kind == ProductionMapNodeKind::End {
            break;
        }
        match node.kind {
            ProductionMapNodeKind::Formula => {
                let Some(formula) = &node.formula else {
                    return Err(ProductionMapError::MissingFormulaExpression);
                };
                let value = evaluate_formula(&formula.expression, &variables)?;
                variables.insert(formula.target.clone(), value);
            }
            ProductionMapNodeKind::Condition => {
                let Some(formula) = &node.formula else {
                    return Err(ProductionMapError::MissingFormulaExpression);
                };
                let result = match evaluate_condition(&formula.expression, &variables) {
                    Ok(result) => result,
                    Err(ProductionMapError::UnknownFormulaVariable(variable)) => {
                        return Ok(ProductionMapRunResult {
                            map_id: map.id.clone(),
                            product_code: map.product_code.clone(),
                            order_qty,
                            variables,
                            tasks,
                            visited_node_ids,
                            awaiting_node_id: node.id.clone(),
                            awaiting_variable: variable,
                            awaiting_expression: formula.expression.clone(),
                        });
                    }
                    Err(error) => return Err(error),
                };
                variables.insert(node.id.clone(), if result { 1.0 } else { 0.0 });
            }
            ProductionMapNodeKind::Location => {}
            ProductionMapNodeKind::Material
            | ProductionMapNodeKind::Apparatus
            | ProductionMapNodeKind::KkProduct
            | ProductionMapNodeKind::Task
            | ProductionMapNodeKind::Wait
            | ProductionMapNodeKind::Output => {
                let qty = node_qty(node, order_qty, &variables)?;
                tasks.push(ProductionTaskDraft {
                    order: tasks.len() + 1,
                    node_id: node.id.clone(),
                    task_kind: compile_node(tasks.len() + 1, node)?.op_code,
                    title: node.title.clone(),
                    role_code: node.role_code.clone(),
                    item_code: node.item_code.clone(),
                    from_location: node.from_location.clone(),
                    to_location: node.to_location.clone(),
                    qty,
                })
            }
            ProductionMapNodeKind::Start | ProductionMapNodeKind::End => {}
        }
        let edges = outgoing.get(current_id).cloned().unwrap_or_default();
        if node.kind == ProductionMapNodeKind::Condition {
            let branch = if variables.get(&node.id).copied().unwrap_or(0.0) != 0.0 {
                "true"
            } else {
                "false"
            };
            let Some(next) = edges
                .into_iter()
                .find(|edge| normalize_branch(&edge.branch) == branch)
            else {
                return Err(ProductionMapError::MissingConditionBranch);
            };
            current_id = next.to.as_str();
        } else {
            let Some(next) = edges.first() else {
                break;
            };
            current_id = next.to.as_str();
        }
    }
    Ok(ProductionMapRunResult {
        map_id: map.id.clone(),
        product_code: map.product_code.clone(),
        order_qty,
        variables,
        tasks,
        visited_node_ids,
        awaiting_node_id: String::new(),
        awaiting_variable: String::new(),
        awaiting_expression: String::new(),
    })
}

fn input_variables(order_qty: f64, mut variables: BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    variables.insert("order_qty".to_string(), order_qty);
    variables
}

fn node_qty(
    node: &ProductionMapNode,
    order_qty: f64,
    variables: &BTreeMap<String, f64>,
) -> Result<f64, ProductionMapError> {
    let qty = if node.qty_formula.trim().is_empty() {
        order_qty
    } else {
        evaluate_formula(&node.qty_formula, variables)?
    };
    if qty.is_finite() && qty > 0.0 {
        Ok(qty)
    } else {
        Err(ProductionMapError::InvalidNodeQty(node.id.clone()))
    }
}
