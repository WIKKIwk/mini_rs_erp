//! On-demand input history and reversal of an untouched physical WIP claim.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannedWipInput {
    pub session_id: String,
    pub batch_id: String,
    pub qr_payload: String,
    pub source_apparatus: String,
    pub source_kind: String,
    pub linked_at_unix: i64,
    pub worker_display_name: String,
    /// `processed` at a splice boundary is deliberately not consumption evidence.
    pub state: String,
    pub consume_state: String,
    pub return_token: Option<String>,
    pub blocked_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScannedWipReturn {
    pub apparatus: String,
    pub order_id: String,
    pub stage_node_id: String,
    pub session_id: String,
    pub batch_id: String,
    pub token: String,
}

pub(crate) fn scanned_input_links(
    session: &OrderRunSession,
) -> Result<Vec<OrderRunInputLink>, ProductionMapError> {
    let links = order_run_input_links_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::QueueActionNotAllowed)?;
    if !links.is_empty() || session.payload_json.get("input_lineage").is_some() {
        return Ok(links);
    }
    let field = |key| session.payload_json[key].as_str().unwrap_or_default();
    let batch = field("input_progress_batch_id");
    if batch.is_empty() {
        return Ok(links);
    }
    let kind = OrderRunInputSourceKind::parse(field("input_wip_source_kind"))
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
    Ok(vec![OrderRunInputLink {
        input_batch_id: batch.into(),
        input_qr_payload: field("input_progress_qr_payload").into(),
        source_apparatus: field("input_progress_apparatus").into(),
        source_kind: kind,
        stage_node_id: session.stage_node_id.clone(),
        sequence_no: 1,
        status: OrderRunInputStatus::InUse,
        linked_at_unix: session.started_at_unix,
        processed_at_unix: None,
    }])
}

impl ProductionMapService {
    async fn validate_scanned_wip_scope(
        &self,
        apparatus: &str,
        order: &str,
        stage: &str,
        assigned: &[String],
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        if actor.role != "aparatchi"
            || actor.ref_.trim().is_empty()
            || order.trim().is_empty()
            || stage.trim().is_empty()
            || order.starts_with("training-")
            || !assigned.iter().any(|id| id == apparatus)
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        if !apparatus::requires_previous_stage(&canonical) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let map = self
            .raw_map(order)
            .await?
            .ok_or(ProductionMapError::MissingId)?;
        if !chain::linear_work_stages(&map)
            .iter()
            .any(|work| work.node_id == stage && work.apparatus_id.as_deref() == Some(apparatus))
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        Ok(())
    }

    pub async fn scanned_wip_inputs(
        &self,
        apparatus: &str,
        order: &str,
        stage: &str,
        assigned: &[String],
        actor: QueueActionActor,
    ) -> Result<Vec<ScannedWipInput>, ProductionMapError> {
        self.validate_scanned_wip_scope(apparatus, order, stage, assigned, &actor)
            .await?;
        self.store
            .scanned_wip_inputs(apparatus, order, stage, &actor)
            .await
    }

    pub async fn return_scanned_wip(
        &self,
        target: &ScannedWipReturn,
        assigned: &[String],
        actor: QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        self.validate_scanned_wip_scope(
            &target.apparatus,
            &target.order_id,
            &target.stage_node_id,
            assigned,
            &actor,
        )
        .await?;
        if target.session_id.is_empty()
            || target.batch_id.is_empty()
            || target.token.len() != 64
            || !target.token.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let _guard = self.apparatus_action_guard(&target.apparatus).await;
        self.store
            .return_scanned_wip(target, &actor, assigned)
            .await?;
        self.notify_live();
        Ok(())
    }
}

/// Remove only the selected input's provenance. Existing output batches are
/// immutable; the caller must reject any input referenced by recorded work.
pub(crate) fn detach_scanned_input(
    current: &OrderRunSession,
    batch_id: &str,
    now: i64,
) -> Result<(OrderRunSession, Vec<OrderRunInputLink>), ProductionMapError> {
    let mut links = scanned_input_links(current)?;
    let selected = links
        .iter()
        .find(|l| l.input_batch_id == batch_id)
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
    let was_active = selected.status == OrderRunInputStatus::InUse;
    links.retain(|link| link.input_batch_id != batch_id);
    links.sort_by_key(|link| link.sequence_no);
    if was_active && let Some(last) = links.last_mut() {
        last.status = OrderRunInputStatus::InUse;
        last.processed_at_unix = None;
    }
    // Preserve surviving sequence numbers used by existing output provenance.
    let mut session = current.clone();
    session.updated_at_unix = now;
    write_order_run_input_links(&mut session.payload_json, &links);
    if let Some(active) = links
        .iter()
        .find(|l| l.status == OrderRunInputStatus::InUse)
    {
        session.payload_json["input_progress_batch_id"] = serde_json::json!(active.input_batch_id);
        session.payload_json["input_progress_qr_payload"] =
            serde_json::json!(active.input_qr_payload);
        session.payload_json["input_progress_apparatus"] =
            serde_json::json!(active.source_apparatus);
        session.payload_json["input_wip_source_kind"] =
            serde_json::json!(active.source_kind.as_str());
    }
    let mut rolls = rezka_active_partial_rolls_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::QueueActionNotAllowed)?;
    for roll in &mut rolls {
        roll.source_input_batch_ids.retain(|id| id != batch_id);
        if let Some(active) = links
            .iter()
            .find(|l| l.status == OrderRunInputStatus::InUse)
            && !roll.source_input_batch_ids.contains(&active.input_batch_id)
        {
            roll.source_input_batch_ids
                .push(active.input_batch_id.clone());
        }
    }
    if !rolls.is_empty() {
        write_rezka_active_partial_rolls(&mut session.payload_json, &rolls);
    }
    if let Some(payload) = session.payload_json.as_object_mut() {
        // Old reversal tokens must never survive this changed execution state.
        payload.remove("_resume_pause");
        payload.remove("merge_from_input_batch_id");
        payload.remove("merge_to_input_batch_id");
        payload.insert(
            "last_action".into(),
            serde_json::json!("wip_input_returned"),
        );
    }
    Ok((session, links))
}

pub(crate) fn remount_progress_input(
    batch: OrderProgressBatch,
    apparatus: &str,
    session: &str,
    now: i64,
) -> OrderProgressBatch {
    super::service_progress_support::wip_batch_in_use(batch, apparatus, session, now)
}
