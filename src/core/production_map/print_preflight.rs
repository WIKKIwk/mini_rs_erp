use serde::{Deserialize, Serialize};

use super::pechat;
use super::progress;
use super::queue_state;
use super::service::ProductionMapService;
use super::types::{OrderControlState, ProductionMapError, QueueActionActor};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintPreflightStatus {
    Held,
    Running,
    Passed,
    Failed,
    Cancelled,
    Consumed,
}

impl PrintPreflightStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Consumed => "consumed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "held" => Some(Self::Held),
            "running" => Some(Self::Running),
            "passed" => Some(Self::Passed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            "consumed" => Some(Self::Consumed),
            _ => None,
        }
    }

    pub fn reserves_apparatus(self) -> bool {
        matches!(self, Self::Held | Self::Running | Self::Passed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrintPreflightHold {
    pub hold_id: String,
    pub idempotency_key: String,
    pub order_id: String,
    pub apparatus: String,
    #[serde(default)]
    pub stage_node_id: String,
    pub status: PrintPreflightStatus,
    pub actor: QueueActionActor,
    pub created_at_unix: i64,
    pub updated_at_unix: i64,
    /// Queue state to restore when the colour trial fails; absent means no row.
    #[serde(default)]
    pub previous_queue_state: Option<String>,
    pub expires_at_unix: i64,
}

impl PrintPreflightHold {
    pub fn is_live_at(&self, _now: i64) -> bool {
        self.status.reserves_apparatus()
    }
}

impl ProductionMapService {
    pub async fn begin_print_preflight(
        &self,
        apparatus: &str,
        order_id: &str,
        hold_id: &str,
        idempotency_key: &str,
        actor: QueueActionActor,
    ) -> Result<PrintPreflightHold, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let apparatus = apparatus.trim();
        let order_id = order_id.trim();
        let hold_id = hold_id.trim();
        let idempotency_key = idempotency_key.trim();
        if apparatus.is_empty()
            || order_id.is_empty()
            || hold_id.is_empty()
            || idempotency_key.is_empty()
            || hold_id.len() > 200
            || idempotency_key.len() > 200
        {
            return Err(ProductionMapError::PrintPreflightNotReady);
        }

        if let Some(existing) = self
            .store
            .print_preflight_hold_by_idempotency_key(idempotency_key)
            .await?
        {
            if existing.order_id.trim() == order_id
                && queue_state::apparatus_ids_match(&existing.apparatus, apparatus)
            {
                if existing.status == PrintPreflightStatus::Held {
                    let now = progress::unix_seconds();
                    let mut running = existing;
                    running.status = PrintPreflightStatus::Running;
                    running.updated_at_unix = now;
                    self.store
                        .update_print_preflight_hold(running.clone())
                        .await?;
                    self.notify_live();
                    return Ok(running);
                }
                return Ok(existing);
            }
            return Err(ProductionMapError::PrintPreflightActive);
        }

        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        if !pechat::is_pechat_apparatus(&canonical) {
            return Err(ProductionMapError::PrintPreflightNotReady);
        }
        let canonical_id = canonical.runtime.apparatus_id.to_string();
        let now = progress::unix_seconds();
        let active_holds = self.store.active_print_preflight_holds().await?;
        if let Some(existing) = active_holds.iter().find(|hold| {
            hold.is_live_at(now) && queue_state::apparatus_ids_match(&hold.apparatus, &canonical_id)
        }) {
            if existing.order_id.trim() == order_id {
                if existing.status == PrintPreflightStatus::Held {
                    let now = progress::unix_seconds();
                    let mut running = existing.clone();
                    running.status = PrintPreflightStatus::Running;
                    running.updated_at_unix = now;
                    self.store
                        .update_print_preflight_hold(running.clone())
                        .await?;
                    self.notify_live();
                    return Ok(running);
                }
                return Ok(existing.clone());
            }
            return Err(ProductionMapError::PrintPreflightActive);
        }

        let controls = self.queue_action_controls().await?;
        let control = controls
            .get(&canonical_id)
            .and_then(|items| items.get(order_id))
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        if !control.print_preflight_allowed {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }

        let previous_queue_state = self
            .store
            .apparatus_queue_states()
            .await?
            .get(&canonical_id)
            .and_then(|states| states.get(order_id))
            .cloned();
        let hold = PrintPreflightHold {
            hold_id: hold_id.to_string(),
            idempotency_key: idempotency_key.to_string(),
            order_id: order_id.to_string(),
            apparatus: canonical_id,
            stage_node_id: control.stage_node_id.clone(),
            // The colour button is the actual start of the preflight. Keep
            // the reservation and the running state in one server write so
            // the client never exposes a second Start button.
            status: PrintPreflightStatus::Running,
            actor,
            created_at_unix: now,
            updated_at_unix: now,
            previous_queue_state,
            // Retained for old API clients. The persisted queue status has no TTL.
            expires_at_unix: 0,
        };
        self.store.put_print_preflight_hold(hold.clone()).await?;
        self.notify_live();
        Ok(hold)
    }

    pub async fn advance_print_preflight(
        &self,
        apparatus: &str,
        order_id: &str,
        hold_id: &str,
        action: &str,
        actor: QueueActionActor,
    ) -> Result<PrintPreflightHold, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let hold_id = hold_id.trim();
        let order_id = order_id.trim();
        let apparatus = apparatus.trim();
        let mut hold = self
            .store
            .print_preflight_hold_by_id(hold_id)
            .await?
            .ok_or(ProductionMapError::PrintPreflightNotFound)?;
        let now = progress::unix_seconds();
        if hold.order_id.trim() != order_id
            || !queue_state::apparatus_ids_match(&hold.apparatus, apparatus)
            || !hold.is_live_at(now)
        {
            return Err(ProductionMapError::PrintPreflightNotReady);
        }
        if self.order_control_state(order_id).await?.state == OrderControlState::FreezeRequested {
            return Err(ProductionMapError::OrderFreezeRequested);
        }
        let next_status = match (action.trim().to_ascii_lowercase().as_str(), hold.status) {
            ("start", PrintPreflightStatus::Held) => PrintPreflightStatus::Running,
            ("passed", PrintPreflightStatus::Running) => PrintPreflightStatus::Passed,
            ("failed", PrintPreflightStatus::Held | PrintPreflightStatus::Running) => {
                PrintPreflightStatus::Failed
            }
            ("cancel", PrintPreflightStatus::Held | PrintPreflightStatus::Running) => {
                PrintPreflightStatus::Cancelled
            }
            ("start", PrintPreflightStatus::Running)
            | ("passed", PrintPreflightStatus::Passed)
            | ("failed", PrintPreflightStatus::Failed)
            | ("cancel", PrintPreflightStatus::Cancelled) => hold.status,
            _ => return Err(ProductionMapError::PrintPreflightActionNotAllowed),
        };
        hold.status = next_status;
        hold.actor = actor;
        hold.updated_at_unix = now;
        self.store.update_print_preflight_hold(hold.clone()).await?;
        self.notify_live();
        Ok(hold)
    }

    pub async fn validate_print_preflight_start(
        &self,
        apparatus: &str,
        order_id: &str,
        hold_id: &str,
    ) -> Result<(), ProductionMapError> {
        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        if !pechat::is_pechat_apparatus(&canonical) {
            return Ok(());
        }
        let canonical_id = canonical.runtime.apparatus_id.to_string();
        let now = progress::unix_seconds();
        let holds = self.store.active_print_preflight_holds().await?;
        if hold_id.trim().is_empty() {
            if holds.iter().any(|hold| {
                hold.is_live_at(now)
                    && queue_state::apparatus_ids_match(&hold.apparatus, &canonical_id)
            }) {
                return Err(ProductionMapError::PrintPreflightNotReady);
            }
            return Ok(());
        }
        let hold = holds
            .into_iter()
            .find(|hold| hold.hold_id.trim() == hold_id.trim())
            .ok_or(ProductionMapError::PrintPreflightNotFound)?;
        if hold.order_id.trim() != order_id.trim()
            || !queue_state::apparatus_ids_match(&hold.apparatus, &canonical_id)
            || hold.status != PrintPreflightStatus::Passed
        {
            return Err(ProductionMapError::PrintPreflightNotReady);
        }
        Ok(())
    }
}
