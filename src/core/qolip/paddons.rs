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
    #[serde(default)]
    pub view: String,
    #[serde(default)]
    pub warehouse: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: usize,
}
impl QolipStorageQuery {
    pub(crate) fn validate(&self) -> Result<(), QolipStorageError> {
        if self.q.len() > 512
            || self.code.len() > 512
            || self.warehouse.len() > 512
            || self.offset > 1_000_000
            || (!self.view.is_empty() && self.view != "molds")
            || (!self.status.is_empty()
                && !matches!(self.status.as_str(), "open" | "all" | "returned"))
        {
            return Err(QolipStorageError::Invalid("qolip_storage_invalid_query"));
        }
        let from = self.from.as_deref().map(parse_date).transpose()?;
        let to = self.to.as_deref().map(parse_date).transpose()?;
        if matches!((from,to),(Some(from),Some(to)) if from>to) {
            return Err(QolipStorageError::Invalid(
                "qolip_storage_invalid_date_range",
            ));
        }
        Ok(())
    }
}
fn parse_date(value: &str) -> Result<time::Date, QolipStorageError> {
    let invalid = || QolipStorageError::Invalid("qolip_storage_invalid_date");
    if value.len() != 10
        || !value.is_ascii()
        || &value[4..5] != "-"
        || &value[7..8] != "-"
        || value
            .bytes()
            .enumerate()
            .any(|(i, b)| i != 4 && i != 7 && !b.is_ascii_digit())
    {
        return Err(invalid());
    }
    let year = value[..4].parse::<i32>().map_err(|_| invalid())?;
    let month = value[5..7].parse::<u8>().map_err(|_| invalid())?;
    let day = value[8..].parse::<u8>().map_err(|_| invalid())?;
    if !(1..=9999).contains(&year) {
        return Err(invalid());
    }
    time::Date::from_calendar_date(
        year,
        time::Month::try_from(month).map_err(|_| invalid())?,
        day,
    )
    .map_err(|_| invalid())
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
                "delete",
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
        query.validate()?;
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
        assert!(input("unknown").normalize(false).is_err());
        assert!(input("add").normalize(false).is_err());
    }
    #[test]
    fn delete_requires_a_current_revision() {
        assert!(input("delete").normalize(false).is_ok());
        let mut i = input("delete");
        i.expected_revision = None;
        assert!(i.normalize(false).is_err());
    }
    #[test]
    fn validates_archive_calendar_dates_and_order() {
        for date in [
            "2026-02-29",
            "2026-13-01",
            "0000-01-01",
            "26-10-28",
            "2026-1-28",
            "2026-10-28T00:00:00Z",
            "+026-10-28",
            "2026-+1-28",
            "2026-10-+8",
            "",
        ] {
            let q = QolipStorageQuery {
                from: Some(date.into()),
                ..Default::default()
            };
            assert!(q.validate().is_err(), "accepted {date}");
        }
        let reversed = QolipStorageQuery {
            from: Some("2026-10-29".into()),
            to: Some("2026-10-28".into()),
            ..Default::default()
        };
        assert!(matches!(
            reversed.validate(),
            Err(QolipStorageError::Invalid(
                "qolip_storage_invalid_date_range"
            ))
        ));
        let leap = QolipStorageQuery {
            from: Some("2024-02-29".into()),
            to: Some("2024-02-29".into()),
            ..Default::default()
        };
        assert!(leap.validate().is_ok());
    }
}
