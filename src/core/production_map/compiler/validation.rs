use std::collections::BTreeSet;

use super::super::formula::{
    validate_condition_expression, validate_formula_expression, validate_formula_target,
    validate_location_ref,
};
use super::super::types::*;
use super::normalize::normalize_branch;
use crate::core::apparatus_standard::ApparatusId;

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
            ProductionMapNodeKind::Apparatus => {
                validate_apparatus_identity(map, node)?;
                if !node.qty_formula.trim().is_empty() {
                    validate_formula_expression(&node.qty_formula)?;
                }
            }
            ProductionMapNodeKind::Material
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

fn validate_apparatus_identity(
    map: &ProductionMapDefinition,
    node: &ProductionMapNode,
) -> Result<(), ProductionMapError> {
    let apparatus_id = node.apparatus_id.trim();
    if apparatus_id.is_empty() || ApparatusId::new(apparatus_id).is_err() {
        return Err(ProductionMapError::MissingId);
    }

    let assigned_id = node.alternative_assigned_apparatus_id.trim();
    if assigned_id.is_empty() {
        if !node.alternative_assigned_title.trim().is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        return Ok(());
    }
    if ApparatusId::new(assigned_id).is_err() {
        return Err(ProductionMapError::MissingId);
    }
    let group_id = node.alternative_group_id.trim();
    if group_id.is_empty()
        || !map.nodes.iter().any(|candidate| {
            candidate.kind == ProductionMapNodeKind::Apparatus
                && candidate.alternative_group_id.trim() == group_id
                && candidate.apparatus_id.trim() == assigned_id
        })
    {
        return Err(ProductionMapError::MissingId);
    }
    Ok(())
}
