//! Bulk raw-material preparation. Quantities and percentages use six decimal
//! places, like the ERP quantity columns; no floating-point stock arithmetic.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SCALE: i64 = 1_000_000;
const MAX: i64 = 999_999_999_999_999_999;

#[derive(Debug, thiserror::Error)]
pub enum PreparationError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("Ombor yoki homashyo sizga biriktirilmagan")]
    Forbidden,
    #[error("Homashyo qoldig‘i yetarli emas")]
    Insufficient,
    #[error("Tayyorlov ma’lumotlarini saqlashda xatolik")]
    StoreFailed,
}

pub fn decimal(value: &str) -> Result<i64, PreparationError> {
    let value = value.trim();
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty()
        || !whole.bytes().all(|c| c.is_ascii_digit())
        || fraction.len() > 6
        || !fraction.bytes().all(|c| c.is_ascii_digit())
    {
        return Err(PreparationError::Invalid(
            "Musbat son kiriting (ko‘pi bilan 6 kasr xona)",
        ));
    }
    let whole = whole.parse::<i64>().ok();
    let fraction = format!("{fraction:0<6}").parse::<i64>().unwrap_or(0);
    whole
        .and_then(|n| n.checked_mul(SCALE))
        .and_then(|n| n.checked_add(fraction))
        .filter(|n| *n > 0 && *n <= MAX)
        .ok_or(PreparationError::Invalid(
            "Miqdor ruxsat etilgan chegaradan tashqarida",
        ))
}

pub fn decimal_text(value: i64) -> String {
    format!("{}.{:06}", value / SCALE, value % SCALE)
}

pub fn required_kg(order: i64, percent: i64) -> Result<i64, PreparationError> {
    if order <= 0 || order > MAX || percent <= 0 || percent > 100 * SCALE {
        return Err(PreparationError::Invalid(
            "Foiz 0 dan katta va 100 dan oshmasligi kerak",
        ));
    }
    // Half-up at one micro-KG, using a wide intermediate to prevent overflow.
    let result =
        ((order as i128 * percent as i128 + 50 * SCALE as i128) / (100 * SCALE as i128)) as i64;
    if result == 0 {
        return Err(PreparationError::Invalid(
            "Hisoblangan sarf 0.000001 kg dan kichik",
        ));
    }
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialCreate {
    pub request_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptCreate {
    pub request_id: String,
    pub item_code: String,
    pub warehouse: String,
    pub kg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumptionLine {
    pub item_code: String,
    pub percent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumptionCreate {
    pub request_id: String,
    pub warehouse: String,
    pub order_id: String,
    pub expected_order_kg: String,
    pub lines: Vec<ConsumptionLine>,
}

impl ConsumptionCreate {
    pub fn quantities(&self, kg: i64) -> Result<Vec<(String, i64, i64)>, PreparationError> {
        if self.lines.is_empty() || self.lines.len() > 100 {
            return Err(PreparationError::Invalid("1–100 ta homashyo tanlang"));
        }
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        for line in &self.lines {
            let code = line.item_code.trim();
            if code.is_empty() || !seen.insert(code.to_string()) {
                return Err(PreparationError::Invalid(
                    "Homashyo bo‘sh yoki takrorlangan",
                ));
            }
            let percent = decimal(&line.percent)?;
            result.push((code.to_string(), percent, required_kg(kg, percent)?));
        }
        // Every transaction locks materials/lots in the same order.
        result.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(result)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaLine {
    pub item_code: String,
    pub percent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaUpsert {
    pub product_code: String,
    pub lines: Vec<FormulaLine>,
}

impl FormulaUpsert {
    pub fn product_key(&self) -> Result<String, PreparationError> {
        let code = self.product_code.trim().to_string();
        if code.is_empty() || code.chars().count() > 160 {
            return Err(PreparationError::Invalid("Mahsulot kodi noto‘g‘ri"));
        }
        Ok(code)
    }

    /// Validated (item_code, percent) pairs sorted alphabetically by code.
    /// Display order by name is applied by the caller once names resolve.
    pub fn normalized_lines(&self) -> Result<Vec<(String, i64)>, PreparationError> {
        if self.lines.is_empty() || self.lines.len() > 100 {
            return Err(PreparationError::Invalid("1–100 ta seriya tanlang"));
        }
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        for line in &self.lines {
            let code = line.item_code.trim();
            if code.is_empty() || !seen.insert(code.to_string()) {
                return Err(PreparationError::Invalid(
                    "Seriya bo‘sh yoki takrorlangan",
                ));
            }
            let percent = decimal(&line.percent)?;
            if percent > 100 * SCALE {
                return Err(PreparationError::Invalid(
                    "Foiz 100 dan oshmasligi kerak",
                ));
            }
            result.push((code.to_string(), percent));
        }
        result.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_decimal_and_rounding_are_exact() {
        assert_eq!(
            required_kg(decimal("1250.5").unwrap(), decimal("2.5").unwrap()).unwrap(),
            31_262_500
        );
        assert_eq!(required_kg(1, 50_000_000).unwrap(), 1);
        assert!(required_kg(1, 1).is_err());
        assert_eq!(decimal_text(31_262_500), "31.262500");
        assert_eq!(required_kg(MAX, 100 * SCALE).unwrap(), MAX);
        for value in [
            "NaN",
            "inf",
            "1e3",
            "-1",
            "0",
            "1.0000001",
            "1..2",
            "9999999999999",
            "",
        ] {
            assert!(decimal(value).is_err(), "{value}");
        }
    }
    #[test]
    fn preparation_recipe_rejects_duplicate_and_invalid_percent() {
        let mut input = ConsumptionCreate {
            request_id: "test".into(),
            warehouse: "W".into(),
            order_id: "O".into(),
            expected_order_kg: "100".into(),
            lines: vec![ConsumptionLine {
                item_code: "A".into(),
                percent: "2".into(),
            }],
        };
        assert_eq!(input.quantities(100 * SCALE).unwrap()[0].2, 2 * SCALE);
        input.lines.push(input.lines[0].clone());
        assert!(input.quantities(100 * SCALE).is_err());
        input.lines.pop();
        input.lines[0].percent = "100.000001".into();
        assert!(input.quantities(100 * SCALE).is_err());
    }
    #[test]
    fn preparation_formula_sorts_alphabetically_and_validates() {
        let input = FormulaUpsert {
            product_code: "  PC-1 ".into(),
            lines: vec![
                FormulaLine {
                    item_code: "B".into(),
                    percent: "30".into(),
                },
                FormulaLine {
                    item_code: "A".into(),
                    percent: "70".into(),
                },
            ],
        };
        assert_eq!(input.product_key().unwrap(), "PC-1");
        let lines = input.normalized_lines().unwrap();
        assert_eq!(lines[0].0, "A");
        assert_eq!(lines[1].0, "B");
        let dup = FormulaUpsert {
            product_code: "PC-1".into(),
            lines: vec![
                FormulaLine {
                    item_code: "A".into(),
                    percent: "50".into(),
                },
                FormulaLine {
                    item_code: "A".into(),
                    percent: "50".into(),
                },
            ],
        };
        assert!(dup.normalized_lines().is_err());
        let bad = FormulaUpsert {
            product_code: "".into(),
            lines: vec![FormulaLine {
                item_code: "A".into(),
                percent: "10".into(),
            }],
        };
        assert!(bad.product_key().is_err());
    }
}
