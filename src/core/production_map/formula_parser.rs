use std::collections::BTreeMap;

use super::types::ProductionMapError;

pub(super) fn split_condition(expression: &str) -> Option<(&str, &str, &str)> {
    let mut depth = 0usize;
    let bytes = expression.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => {
                for operator in [">=", "<=", "==", "!=", ">", "<"] {
                    if expression[index..].starts_with(operator) {
                        let left = expression[..index].trim();
                        let right = expression[index + operator.len()..].trim();
                        if !left.is_empty() && !right.is_empty() {
                            return Some((left, operator, right));
                        }
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

pub(super) fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

pub(super) struct FormulaParser<'a> {
    input: &'a str,
    position: usize,
}

impl<'a> FormulaParser<'a> {
    pub(super) fn new(input: &'a str) -> Self {
        Self { input, position: 0 }
    }

    pub(super) fn parse_expression(&mut self) -> Result<(), ProductionMapError> {
        self.parse_term()?;
        loop {
            self.skip_whitespace();
            if self.consume('+') || self.consume('-') {
                self.parse_term()?;
            } else {
                return Ok(());
            }
        }
    }

    pub(super) fn evaluate_expression(
        &mut self,
        variables: &BTreeMap<String, f64>,
    ) -> Result<f64, ProductionMapError> {
        let mut value = self.evaluate_term(variables)?;
        loop {
            self.skip_whitespace();
            if self.consume('+') {
                value += self.evaluate_term(variables)?;
            } else if self.consume('-') {
                value -= self.evaluate_term(variables)?;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_term(&mut self) -> Result<(), ProductionMapError> {
        self.parse_factor()?;
        loop {
            self.skip_whitespace();
            if self.consume('*') || self.consume('/') {
                self.parse_factor()?;
            } else {
                return Ok(());
            }
        }
    }

    fn evaluate_term(
        &mut self,
        variables: &BTreeMap<String, f64>,
    ) -> Result<f64, ProductionMapError> {
        let mut value = self.evaluate_factor(variables)?;
        loop {
            self.skip_whitespace();
            if self.consume('*') {
                value *= self.evaluate_factor(variables)?;
            } else if self.consume('/') {
                let divisor = self.evaluate_factor(variables)?;
                if divisor == 0.0 {
                    return Err(ProductionMapError::FormulaDivisionByZero);
                }
                value /= divisor;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_factor(&mut self) -> Result<(), ProductionMapError> {
        self.skip_whitespace();
        if self.consume('-') {
            return self.parse_factor();
        }
        if self.consume('(') {
            self.parse_expression()?;
            self.skip_whitespace();
            return if self.consume(')') {
                Ok(())
            } else {
                self.invalid()
            };
        }
        if self.parse_identifier() || self.parse_number() {
            Ok(())
        } else {
            self.invalid()
        }
    }

    fn evaluate_factor(
        &mut self,
        variables: &BTreeMap<String, f64>,
    ) -> Result<f64, ProductionMapError> {
        self.skip_whitespace();
        if self.consume('-') {
            return Ok(-self.evaluate_factor(variables)?);
        }
        if self.consume('(') {
            let value = self.evaluate_expression(variables)?;
            self.skip_whitespace();
            return if self.consume(')') {
                Ok(value)
            } else {
                self.invalid()
            };
        }
        if let Some(identifier) = self.read_identifier() {
            return variables
                .get(&identifier)
                .copied()
                .ok_or(ProductionMapError::UnknownFormulaVariable(identifier));
        }
        if let Some(number) = self.read_number() {
            return Ok(number);
        }
        self.invalid()
    }

    fn parse_identifier(&mut self) -> bool {
        self.read_identifier().is_some()
    }

    fn read_identifier(&mut self) -> Option<String> {
        let start = self.position;
        while let Some(ch) = self.peek() {
            if self.position == start {
                if ch.is_ascii_alphabetic() || ch == '_' {
                    self.position += ch.len_utf8();
                } else {
                    break;
                }
            } else if ch.is_ascii_alphanumeric() || ch == '_' {
                self.position += ch.len_utf8();
            } else {
                break;
            }
        }
        (self.position > start).then(|| self.input[start..self.position].to_string())
    }

    fn parse_number(&mut self) -> bool {
        self.read_number().is_some()
    }

    fn read_number(&mut self) -> Option<f64> {
        let start = self.position;
        while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
            self.position += 1;
        }
        if self.consume('.') {
            while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
                self.position += 1;
            }
        }
        (self.position > start)
            .then(|| self.input[start..self.position].parse::<f64>().ok())
            .flatten()
    }

    pub(super) fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(ch) if ch.is_ascii_whitespace()) {
            self.position += 1;
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.position += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    pub(super) fn is_eof(&self) -> bool {
        self.position >= self.input.len()
    }

    fn invalid<T>(&self) -> Result<T, ProductionMapError> {
        Err(ProductionMapError::InvalidFormulaExpression(
            self.input.to_string(),
        ))
    }
}
