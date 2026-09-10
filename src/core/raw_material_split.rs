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
mod tests {
    use super::*;
    #[test]
    fn raw_material_split_recorded_issue_requires_unchanged_measurements() {
        let mut report = SplitIssueCreate {
            command: sample(),
            note: "Tarozi xatosi".into(),
        };
        report.command.waste_kg = "1".into();
        let mut command = report.command.clone();
        command.issue_id = Some("raw-issue:test".into());
        command.request_id = "split-complete-01".into();
        command.waste_kg = "1.000000".into();
        assert!(command.validate().is_err());
        assert!(command.validate_recorded_issue(&report).is_ok());
        command.outputs[0].gross_kg = Some("41".into());
        command.outputs[0].kg = "40".into();
        assert!(command.validate_recorded_issue(&report).is_err());
        command = report.command.clone();
        assert!(command.validate_recorded_issue(&report).is_err()); // No saved issue ID.
        command.issue_id = Some("raw-issue:test".into());
        command.expected_revision = "2".into();
        assert!(command.validate_recorded_issue(&report).is_err());
        for waste in ["0", "2.000001", "bad"] {
            report.command.waste_kg = waste.into();
            command = report.command.clone();
            command.issue_id = Some("raw-issue:test".into());
            assert_eq!(
                command.validate_recorded_issue(&report).is_ok(),
                waste != "bad"
            );
        }
    }
    #[test]
    fn raw_material_split_issues_record_exact_discrepancy_without_accepting_split() {
        let mut issue = SplitIssueCreate {
            command: sample(),
            note: "Tarozi qayta tekshirilsin".into(),
        };
        for (waste, kind, difference) in [
            ("1", "missing_weight", Some(1_000_000)),
            ("0", "zero_waste", Some(2_000_000)),
            ("00,000000", "zero_waste", Some(2_000_000)),
            ("2.000001", "excess_weight", Some(-1)),
            ("1,999999", "missing_weight", Some(1)),
            ("", "invalid_waste", None),
            ("bad", "invalid_waste", None),
            ("-1", "invalid_waste", None),
            ("1.", "invalid_waste", None),
        ] {
            issue.command.waste_kg = waste.into();
            let balance = issue.validate().unwrap();
            assert_eq!(balance.kind, kind, "{waste}");
            assert_eq!(balance.difference, difference, "{waste}");
            assert_eq!(balance.output, 98_000_000);
            assert!(issue.command.validate().is_err());
        }
        assert_eq!(split_issue_decimal(-1), "-0.000001");
        issue.command.waste_kg = "2".into();
        assert!(issue.validate().is_err()); // No discrepancy to report.
        assert!(issue.command.validate().is_ok());
        issue.command.waste_kg = "0".into();
        issue.note.clear();
        assert!(issue.validate().is_err());
        issue.note = "Reason".into();
        issue.command.outputs[0].kg = "40".into();
        assert!(issue.validate().is_err()); // Caller cannot invent net weight.
    }
    pub fn sample() -> SplitCreate {
        SplitCreate {
            request_id: "split-test-01".into(),
            issue_id: None,
            source_barcode: "parent".into(),
            expected_revision: "1.000000".into(),
            expected_kg: "100".into(),
            expected_width_mm: "1000".into(),
            expected_micron: "20".into(),
            waste_kg: "2".into(),
            outputs: vec![
                SplitOutput {
                    kg: "39".into(),
                    width_mm: "400".into(),
                    gross_kg: Some("40".into()),
                    bobina_kg: Some("1".into()),
                    length_m: None,
                },
                SplitOutput {
                    kg: "59".into(),
                    width_mm: "600".into(),
                    gross_kg: Some("60".into()),
                    bobina_kg: Some("1".into()),
                    length_m: None,
                },
            ],
        }
    }
    #[test]
    fn raw_material_split_minimum_width_and_exact_cut_plan() {
        let mut input = sample();
        input.expected_width_mm = "700".into();
        for width in ["350", "354.999999", "355"] {
            input.outputs[0].width_mm = width.into();
            input.outputs[1].width_mm = width.into();
            assert!(input.validate().is_err(), "700 cannot yield 2 x {width}");
        }
        input.expected_width_mm = "710".into();
        assert!(input.validate().is_ok());
        input.outputs[1].width_mm = "355.000001".into();
        assert!(input.validate().is_err());
        input.outputs.truncate(1);
        input.outputs[0].kg = "98".into();
        input.outputs[0].gross_kg = Some("99".into());
        input.expected_width_mm = "700".into();
        for width in ["355", "400", "700"] {
            input.outputs[0].width_mm = width.into();
            assert!(input.validate().is_ok());
        }
        input.expected_width_mm = "1000".into();
        input.outputs[0].width_mm = "645".into();
        assert!(input.validate().is_err()); // 355 mm is still usable.
        input.outputs[0].width_mm = "645.000001".into();
        assert!(input.validate().is_ok());
    }
    #[test]
    fn raw_material_split_exact_balance() {
        let mut input = sample();
        assert_eq!(
            input.validate().unwrap(),
            (
                100_000_000,
                2_000_000,
                vec![
                    WeighedOutput {
                        kg: 39_000_000,
                        width_mm: 400_000_000,
                        gross_kg: 40_000_000,
                        bobina_kg: 1_000_000,
                        length_m: None,
                    },
                    WeighedOutput {
                        kg: 59_000_000,
                        width_mm: 600_000_000,
                        gross_kg: 60_000_000,
                        bobina_kg: 1_000_000,
                        length_m: None,
                    },
                ]
            )
        );
        input.outputs[0].kg = "39.000001".into();
        assert!(input.validate().is_err());
        input.outputs[0].kg = "38.999999".into();
        input.outputs[0].gross_kg = Some("39.999999".into());
        input.waste_kg = "2.000001".into();
        assert!(input.validate().is_ok());
    }
    #[test]
    fn raw_material_split_rejects_invalid_sizes_and_quantities() {
        for invalid in ["NaN", "-1", "0", "1e2", "1.0000001", "9999999999999"] {
            let mut input = sample();
            input.outputs[0].kg = invalid.into();
            assert!(input.validate().is_err(), "{invalid}");
        }
        let mut input = sample();
        input.outputs[0].width_mm = "1001".into();
        assert!(input.validate().is_err());
        input = sample();
        input.outputs.clear();
        assert!(input.validate().is_err());
    }
    #[test]
    fn raw_material_split_requires_positive_waste_even_with_balanced_net() {
        let mut input = sample();
        input.outputs[0].kg = "41".into();
        input.outputs[0].gross_kg = Some("42".into());
        for waste in ["", "0", "0.0", "0.000000", "00", "-1", "NaN"] {
            input.waste_kg = waste.into();
            assert!(input.validate().is_err(), "waste={waste}");
        }
        input.waste_kg = "0.000001".into();
        input.outputs[0].kg = "40.999999".into();
        input.outputs[0].gross_kg = Some("41.999999".into());
        assert!(input.validate().is_ok());
    }
    #[test]
    fn raw_material_split_requires_measured_weights_and_explicit_waste() {
        let mut input = sample();
        input.waste_kg.clear();
        assert!(input.validate().is_err());
        for missing in ["waste_kg", "outputs"] {
            let mut json = serde_json::to_value(sample()).unwrap();
            json.as_object_mut().unwrap().remove(missing);
            assert!(serde_json::from_value::<SplitCreate>(json).is_err());
        }
        for invalid in [
            None,
            Some(""),
            Some("-1"),
            Some("40"),
            Some("NaN"),
            Some("1.000001"),
        ] {
            let mut input = sample();
            input.outputs[0].bobina_kg = invalid.map(str::to_owned);
            assert!(input.validate().is_err());
        }
        let mut input = sample();
        input.outputs[0].bobina_kg = Some("0".into());
        input.outputs[0].gross_kg = Some("39".into());
        assert!(input.validate().is_ok());
        input.outputs[0].gross_kg = None;
        assert!(input.validate().is_err());
    }
    #[test]
    fn raw_material_split_name_uses_child_width_and_inherited_micron() {
        for name in [
            "BOPP METAL 700/12",
            "BOPP METAL 700 / 12",
            "BOPP METAL 350/12",
            "BOPP METAL",
        ] {
            assert_eq!(
                split_item_name(name, "350.000000", "12"),
                "BOPP METAL 350/12"
            );
        }
        assert_eq!(
            split_item_name("Plyonka 3 qatlam 700/12", "349.5", "12.500000"),
            "Plyonka 3 qatlam 349.5/12.5"
        );
    }
    #[test]
    fn raw_material_split_length_m_validation() {
        let mut input = sample();
        input.outputs[0].length_m = Some("1000".into());
        input.outputs[1].length_m = Some("2500.5".into());
        let (_, _, outputs) = input.validate().unwrap();
        assert_eq!(outputs[0].length_m, Some(1_000_000_000));
        assert_eq!(outputs[1].length_m, Some(2_500_500_000));

        input.outputs[0].length_m = Some("0".into());
        assert!(input.validate().is_err());
        input.outputs[0].length_m = Some("-10".into());
        assert!(input.validate().is_err());
    }
}
