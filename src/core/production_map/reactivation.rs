//! Explicit correction of a premature Complete. It never reverses production quantities.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReactivationRecloseControl {
    pub session_id: String,
    pub stage_node_id: String,
    pub token: String,
    pub worker_role: String,
    pub worker_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReactivationRecloseTarget {
    pub apparatus: String,
    pub order_id: String,
    pub session_id: String,
    pub stage_node_id: String,
    pub token: String,
}

/// Only a paused continuation that has never started can undo its reopening.
pub(crate) fn reclose_control(session: &OrderRunSession) -> Option<ReactivationRecloseControl> {
    if session.status != OrderRunStatus::Paused
        || session.payload_json.get("_reactivation_started").and_then(serde_json::Value::as_bool) == Some(true)
        || !session.payload_json.get("_reactivation_reclose").is_some_and(serde_json::Value::is_object)
    {
        return None;
    }
    let token = session.payload_json.get("reactivation_token")?.as_str()?;
    if token.len() != 64 { return None; }
    let baseline = session.payload_json.get("_reactivation_reclose")?.get("payload")?;
    for key in ["rezka_output_cycle", "rezka_output_revision", "rezka_output_report",
        "rezka_active_partial_rolls", "rezka_recorded_kadr_counts", "qolip_code", "qolip_codes", "qolip_set_id", "qolip_set_snapshot"] {
        if session.payload_json.get(key) != baseline.get(key) { return None; }
    }
    Some(ReactivationRecloseControl {
        session_id: session.session_id.clone(), stage_node_id: session.stage_node_id.clone(),
        token: token.into(), worker_role: session.worker_role.clone(), worker_ref: session.worker_ref.clone(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReactivationSelector {
    pub apparatus: String,
    pub order_id: String,
    #[serde(default)]
    pub batch_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReactivationTarget {
    pub apparatus: String,
    pub order_id: String,
    #[serde(default)]
    pub batch_id: String,
    pub session_id: String,
    pub stage_node_id: String,
    pub token: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactivationPreview {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub denial_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<ReactivationTarget>,
}
impl ReactivationPreview {
    pub(crate) fn denied(reason: &str) -> Self {
        Self {
            available: false,
            denial_reason: Some(reason.into()),
            target: None,
        }
    }
}
impl ProductionMapService {
    pub async fn reclose_reactivated_work(
        &self, target: &ReactivationRecloseTarget, assigned: &[String], actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        self.validate_reactivation_selector(&ReactivationSelector {
            apparatus: target.apparatus.clone(), order_id: target.order_id.clone(), batch_id: String::new(),
        }, assigned, actor).await?;
        if target.token.len() != 64 || target.session_id.is_empty() || target.session_id.len() > 4096
            || target.stage_node_id.is_empty() || target.stage_node_id.len() > 512 {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let _guard = self.queue_action_guard().await;
        self.store.reclose_reactivated_work(target, actor, assigned).await?;
        self.notify_live();
        Ok(())
    }
    async fn validate_reactivation_selector(
        &self,
        selector: &ReactivationSelector,
        assigned: &[String],
        actor: &QueueActionActor,
    ) -> Result<bool, ProductionMapError> {
        if actor.role != "aparatchi"
            || actor.ref_.trim().is_empty()
            || selector.order_id.trim().is_empty()
            || selector.order_id.starts_with("training-")
            || selector.order_id.len() > 512
            || selector.batch_id.len() > 512
            || !assigned.iter().any(|id| id == &selector.apparatus)
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let canonical = self
            .resolve_canonical_apparatus_text(&selector.apparatus)
            .await?;
        if !pechat::is_pechat_apparatus(&canonical)
            && !apparatus::is_rezka_apparatus(&canonical)
            && !apparatus::uses_lamination_workflow(&canonical)
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        Ok(pechat::is_pechat_apparatus(&canonical))
    }
    /// A matching immutable receipt acknowledges a retry without reading or
    /// preparing resources for whatever work is current now.
    pub async fn reactivation_was_applied(
        &self, target: &ReactivationTarget, assigned: &[String], actor: &QueueActionActor,
    ) -> Result<bool, ProductionMapError> {
        self.validate_reactivation_selector(&ReactivationSelector {
            apparatus: target.apparatus.clone(), order_id: target.order_id.clone(),
            batch_id: target.batch_id.clone(),
        }, assigned, actor).await?;
        if target.token.len() != 64 || target.session_id.is_empty() || target.stage_node_id.is_empty() {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        self.store.reactivation_was_applied(target, actor).await
    }
    pub async fn preview_reactivation(
        &self,
        selector: &ReactivationSelector,
        assigned: &[String],
        actor: &QueueActionActor,
    ) -> Result<ReactivationPreview, ProductionMapError> {
        let is_print = self
            .validate_reactivation_selector(selector, assigned, actor)
            .await?;
        self.store
            .preview_reactivation(selector, actor, is_print)
            .await
    }
    pub async fn reactivate_completed_work(
        &self,
        target: &ReactivationTarget,
        assigned: &[String],
        actor: &QueueActionActor,
        checkouts: &[crate::core::qolip::QolipCheckout],
    ) -> Result<(), ProductionMapError> {
        let is_print = self
            .validate_reactivation_selector(
                &ReactivationSelector {
                    apparatus: target.apparatus.clone(),
                    order_id: target.order_id.clone(),
                    batch_id: target.batch_id.clone(),
                },
                assigned,
                actor,
            )
            .await?;
        if target.token.len() != 64
            || target.session_id.is_empty()
            || target.stage_node_id.is_empty()
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let _guard = self.queue_action_guard().await;
        self.store
            .reactivate_completed_work(target, actor, assigned, checkouts, is_print)
            .await?;
        self.notify_live();
        Ok(())
    }
}
