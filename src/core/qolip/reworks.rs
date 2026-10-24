use serde::{Deserialize, Serialize};

use super::QolipError;

/// One physical mold's visit to peredelka. Snapshots stay stable after return.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QolipRework {
    pub id: String,
    pub qolip_code: String,
    pub qolip_set_id: String,
    pub warehouse: String,
    pub item_code: String,
    pub item_name: String,
    pub color: String,
    pub size: i32,
    pub sent_at: String,
    pub returned_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QolipReworkSend {
    pub request_id: String,
    pub qolip_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QolipReworkReturn {
    pub request_id: String,
    pub rework_ids: Vec<String>,
}

pub(super) fn normalize_request(
    request_id: &mut String,
    values: &mut Vec<String>,
    lowercase: bool,
) -> Result<(), QolipError> {
    *request_id = request_id.trim().to_owned();
    if request_id.is_empty() || request_id.len() > 128 || values.is_empty() || values.len() > 1000 {
        return Err(QolipError::ReworkConflict);
    }
    for value in values.iter_mut() {
        *value = value.trim().to_owned();
        if value.is_empty() || value.len() > 512 {
            return Err(QolipError::ReworkConflict);
        }
        if lowercase {
            *value = value.to_lowercase();
        }
    }
    values.sort();
    values.dedup();
    Ok(())
}

pub(super) fn ensure_rework_warehouse(
    warehouse: &str,
    allowed: Option<&[String]>,
) -> Result<(), QolipError> {
    if warehouse.trim().is_empty()
        || allowed.is_some_and(|warehouses| {
            !warehouses
                .iter()
                .any(|value| value.trim().eq_ignore_ascii_case(warehouse.trim()))
        })
    {
        return Err(QolipError::AccessDenied);
    }
    Ok(())
}

pub(super) fn validate_rework_status(status: &str) -> Result<(), QolipError> {
    if matches!(status, "open" | "returned" | "all") {
        Ok(())
    } else {
        Err(QolipError::ReworkConflict)
    }
}

#[derive(Default)]
pub(super) struct MemoryQolipReworkState {
    pub entries: std::collections::BTreeMap<String, QolipRework>,
    pub requests:
        std::collections::BTreeMap<(String, String), (String, serde_json::Value, Vec<QolipRework>)>,
}

impl MemoryQolipReworkState {
    pub(super) fn ensure_available(&self, codes: &[String]) -> Result<(), QolipError> {
        if self.entries.values().any(|entry| {
            entry.returned_at.is_none()
                && codes
                    .iter()
                    .any(|code| entry.qolip_code.eq_ignore_ascii_case(code.trim()))
        }) {
            return Err(QolipError::QolipInRework);
        }
        Ok(())
    }
}
