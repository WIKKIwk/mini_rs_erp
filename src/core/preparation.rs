//! Bulk raw-material preparation. Quantities and percentages use six decimal
//! places, like the ERP quantity columns; no floating-point stock arithmetic.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::core::auth::models::PrincipalRole;

pub const SCALE: i64 = 1_000_000;
pub const MAX_PRODUCTION_NOTE_CHARS: usize = 2000;
pub const MAX_PRODUCTION_NOTE_ROLLS: usize = 100;
pub(crate) const MAX_PREPARATION_BATCH_SELECTION: usize = 100;
const MAX: i64 = 999_999_999_999_999_999;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparationNoteViewerRole {
    RezkaMaster,
    HomashyoRezkachi,
    RezkaWorker,
    Aparatchi,
    TayyorlovMasteri,
    MaterialTaminotchi,
    Werka,
    Metalayzer,
    Qolipchi,
    Brigader,
    Admin,
}

impl PreparationNoteViewerRole {
    pub fn for_principal(role: PrincipalRole) -> Option<Self> {
        Some(match role {
            PrincipalRole::RezkaMaster => Self::RezkaMaster,
            PrincipalRole::HomashyoRezkachi => Self::HomashyoRezkachi,
            PrincipalRole::Aparatchi => Self::Aparatchi,
            PrincipalRole::TayyorlovMasteri => Self::TayyorlovMasteri,
            PrincipalRole::MaterialTaminotchi => Self::MaterialTaminotchi,
            PrincipalRole::Werka => Self::Werka,
            PrincipalRole::Metalayzer => Self::Metalayzer,
            PrincipalRole::Qolipchi => Self::Qolipchi,
            PrincipalRole::Brigader => Self::Brigader,
            PrincipalRole::Admin => Self::Admin,
            _ => return None,
        })
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::RezkaMaster => "rezka_master",
            Self::HomashyoRezkachi => "homashyo_rezkachi",
            Self::RezkaWorker => "rezka_worker",
            Self::Aparatchi => "aparatchi",
            Self::TayyorlovMasteri => "tayyorlov_masteri",
            Self::MaterialTaminotchi => "material_taminotchi",
            Self::Werka => "werka",
            Self::Metalayzer => "metalayzer",
            Self::Qolipchi => "qolipchi",
            Self::Brigader => "brigader",
            Self::Admin => "admin",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreparationNoteAudience {
    All,
    Roles { roles: BTreeSet<PreparationNoteViewerRole> },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationBatchNoteTarget {
    pub batch_id: String,
    pub expected_revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationRollNoteTarget {
    pub receipt_name: String,
    pub expected_revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreparationOutputNoteUpdate {
    Batch {
        batch_id: String,
        expected_revision: i64,
        text: Option<String>,
        audience: Option<PreparationNoteAudience>,
    },
    Batches {
        batches: Vec<PreparationBatchNoteTarget>,
        text: Option<String>,
        audience: Option<PreparationNoteAudience>,
    },
    Rolls {
        rolls: Vec<PreparationRollNoteTarget>,
        text: Option<String>,
        audience: Option<PreparationNoteAudience>,
    },
}

pub struct PreparationNoteChanges<'a> {
    pub target_kind: &'static str,
    pub targets: BTreeMap<&'a str, i64>,
    pub text: Option<&'a str>,
    pub audience: Option<&'a PreparationNoteAudience>,
}

impl PreparationOutputNoteUpdate {
    pub fn changes(&self) -> Result<PreparationNoteChanges<'_>, PreparationError> {
        let (target_kind, targets, text, audience) = match self {
            Self::Batch { batch_id, expected_revision, text, audience } => {
                ("batch", BTreeMap::from([(batch_id.trim(), *expected_revision)]), text, audience)
            }
            Self::Batches { batches, text, audience } => {
                let selected: BTreeMap<_, _> = batches
                    .iter()
                    .map(|batch| (batch.batch_id.trim(), batch.expected_revision))
                    .collect();
                if !(1..=MAX_PREPARATION_BATCH_SELECTION).contains(&batches.len())
                    || selected.len() != batches.len()
                {
                    return Err(PreparationError::Invalid("1 dan 100 tagacha turli batch tanlang"));
                }
                ("batch", selected, text, audience)
            }
            Self::Rolls { rolls, text, audience } => {
                let selected: BTreeMap<_, _> = rolls.iter()
                    .map(|roll| (roll.receipt_name.trim(), roll.expected_revision)).collect();
                if selected.is_empty() || selected.len() != rolls.len() || selected.len() > MAX_PRODUCTION_NOTE_ROLLS {
                    return Err(PreparationError::Invalid("1 dan 100 tagacha turli rulon tanlang"));
                }
                ("roll", selected, text, audience)
            }
        };
        if target_kind == "batch"
            && targets.keys().any(|id| {
                id.strip_prefix("batch:").is_none_or(|id| id.trim().is_empty())
            })
        {
            return Err(PreparationError::Invalid("Batch tanlanmagan"));
        }
        if targets.iter().any(|(id, rev)| id.is_empty() || id.len() > 1024 || id.contains('\0') || !(0..9_007_199_254_740_991).contains(rev)) {
            return Err(PreparationError::Invalid("Eslatma yozuvi yoki versiyasi noto‘g‘ri"));
        }
        let text = text.as_deref().map(str::trim);
        if text.is_some_and(|text| text.chars().count() > MAX_PRODUCTION_NOTE_CHARS || text.contains('\0')) {
            return Err(PreparationError::Invalid("Eslatma 2000 belgidan oshmasligi kerak"));
        }
        if text.is_none() && audience.is_none() {
            return Err(PreparationError::Invalid("Eslatma yoki ko‘rinish sozlamasini kiriting"));
        }
        Ok(PreparationNoteChanges { target_kind, targets, text, audience: audience.as_ref() })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationProductionNoteUpdate {
    pub order_id: String,
    pub text: String,
    pub expected_revision: i64,
}

impl PreparationProductionNoteUpdate {
    pub fn validate(&self) -> Result<(), PreparationError> {
        if self.order_id.trim().is_empty() || self.order_id.len() > 512 || self.order_id.contains('\0') {
            return Err(PreparationError::Invalid("Buyurtma tanlanmagan"));
        }
        if self.expected_revision < 0 {
            return Err(PreparationError::Invalid("Eslatma versiyasi noto‘g‘ri"));
        }
        if self.text.trim().chars().count() > MAX_PRODUCTION_NOTE_CHARS || self.text.contains('\0') {
            return Err(PreparationError::Invalid("Eslatma 2000 belgidan oshmasligi kerak"));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PreparationError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("Ombor yoki homashyo sizga biriktirilmagan")]
    Forbidden,
    #[error("Bu omborga yangi homashyo ulash huquqi yo‘q")]
    WarehouseNotExclusive,
    #[error("Faqat o‘zingiz yaratgan va faqat sizga biriktirilgan bola omborni boshqarishingiz mumkin")]
    WarehouseNotOwned,
    #[error("Bunday nomli ombor mavjud. Boshqa nom kiriting")]
    WarehouseNameTaken,
    #[error("Omborda homashyo yoki mahsulot bor. Avval ularni boshqa omborga ko‘chiring")]
    WarehouseNotEmpty,
    #[error("Omborga homashyo biriktirilgan. Avval homashyo bog‘lanishlarini olib tashlang")]
    WarehouseHasMaterials,
    #[error("Bu omborda bola omborlar bor. Avval ularni boshqaring")]
    WarehouseHasChildren,
    #[error("Omborda bog‘langan yozuvlar yoki faol operatsiyalar bor. Uni o‘chirib bo‘lmaydi")]
    WarehouseInUse,
    #[error("Bu homashyo tanlangan omborga biriktirilmagan")]
    MaterialNotInWarehouse,
    #[error("Faqat o‘zingiz yaratgan homashyoni boshqarishingiz mumkin")]
    MaterialNotOwned,
    #[error("Sizda bunday nomli homashyo mavjud. Boshqa nom kiriting")]
    MaterialNameTaken,
    #[error("Homashyo ishlatilgan: qoldiq, kirim, formula yoki boshqa bog‘langan yozuvlar bor. Uni o‘chirib bo‘lmaydi")]
    MaterialInUse,
    #[error("Bu omborda homashyo qoldig‘i, kutilayotgan kirim yoki faol ko‘chirish bor. Avval ularni yakunlang")]
    MaterialWarehouseInUse,
    #[error("Bu omborga kirim uchun tarozi kirimidan foydalaning — QR majburiy")]
    ReceiptRequiresQr,
    #[error("Homashyo qoldig‘i yetarli emas")]
    Insufficient,
    #[error("Tayyorlov ma’lumotlarini saqlashda xatolik")]
    StoreFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparationWarehouseMaterialScope {
    OwnSeriyo,
    AssignedItemGroups(Vec<String>),
}

pub type PreparationWarehouseMaterialScopes =
    BTreeMap<String, PreparationWarehouseMaterialScope>;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaterialStockTab {
    #[default]
    Available,
    Receipts,
    Issues,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MaterialStockQuery {
    pub warehouse: String,
    pub item_code: String,
    #[serde(default)]
    pub tab: MaterialStockTab,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "material_stock_page_size")]
    pub limit: usize,
}

fn material_stock_page_size() -> usize {
    50
}

impl MaterialStockQuery {
    pub fn validate(&self) -> Result<(), PreparationError> {
        if self.warehouse.trim().is_empty()
            || self.warehouse.len() > 512
            || self.item_code.trim().is_empty()
            || self.item_code.len() > 512
            || self.offset > 1_000_000
            || !(1..=100).contains(&self.limit)
        {
            return Err(PreparationError::Invalid(
                "Homashyo ro‘yxati filtri noto‘g‘ri",
            ));
        }
        Ok(())
    }
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
    pub warehouse: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialRename {
    pub item_code: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialDelete {
    pub item_code: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialWarehousesUpdate {
    pub item_code: String,
    pub warehouses: Vec<String>,
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
pub struct ReceiptReversalCreate {
    pub request_id: String,
    pub receipt_id: String,
    pub reason: String,
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
pub struct PreparationWarehouseCreate {
    pub parent_warehouse: String,
    pub name: String,
}

impl PreparationWarehouseCreate {
    pub fn clean(value: &str) -> Result<String, PreparationError> {
        let name = value.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() || name.chars().count() > 160 {
            return Err(PreparationError::Invalid(
                "Ombor nomi 1–160 ta belgidan iborat bo‘lishi kerak",
            ));
        }
        Ok(name)
    }

    pub fn parent_key(&self) -> Result<String, PreparationError> {
        Self::clean(&self.parent_warehouse)
    }

    pub fn warehouse_name(&self) -> Result<String, PreparationError> {
        let name = Self::clean(&self.name)?;
        if name.to_lowercase() == self.parent_warehouse.trim().to_lowercase() {
            return Err(PreparationError::Invalid(
                "Bola ombor nomi ota ombordan farqli bo‘lishi kerak",
            ));
        }
        Ok(name)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationWarehouseRename {
    pub warehouse: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationWarehouseDelete {
    pub warehouse: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaLine {
    pub item_code: String,
    pub percent: String,
}

/// A client selects a recipe; its contents are always loaded by the server.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationFormulaReference {
    pub material_id: String,
    #[serde(default)]
    pub variant_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaUpsert {
    pub product_code: String,
    #[serde(default = "default_formula_name")]
    pub name: String,
    #[serde(default)]
    pub material_id: String,
    pub lines: Vec<FormulaLine>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaSetRename {
    pub set_id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaVariantCreate {
    pub product_code: String,
    pub material_id: String,
    pub name: String,
    pub request_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaVariantReuse {
    pub product_code: String,
    pub material_id: String,
    pub source_variant_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaVariantOrderRemove {
    pub order_id: String,
    pub product_code: String,
    pub material_id: String,
    pub variant_id: String,
}

include!("preparation_parts/formula_variant_target.rs");

#[cfg(test)]
#[path = "preparation_tests.rs"]
mod tests;
