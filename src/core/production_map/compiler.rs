use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::apparatus::is_laminatsiya_title;
use super::formula::{
    evaluate_condition, evaluate_formula, validate_condition_expression,
    validate_formula_expression, validate_formula_target, validate_location_ref,
};
use super::pechat;
use super::types::*;

const MAX_LAMINATSIYA_RUBBER_SIZE_MM: i64 = 1050;

pub fn compile_map(
    map: &ProductionMapDefinition,
) -> Result<ProductionMapProgram, ProductionMapError> {
    validate_map(map)?;
    let order = topological_order(map)?;
    let node_by_id: BTreeMap<&str, &ProductionMapNode> = map
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut operations = Vec::with_capacity(order.len());
    for (index, node_id) in order.into_iter().enumerate() {
        let node = node_by_id
            .get(node_id.as_str())
            .expect("topological order only contains known node ids");
        operations.push(compile_node(index + 1, node)?);
    }
    Ok(ProductionMapProgram {
        map_id: map.id.clone(),
        product_code: map.product_code.clone(),
        operations,
    })
}

#[cfg(test)]
pub(super) fn reject_order_number_immutable(
    maps: &BTreeMap<String, ProductionMapDefinition>,
    next: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let id = next.id.trim();
    if !id.starts_with("zakaz-") {
        return Ok(());
    }
    let order_number = next.order_number.trim();
    if order_number.is_empty() {
        return Ok(());
    }
    let Some(existing) = maps.get(id) else {
        return Ok(());
    };
    let existing_number = existing.order_number.trim();
    if !existing_number.is_empty() && existing_number != order_number {
        return Err(ProductionMapError::OrderNumberImmutable);
    }
    Ok(())
}

pub(super) fn normalize_map(map: &mut ProductionMapDefinition) {
    map.id = map.id.trim().to_ascii_lowercase();
    map.product_code = map.product_code.trim().to_string();
    map.title = map.title.trim().to_string();
    map.code = map.code.trim().to_string();
    map.order_number = map.order_number.trim().to_string();
    if map
        .roll_count
        .is_some_and(|value| !value.is_finite() || value <= 0.0)
    {
        map.roll_count = None;
    }
    if map
        .width_mm
        .is_some_and(|value| !value.is_finite() || value <= 0.0)
    {
        map.width_mm = None;
    }
    for node in &mut map.nodes {
        node.id = node.id.trim().to_ascii_lowercase();
        node.title = node.title.trim().to_string();
        node.role_code = node.role_code.trim().to_string();
        node.item_code = node.item_code.trim().to_string();
        node.qty_formula = node.qty_formula.trim().to_string();
        node.from_location = node.from_location.trim().to_string();
        node.to_location = node.to_location.trim().to_string();
        node.alternative_group_id = node.alternative_group_id.trim().to_string();
        node.alternative_group_label = node.alternative_group_label.trim().to_string();
        node.alternative_assigned_title = node.alternative_assigned_title.trim().to_string();
        if !node.x.is_finite() {
            node.x = 0.0;
        }
        if !node.y.is_finite() {
            node.y = 0.0;
        }
        if let Some(formula) = &mut node.formula {
            formula.target = formula.target.trim().to_string();
            formula.expression = formula.expression.trim().to_string();
        }
    }
    for edge in &mut map.edges {
        edge.from = edge.from.trim().to_ascii_lowercase();
        edge.to = edge.to.trim().to_ascii_lowercase();
        edge.branch = normalize_branch(&edge.branch);
    }
}

fn validate_map(map: &ProductionMapDefinition) -> Result<(), ProductionMapError> {
    if map.id.trim().is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    if map.product_code.trim().is_empty() {
        return Err(ProductionMapError::MissingProductCode);
    }
    if map.title.trim().is_empty() {
        return Err(ProductionMapError::MissingTitle);
    }
    if laminatsiya_rubber_too_large(map) {
        return Err(ProductionMapError::LaminatsiyaRubberTooLarge);
    }

    let mut ids = BTreeSet::new();
    let mut start_count = 0;
    let mut end_count = 0;
    for node in &map.nodes {
        if !ids.insert(node.id.as_str()) {
            return Err(ProductionMapError::DuplicateNode(node.id.clone()));
        }
        match node.kind {
            ProductionMapNodeKind::Start => start_count += 1,
            ProductionMapNodeKind::End => end_count += 1,
            ProductionMapNodeKind::Formula => {
                let Some(formula) = &node.formula else {
                    return Err(ProductionMapError::MissingFormulaExpression);
                };
                if formula.target.trim().is_empty() {
                    return Err(ProductionMapError::MissingFormulaTarget);
                }
                if formula.expression.trim().is_empty() {
                    return Err(ProductionMapError::MissingFormulaExpression);
                }
                validate_formula_target(&formula.target)?;
                validate_formula_expression(&formula.expression)?;
            }
            ProductionMapNodeKind::Condition => {
                let Some(formula) = &node.formula else {
                    return Err(ProductionMapError::MissingFormulaExpression);
                };
                if formula.expression.trim().is_empty() {
                    return Err(ProductionMapError::MissingFormulaExpression);
                }
                validate_condition_expression(&formula.expression)?;
            }
            ProductionMapNodeKind::Location => {}
            ProductionMapNodeKind::Material
            | ProductionMapNodeKind::Apparatus
            | ProductionMapNodeKind::KkProduct
            | ProductionMapNodeKind::Task
            | ProductionMapNodeKind::Wait
            | ProductionMapNodeKind::Output => {
                if !node.qty_formula.trim().is_empty() {
                    validate_formula_expression(&node.qty_formula)?;
                }
            }
        }
        validate_location_ref(&node.from_location)?;
        validate_location_ref(&node.to_location)?;
    }
    if start_count != 1 {
        return Err(ProductionMapError::MissingStart);
    }
    if end_count != 1 {
        return Err(ProductionMapError::MissingEnd);
    }
    for edge in &map.edges {
        if !ids.contains(edge.from.as_str()) {
            return Err(ProductionMapError::MissingEdgeNode(edge.from.clone()));
        }
        if !ids.contains(edge.to.as_str()) {
            return Err(ProductionMapError::MissingEdgeNode(edge.to.clone()));
        }
    }
    for node in &map.nodes {
        if node.kind != ProductionMapNodeKind::Condition {
            continue;
        }
        let mut has_true = false;
        let mut has_false = false;
        for edge in map.edges.iter().filter(|edge| edge.from == node.id) {
            match normalize_branch(&edge.branch).as_str() {
                "true" => has_true = true,
                "false" => has_false = true,
                _ => {}
            }
        }
        if !has_true || !has_false {
            return Err(ProductionMapError::MissingConditionBranch);
        }
    }
    Ok(())
}

fn laminatsiya_rubber_too_large(map: &ProductionMapDefinition) -> bool {
    let Some(width_mm) = map.width_mm.filter(|value| *value > 0.0) else {
        return false;
    };
    if pechat::rubber_size_from_width(width_mm) <= MAX_LAMINATSIYA_RUBBER_SIZE_MM {
        return false;
    }
    map.nodes.iter().any(|node| {
        matches!(
            node.kind,
            ProductionMapNodeKind::Apparatus | ProductionMapNodeKind::Task
        ) && is_laminatsiya_title(&node.title)
    })
}

fn normalize_branch(branch: &str) -> String {
    match branch.trim().to_ascii_lowercase().as_str() {
        "ha" | "yes" | "true" | "1" => "true".to_string(),
        "yo'q" | "yoq" | "no" | "false" | "0" => "false".to_string(),
        value => value.to_string(),
    }
}

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

fn topological_order(map: &ProductionMapDefinition) -> Result<Vec<String>, ProductionMapError> {
    let mut indegree = BTreeMap::<String, usize>::new();
    let mut outgoing = BTreeMap::<String, Vec<String>>::new();
    for node in &map.nodes {
        indegree.insert(node.id.clone(), 0);
        outgoing.insert(node.id.clone(), Vec::new());
    }
    for edge in &map.edges {
        *indegree
            .get_mut(&edge.to)
            .expect("validated edge target exists") += 1;
        outgoing
            .get_mut(&edge.from)
            .expect("validated edge source exists")
            .push(edge.to.clone());
    }

    let mut queue = indegree
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(id.clone()))
        .collect::<VecDeque<_>>();
    let mut order = Vec::new();
    while let Some(id) = queue.pop_front() {
        order.push(id.clone());
        for child in outgoing.get(&id).into_iter().flatten() {
            let count = indegree
                .get_mut(child)
                .expect("validated child exists in indegree map");
            *count = count.saturating_sub(1);
            if *count == 0 {
                queue.push_back(child.clone());
            }
        }
    }
    if order.len() != map.nodes.len() {
        return Err(ProductionMapError::Cycle);
    }
    Ok(order)
}

fn compile_node(
    order: usize,
    node: &ProductionMapNode,
) -> Result<ProductionMapOperation, ProductionMapError> {
    let mut args = BTreeMap::new();
    args.insert("title".to_string(), node.title.clone());
    if !node.role_code.is_empty() {
        args.insert("role_code".to_string(), node.role_code.clone());
    }
    if !node.item_code.is_empty() {
        args.insert("item_code".to_string(), node.item_code.clone());
    }
    if !node.qty_formula.is_empty() {
        args.insert("qty_formula".to_string(), node.qty_formula.clone());
    }
    if !node.from_location.is_empty() {
        args.insert("from_location".to_string(), node.from_location.clone());
    }
    if !node.to_location.is_empty() {
        args.insert("to_location".to_string(), node.to_location.clone());
    }
    if !node.alternative_group_id.is_empty() {
        args.insert(
            "alternative_group_id".to_string(),
            node.alternative_group_id.clone(),
        );
    }
    if !node.alternative_group_label.is_empty() {
        args.insert(
            "alternative_group_label".to_string(),
            node.alternative_group_label.clone(),
        );
    }
    if !node.alternative_assigned_title.is_empty() {
        args.insert(
            "alternative_assigned_title".to_string(),
            node.alternative_assigned_title.clone(),
        );
    }
    if let Some(value) = node.rezka_kadr_count {
        args.insert("rezka_kadr_count".to_string(), value.to_string());
    }
    if let Some(value) = node.rezka_label_length {
        args.insert("rezka_label_length".to_string(), value.to_string());
    }
    let op_code = match node.kind {
        ProductionMapNodeKind::Start => "start",
        ProductionMapNodeKind::Location => "warehouse_location",
        ProductionMapNodeKind::Material => "require_material",
        ProductionMapNodeKind::Apparatus => "apparatus",
        ProductionMapNodeKind::KkProduct => "kk_product",
        ProductionMapNodeKind::Formula => {
            let Some(formula) = &node.formula else {
                return Err(ProductionMapError::MissingFormulaExpression);
            };
            args.insert("target".to_string(), formula.target.clone());
            args.insert("expression".to_string(), formula.expression.clone());
            "calculate"
        }
        ProductionMapNodeKind::Condition => {
            let Some(formula) = &node.formula else {
                return Err(ProductionMapError::MissingFormulaExpression);
            };
            args.insert("expression".to_string(), formula.expression.clone());
            "condition"
        }
        ProductionMapNodeKind::Task => "create_task",
        ProductionMapNodeKind::Wait => "wait_dependency",
        ProductionMapNodeKind::Output => "produce_output",
        ProductionMapNodeKind::End => "end",
    };
    Ok(ProductionMapOperation {
        order,
        node_id: node.id.clone(),
        op_code: op_code.to_string(),
        args,
    })
}
