use std::collections::BTreeSet;

use super::super::apparatus::is_laminatsiya_title;
use super::super::formula::{
    validate_condition_expression, validate_formula_expression, validate_formula_target,
    validate_location_ref,
};
use super::super::pechat;
use super::super::types::*;
use super::normalize::normalize_branch;

const MAX_LAMINATSIYA_RUBBER_SIZE_MM: i64 = 1050;

pub(super) fn validate_map(map: &ProductionMapDefinition) -> Result<(), ProductionMapError> {
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
