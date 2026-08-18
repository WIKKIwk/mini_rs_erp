use std::collections::BTreeMap;

use super::formula_parser::{FormulaParser, is_identifier, split_condition};
use super::types::ProductionMapError;

pub(super) fn validate_formula_target(target: &str) -> Result<(), ProductionMapError> {
    if is_identifier(target.trim()) {
        Ok(())
    } else {
        Err(ProductionMapError::InvalidFormulaTarget(target.to_string()))
    }
}

pub(super) fn validate_location_ref(location: &str) -> Result<(), ProductionMapError> {
    let location = location.trim();
    if location.is_empty() {
        return Ok(());
    }
    let valid = location.len() <= 120
        && location.chars().any(char::is_alphanumeric)
        && location.chars().all(|ch| {
            ch.is_alphanumeric()
                || ch.is_whitespace()
                || matches!(ch, '-' | '_' | '.' | '/' | '(' | ')')
        });
    if valid {
        Ok(())
    } else {
        Err(ProductionMapError::InvalidLocation(location.to_string()))
    }
}

pub(super) fn validate_formula_expression(expression: &str) -> Result<(), ProductionMapError> {
    let mut parser = FormulaParser::new(expression);
    parser.parse_expression()?;
    parser.skip_whitespace();
    if parser.is_eof() {
        Ok(())
    } else {
        Err(ProductionMapError::InvalidFormulaExpression(
            expression.to_string(),
        ))
    }
}

pub(super) fn validate_condition_expression(expression: &str) -> Result<(), ProductionMapError> {
    evaluate_condition(expression, &BTreeMap::new())
        .map(|_| ())
        .or_else(|error| {
            if matches!(error, ProductionMapError::UnknownFormulaVariable(_)) {
                Ok(())
            } else {
                Err(error)
            }
        })
}

pub(super) fn evaluate_formula(
    expression: &str,
    variables: &BTreeMap<String, f64>,
) -> Result<f64, ProductionMapError> {
    let mut parser = FormulaParser::new(expression);
    let value = parser.evaluate_expression(variables)?;
    parser.skip_whitespace();
    if parser.is_eof() {
        Ok(value)
    } else {
        Err(ProductionMapError::InvalidFormulaExpression(
            expression.to_string(),
        ))
    }
}

pub(super) fn evaluate_condition(
    expression: &str,
    variables: &BTreeMap<String, f64>,
) -> Result<bool, ProductionMapError> {
    if let Some((left, operator, right)) = split_condition(expression) {
        let left = evaluate_formula(left, variables)?;
        let right = evaluate_formula(right, variables)?;
        return match operator {
            ">" => Ok(left > right),
            ">=" => Ok(left >= right),
            "<" => Ok(left < right),
            "<=" => Ok(left <= right),
            "==" => Ok((left - right).abs() < f64::EPSILON),
            "!=" => Ok((left - right).abs() >= f64::EPSILON),
            _ => Err(ProductionMapError::InvalidFormulaExpression(
                expression.to_string(),
            )),
        };
    }
    Ok(evaluate_formula(expression, variables)? != 0.0)
}
