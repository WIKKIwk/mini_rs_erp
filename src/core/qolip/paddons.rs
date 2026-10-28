use super::{QolipError, QolipService};
use crate::core::auth::models::Principal;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum QolipStorageError {
    #[error("{0}")]
    Conflict(&'static str),
    #[error("{0}")]
    Invalid(&'static str),
    #[error("qolip_storage_forbidden")]
    Forbidden,
    #[error("qolip_storage_not_found")]
    NotFound,
    #[error("qolip_storage_failed")]
    Store,
}
impl From<QolipError> for QolipStorageError {
    fn from(e: QolipError) -> Self {
        match e {
            QolipError::AccessDenied => Self::Forbidden,
            QolipError::QolipCodeNotFound => Self::NotFound,
            QolipError::QolipInUse => Self::Conflict("qolip_in_use"),
            QolipError::QolipInRework => Self::Conflict("qolip_in_rework"),
            _ => Self::Store,
        }
    }
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QolipStorageQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub status: String,
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QolipStorageAction {
    pub action: String,
    pub request_id: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub warehouse: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub note: String,
    pub expected_revision: Option<i64>,
    #[serde(default)]
    pub qolip_codes: Vec<String>,
    #[serde(default)]
    pub entry_ids: Vec<String>,
}
impl QolipStorageAction {
    pub fn normalize(&mut self, archive: bool) -> Result<(), QolipStorageError> {
        self.action = self.action.trim().to_owned();
        self.request_id = self.request_id.trim().to_owned();
        self.code = self.code.trim().to_uppercase();
        self.warehouse = self.warehouse.trim().to_owned();
        self.location = self.location.trim().to_owned();
        self.note = self.note.trim().to_owned();
        if self.request_id.is_empty()
            || self.request_id.len() > 128
            || self.code.len() > 64
            || self.warehouse.len() > 512
            || self.location.len() > 512
            || self.note.len() > 2000
        {
            return Err(QolipStorageError::Invalid("qolip_storage_invalid_input"));
        }
        let actions = if archive {
            &["archive", "restore"][..]
        } else {
            &[
                "create", "add", "remove", "lock", "unlock", "archive", "restore", "inspect",
            ][..]
        };
        if !actions.contains(&self.action.as_str()) {
            return Err(QolipStorageError::Invalid("qolip_storage_invalid_action"));
        }
        for (values, lower) in [(&mut self.qolip_codes, true), (&mut self.entry_ids, false)] {
            if values.len() > 1000 {
                return Err(QolipStorageError::Invalid("qolip_storage_too_many_items"));
            }
            for value in values.iter_mut() {
                *value = if lower {
                    super::qolip_scan_code_from_qr(value).trim().to_lowercase()
                } else {
                    value.trim().to_owned()
                };
                if value.is_empty() || value.len() > 512 {
                    return Err(QolipStorageError::Invalid("qolip_storage_invalid_code"));
                }
            }
            values.sort();
            values.dedup();
        }
        if archive {
            if self.action == "archive" && self.qolip_codes.is_empty()
                || self.action == "restore" && self.entry_ids.is_empty()
            {
                return Err(QolipStorageError::Invalid(
                    "qolip_storage_selection_required",
                ));
            }
        } else if self.action == "create" {
            if self.warehouse.is_empty() || !self.qolip_codes.is_empty() {
                return Err(QolipStorageError::Invalid(
                    "qolip_storage_warehouse_required",
                ));
            }
        } else {
            if self.code.is_empty() || self.expected_revision.is_none_or(|n| n < 1) {
                return Err(QolipStorageError::Invalid(
                    "qolip_storage_revision_required",
                ));
            }
            if matches!(self.action.as_str(), "add" | "remove") && self.qolip_codes.is_empty() {
                return Err(QolipStorageError::Invalid(
                    "qolip_storage_selection_required",
                ));
            }
        }
        Ok(())
    }
}
impl QolipService {
    pub async fn storage_read(
        &self,
        kind: &str,
        query: &QolipStorageQuery,
        principal: &Principal,
        admin: bool,
    ) -> Result<Value, QolipStorageError> {
        if query.q.len() > 512
            || query.offset > 1_000_000
            || (!query.status.is_empty()
                && !matches!(query.status.as_str(), "open" | "all" | "returned"))
        {
            return Err(QolipStorageError::Invalid("qolip_storage_invalid_query"));
        }
        let allowed = self.warehouses_for_principal(principal, false).await?;
        self.store
            .storage_read(kind, query, if admin { None } else { Some(&allowed) })
            .await
    }
    pub async fn storage_action(
        &self,
        mut input: QolipStorageAction,
        archive: bool,
        principal: &Principal,
        admin: bool,
    ) -> Result<Value, QolipStorageError> {
        input.normalize(archive)?;
        let allowed = self.warehouses_for_principal(principal, admin).await?;
        // Even administrators must create packages in an existing Qolip warehouse.
        if !archive
            && input.action == "create"
            && !allowed
                .iter()
                .any(|w| w.eq_ignore_ascii_case(&input.warehouse))
        {
            return Err(QolipStorageError::Forbidden);
        }
        self.store
            .storage_action(
                &input,
                archive,
                principal,
                if admin { None } else { Some(&allowed) },
            )
            .await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input(action: &str) -> QolipStorageAction {
        serde_json::from_value(serde_json::json!({"action":action,"request_id":"retry-1","code":"QP-00001","expected_revision":2})).unwrap()
    }
    #[test]
    fn refuses_stale_unversioned_actions() {
        let mut i = input("lock");
        i.expected_revision = None;
        assert!(i.normalize(false).is_err());
    }
    #[test]
    fn canonicalizes_physical_selection() {
        let mut i = input("add");
        i.qolip_codes = vec![" AbC ".into(), "abc".into()];
        i.normalize(false).unwrap();
        assert_eq!(i.qolip_codes, vec!["abc"]);
    }
    #[test]
    fn archive_restore_requires_visit_identity() {
        let mut i = input("restore");
        i.qolip_codes = vec!["abc".into()];
        assert!(i.normalize(true).is_err());
    }
    #[test]
    fn unknown_actions_and_empty_selection_fail() {
        assert!(input("delete").normalize(false).is_err());
        assert!(input("add").normalize(false).is_err());
    }
}
