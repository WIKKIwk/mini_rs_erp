use serde::{Deserialize, Serialize};

use super::{QolipError, QolipProductSpec, QolipSetSnapshot};
use crate::core::auth::models::Principal;

#[derive(Debug, Clone, Serialize)]
pub struct QolipManagedSet {
    #[serde(flatten)]
    pub snapshot: QolipSetSnapshot,
    pub name: String,
    pub warehouse: String,
    pub item_code: String,
    pub item_name: String,
    pub item_group: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QolipSetRename {
    pub set_id: String,
    pub name: String,
}

pub(super) fn normalize_set_name(name: &str) -> Result<String, QolipError> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > 160 || name.chars().any(char::is_control) {
        return Err(QolipError::InvalidSetName);
    }
    Ok(name)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QolipSetCreate {
    pub warehouse: String,
    pub members: Vec<QolipMoldDraft>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct QolipMoldDraft {
    pub qolip_code: String,
    #[serde(default)]
    pub existing_only: bool,
    #[serde(default)]
    pub size: i32,
    #[serde(default)]
    pub color: String,
}

impl QolipMoldDraft {
    pub(crate) fn standalone_spec(
        &self,
        warehouse: &str,
        set_id: &str,
        principal: &Principal,
    ) -> Result<QolipProductSpec, QolipError> {
        if self.existing_only {
            return Err(QolipError::QolipCodeConflict);
        }
        if self.color.trim().is_empty() || self.color.len() > 64 {
            return Err(QolipError::InvalidColor);
        }
        self.physical_spec(warehouse, set_id, principal)
    }

    pub(crate) fn spec(
        &self,
        item: (&str, &str, &str),
        warehouse: &str,
        set_id: &str,
        principal: &Principal,
    ) -> Result<QolipProductSpec, QolipError> {
        if item.0.is_empty() || item.1.is_empty() {
            return Err(QolipError::MissingItem);
        }
        if item.2.is_empty() {
            return Err(QolipError::MissingItemGroup);
        }
        Ok(QolipProductSpec {
            item_code: item.0.to_owned(),
            item_name: item.1.to_owned(),
            item_group: item.2.to_owned(),
            ..self.physical_spec(warehouse, set_id, principal)?
        })
    }

    fn physical_spec(
        &self,
        warehouse: &str,
        set_id: &str,
        principal: &Principal,
    ) -> Result<QolipProductSpec, QolipError> {
        if self.qolip_code.trim().is_empty() || self.qolip_code.len() > 512 {
            return Err(QolipError::MissingQolipCode);
        }
        if self.size <= 0 {
            return Err(QolipError::InvalidSize);
        }
        Ok(QolipProductSpec {
            qolip_set_id: set_id.to_owned(),
            warehouse: warehouse.trim().to_owned(),
            qolip_code: self.qolip_code.trim().to_owned(),
            size: self.size,
            color: self.color.trim().to_owned(),
            created_by_role: super::role_code(&principal.role).to_owned(),
            created_by_ref: principal.ref_.clone(),
            created_by_name: principal.display_name.clone(),
            ..Default::default()
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct QolipSetEdit {
    pub set_id: String,
    pub fingerprint: String,
    #[serde(default)]
    pub add: Vec<QolipMoldDraft>,
    #[serde(default)]
    pub remove: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct QolipProductMoldEdit {
    pub item_code: String,
    #[serde(default)]
    pub warehouse: String,
    /// Existing codes are linked without changing their physical metadata.
    #[serde(default)]
    pub add: Vec<QolipMoldDraft>,
    #[serde(default)]
    pub remove: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct QolipProductMoldEdits {
    pub changes: Vec<QolipProductMoldEdit>,
}

pub(crate) fn validate_codes(add: &[QolipMoldDraft], remove: &[String]) -> Result<(), QolipError> {
    let count = add.len() + remove.len();
    if count == 0 || count > 1000 {
        return Err(QolipError::MissingQolipCode);
    }
    let mut seen = std::collections::BTreeSet::new();
    for code in add.iter().map(|draft| &draft.qolip_code).chain(remove) {
        if code.trim().is_empty() || code.len() > 512 || !seen.insert(code.trim().to_lowercase()) {
            return Err(QolipError::QolipCodeConflict);
        }
    }
    Ok(())
}
