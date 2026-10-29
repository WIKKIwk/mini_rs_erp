use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ProductionMapError, ProductionMapService, QueueActionActor};

/// A server-wide scan policy. Tool availability and resource ownership remain
/// required even when the operator need not scan each mold.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QolipStartSettings {
    pub automatic_confirmation_enabled: bool,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomaticQolipConfirmation {
    pub method: String,
    pub set_id: String,
    pub fingerprint: String,
    pub policy_revision: i64,
}

impl AutomaticQolipConfirmation {
    pub fn is_valid(&self) -> bool {
        self.method == "automatic"
            && !self.set_id.trim().is_empty()
            && self.set_id.len() <= 512
            && self.policy_revision >= 0
            && self.fingerprint.len() == 64
            && self
                .fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }

    pub fn require_policy(&self, settings: &QolipStartSettings) -> Result<(), ProductionMapError> {
        if !settings.automatic_confirmation_enabled {
            return Err(ProductionMapError::QolipAutomaticConfirmationDisabled);
        }
        if settings.revision != self.policy_revision {
            return Err(ProductionMapError::QolipPolicyChanged);
        }
        Ok(())
    }
}

/// Fingerprint only the physical composition used by this run. Product names,
/// optional set names and unrelated alternative sets cannot change its identity.
pub fn automatic_qolip_set_fingerprint(
    set_id: &str,
    specs: &[crate::core::qolip::QolipProductSpec],
) -> String {
    let mut members = specs
        .iter()
        .map(|spec| {
            (
                spec.qolip_code.trim().to_lowercase(),
                spec.set_id(),
                spec.warehouse.trim().to_lowercase(),
                spec.size,
                spec.color.clone(),
                spec.is_extension,
            )
        })
        .collect::<Vec<_>>();
    members.sort();
    format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(set_id, members)).expect("serializable mold composition")
        )
    )
}

impl ProductionMapService {
    pub async fn qolip_start_settings(&self) -> Result<QolipStartSettings, ProductionMapError> {
        self.store.qolip_start_settings().await
    }

    pub async fn update_qolip_start_settings(
        &self,
        enabled: bool,
        actor: &QueueActionActor,
    ) -> Result<QolipStartSettings, ProductionMapError> {
        if !matches!(actor.role.as_str(), "admin" | "brigader") || actor.ref_.trim().is_empty() {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        // Serialize in-memory starts and policy changes as well as using the
        // database row lock at the authoritative commit boundary.
        let _guard = self.queue_action_guard().await;
        let settings = self
            .store
            .update_qolip_start_settings(enabled, actor)
            .await?;
        self.notify_live();
        Ok(settings)
    }
}
