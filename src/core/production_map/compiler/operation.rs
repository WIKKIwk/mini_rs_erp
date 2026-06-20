use std::collections::BTreeMap;

use super::super::types::*;

pub(super) fn compile_node(
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
