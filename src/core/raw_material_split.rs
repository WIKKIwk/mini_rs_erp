//! Independent, pre-production roll splitting. No order/production Rezka state.
pub use crate::core::preparation::decimal_text;
use serde::{Deserialize, Serialize};

const MIN_OUTPUT_WIDTH: i64 = 355_000_000;

#[derive(Debug, thiserror::Error)]
pub enum SplitError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("Rulon mavjud emas yoki ombor sizga biriktirilmagan")]
    Forbidden,
    #[error("Homashyo rezkasini saqlashda xatolik")]
    StoreFailed,
}
pub fn quantity(raw: &str, zero: bool) -> Result<i64, SplitError> {
    if zero
        && matches!(
            raw.trim(),
            "0" | "0.0" | "0.00" | "0.000" | "0.0000" | "0.00000" | "0.000000"
        )
    {
        return Ok(0);
    }
    crate::core::preparation::decimal(raw)
        .map_err(|_| SplitError::Invalid("Musbat son kiriting (ko‘pi bilan 6 kasr xona)"))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitOutput {
    pub kg: String,
    pub width_mm: String,
    // Optional only for replaying commands saved before weighed outputs existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gross_kg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bobina_kg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_m: Option<String>,
}
#[derive(Debug, PartialEq, Eq)]
pub struct WeighedOutput {
    pub kg: i64,
    pub width_mm: i64,
    pub gross_kg: i64,
    pub bobina_kg: i64,
    pub length_m: Option<i64>,
}

pub fn split_item_name(name: &str, width: &str, micron: &str) -> String {
    let name = name.trim();
    let mut base = name;
    if let Some((left, right)) = name.rsplit_once('/') {
        let left = left.trim_end();
        let start = left
            .rfind(|c: char| !c.is_ascii_digit() && c != '.' && c != ',')
            .map_or(0, |i| i + left[i..].chars().next().unwrap().len_utf8());
        if quantity(&left[start..].replace(',', "."), false).is_ok()
            && quantity(&right.trim().replace(',', "."), false).is_ok()
        {
            base = left[..start].trim_end();
        }
    }
    let compact = |value: &str| {
        if value.contains('.') {
            value.trim_end_matches('0').trim_end_matches('.')
        } else {
            value
        }
        .to_owned()
    };
    format!("{} {}/{}", base, compact(width), compact(micron))
        .trim()
        .to_owned()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitCreate {
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_id: Option<String>,
    pub source_barcode: String,
    pub expected_revision: String,
    pub expected_kg: String,
    pub expected_width_mm: String,
    pub expected_micron: String,
    pub waste_kg: String,
    pub outputs: Vec<SplitOutput>,
}
impl SplitCreate {
    /// Only the store may supply a previously persisted, owner-scoped report.
    pub fn validate_recorded_issue(
        &self,
        report: &SplitIssueCreate,
    ) -> Result<(i64, i64, Vec<WeighedOutput>), SplitError> {
        let balance = report.validate()?;
        let waste = issue_waste(&self.waste_kg)
            .ok_or(SplitError::Invalid("Atxot kg ni son bilan kiriting"))?;
        let (source, outputs) = self.validate_outputs()?;
        let (_, reported_outputs) = report.command.validate_outputs()?;
        if self
            .issue_id
            .as_deref()
            .is_none_or(|id| id.is_empty() || id.len() > 128)
            || source != balance.source
            || Some(waste) != balance.waste
            || outputs != reported_outputs
            || !self
                .source_barcode
                .trim()
                .eq_ignore_ascii_case(report.command.source_barcode.trim())
            || self.expected_revision != report.command.expected_revision
            || quantity(&self.expected_width_mm, false)?
                != quantity(&report.command.expected_width_mm, false)?
            || quantity(&self.expected_micron, false)?
                != quantity(&report.command.expected_micron, false)?
        {
            return Err(SplitError::Conflict(
                "Rulon yoki vaznlar muammo qaydiga mos emas. Muammoni qayta saqlang",
            ));
        }
        // Keep the stock total within the existing NUMERIC(18,6) contract.
        quantity(&split_issue_decimal(balance.output), false)?;
        Ok((source, waste, outputs))
    }

    pub fn validate(&self) -> Result<(i64, i64, Vec<WeighedOutput>), SplitError> {
        let (source, outputs) = self.validate_outputs()?;
        if self.waste_kg.trim().is_empty() {
            return Err(SplitError::Invalid("Atxot kg ni kiriting (0 dan katta)"));
        }
        let waste = quantity(&self.waste_kg, true)?;
        if waste == 0 {
            return Err(SplitError::Invalid("Atxot 0 dan katta bo‘lishi kerak"));
        }
        let total = outputs.iter().map(|o| o.kg as i128).sum::<i128>() + waste as i128;
        if total != source as i128 {
            return Err(SplitError::Invalid(
                "Chiqish rulonlari + chiqindi asl rulon kg iga aniq teng bo‘lishi kerak",
            ));
        }
        Ok((source, waste, outputs))
    }

    fn validate_outputs(&self) -> Result<(i64, Vec<WeighedOutput>), SplitError> {
        if !(8..=128).contains(&self.request_id.len())
            || !self
                .request_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_:".contains(&c))
            || self.source_barcode.trim().is_empty()
            || self.source_barcode.len() > 256
            || self.expected_revision.is_empty()
            || self.expected_revision.len() > 64
        {
            return Err(SplitError::Invalid(
                "So‘rov identifikatori yoki QR noto‘g‘ri",
            ));
        }
        let source = quantity(&self.expected_kg, false)?;
        let width = quantity(&self.expected_width_mm, false)?;
        quantity(&self.expected_micron, false)?;
        if self.outputs.is_empty() || self.outputs.len() > 100 {
            return Err(SplitError::Invalid("1–100 ta chiqish ruloni kiriting"));
        }
        let mut used_width = 0_i128;
        let mut outputs = Vec::new();
        for line in &self.outputs {
            let kg = quantity(&line.kg, false)?;
            let gross_kg = quantity(
                line.gross_kg.as_deref().ok_or(SplitError::Invalid(
                    "Har bir rulonning brutto kg ini kiriting",
                ))?,
                false,
            )?;
            let bobina_kg = quantity(
                line.bobina_kg.as_deref().ok_or(SplitError::Invalid(
                    "Har bir rulonning babina kg ini kiriting",
                ))?,
                true,
            )?;
            if gross_kg <= bobina_kg || gross_kg - bobina_kg != kg {
                return Err(SplitError::Invalid(
                    "Netto brutto − babina kg ga teng bo‘lishi kerak",
                ));
            }
            let mm = quantity(&line.width_mm, false)?;
            if mm < MIN_OUTPUT_WIDTH {
                return Err(SplitError::Invalid(
                    "Rulon eni kamida 355 mm bo‘lishi kerak",
                ));
            }
            if mm > width {
                return Err(SplitError::Invalid(
                    "Chiqish eni asl rulon enidan katta bo‘lmasin",
                ));
            }
            used_width += mm as i128;
            if used_width > width as i128 {
                return Err(SplitError::Invalid(
                    "Enlar yig‘indisi asl rulon enidan oshmasin",
                ));
            }
            let length_m = match line.length_m.as_deref() {
                Some(raw) if !raw.trim().is_empty() => Some(quantity(raw, false)?),
                _ => None,
            };
            outputs.push(WeighedOutput {
                kg,
                width_mm: mm,
                gross_kg,
                bobina_kg,
                length_m,
            });
        }
        if width as i128 - used_width >= MIN_OUTPUT_WIDTH as i128 {
            return Err(SplitError::Invalid(
                "Qolgan yaroqli en uchun ham rulon kiriting",
            ));
        }
        Ok((source, outputs))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitIssueCreate {
    pub command: SplitCreate,
    pub note: String,
}

pub struct SplitIssueBalance {
    pub source: i64,
    pub output: i128,
    pub waste: Option<i64>,
    pub difference: Option<i128>,
    pub kind: &'static str,
}

/// Signed, exact six-place quantities, including totals of up to 100 rolls.
pub fn split_issue_decimal(value: i128) -> String {
    let abs = value.abs();
    format!(
        "{}{}.{:06}",
        if value < 0 { "-" } else { "" },
        abs / 1_000_000,
        abs % 1_000_000
    )
}

fn issue_waste(raw: &str) -> Option<i64> {
    let normalized = raw.trim().replace(',', ".");
    let (whole, fraction) = match normalized.split_once('.') {
        Some((_, "")) => return None,
        Some(parts) => parts,
        None => (normalized.as_str(), ""),
    };
    if whole.is_empty()
        || !whole.bytes().all(|c| c.is_ascii_digit())
        || fraction.len() > 6
        || !fraction.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let whole = whole.trim_start_matches('0');
    quantity(
        &format!(
            "{}.{fraction:0<6}",
            if whole.is_empty() { "0" } else { whole }
        ),
        true,
    )
    .ok()
}

impl SplitIssueCreate {
    pub fn validate(&self) -> Result<SplitIssueBalance, SplitError> {
        if self.command.issue_id.is_some() {
            return Err(SplitError::Invalid(
                "Muammo qaydi boshqa muammoga bog‘lanmasin",
            ));
        }
        if self.note.trim().is_empty() || self.note.chars().count() > 1000 {
            return Err(SplitError::Invalid(
                "Farq sababini yozing (ko‘pi bilan 1000 belgi)",
            ));
        }
        if self.command.waste_kg.len() > 64 {
            return Err(SplitError::Invalid("Atxot maydoni juda uzun"));
        }
        let (source, outputs) = self.command.validate_outputs()?;
        let output = outputs.iter().map(|o| o.kg as i128).sum::<i128>();
        let waste = issue_waste(&self.command.waste_kg);
        let difference = waste.map(|w| source as i128 - output - w as i128);
        let kind = match (waste, difference) {
            (None, _) => "invalid_waste",
            (Some(0), _) => "zero_waste",
            (_, Some(d)) if d > 0 => "missing_weight",
            (_, Some(d)) if d < 0 => "excess_weight",
            _ => {
                return Err(SplitError::Invalid(
                    "Hisob teng, qayd etiladigan vazn xatosi yo‘q",
                ));
            }
        };
        Ok(SplitIssueBalance {
            source,
            output,
            waste,
            difference,
            kind,
        })
    }
}
#[cfg(test)]
#[path = "raw_material_split_tests.rs"]
mod tests;
