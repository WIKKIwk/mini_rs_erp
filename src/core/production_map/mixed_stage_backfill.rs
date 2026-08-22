use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

use super::chain;
use super::queue_state;
use super::store_port::{MixedStageBackfillWriteResult, ProductionMapStorePort};
use super::{
    OrderProgressBatch, OrderProgressBatchStatus, OrderProgressBatchWipStatus, OrderProgressEvent,
    OrderRunSession, OrderRunStatus, ProductionMapDefinition, ProductionMapError,
    ProductionMapService,
};

const MIXED_STAGE_BACKFILL_VERSION: u32 = 1;

/// A versioned, operator-supplied record for one physically completed prior
/// stage whose output must be visible as waiting WIP after cutover.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MixedStageBackfillRecord {
    pub idempotency_key: String,
    pub order_id: String,
    pub source_apparatus: String,
    pub next_apparatus: String,
    pub current_location: String,
    pub source_ref: String,
    pub started_at_unix: i64,
    pub completed_at_unix: i64,
    pub observed_at_unix: i64,
    pub produced_qty: f64,
    pub uom: String,
    pub label_item_code: String,
    pub label_item_name: String,
    #[serde(default)]
    pub executor_name: String,
    #[serde(default)]
    pub worker_role: String,
    #[serde(default)]
    pub worker_ref: String,
    #[serde(default)]
    pub worker_display_name: String,
    #[serde(default)]
    pub gross_qty: Option<f64>,
    #[serde(default)]
    pub return_ink_kg: Option<f64>,
    #[serde(default)]
    pub lamination_print_leftover_rolls: Option<f64>,
    #[serde(default)]
    pub lamination_film_leftover_rolls: Option<f64>,
    #[serde(default)]
    pub rezka_bosma_waste: Option<f64>,
    #[serde(default)]
    pub rezka_lamination_waste: Option<f64>,
    #[serde(default)]
    pub rezka_edge_waste: Option<f64>,
    #[serde(default, alias = "waste", alias = "atxot")]
    pub total_waste: Option<f64>,
    #[serde(default)]
    pub finished_goods_kg: Option<f64>,
    #[serde(default, alias = "babina_kg")]
    pub bobina_kg: Option<f64>,
    #[serde(default)]
    pub finished_goods_meter: Option<f64>,
    #[serde(default)]
    pub diameter: Option<f64>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MixedStageBackfillManifest {
    pub version: u32,
    pub source: String,
    pub records: Vec<MixedStageBackfillRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MixedStageBackfillPlanStatus {
    New,
    AlreadyPresent,
    Applied,
}

#[derive(Debug, Clone, Serialize)]
pub struct MixedStageBackfillPlanRow {
    pub input: MixedStageBackfillRecord,
    pub batch_id: String,
    pub session_id: String,
    pub event_id: String,
    pub qr_payload: String,
    pub status: MixedStageBackfillPlanStatus,
    #[serde(skip)]
    session: OrderRunSession,
    #[serde(skip)]
    event: OrderProgressEvent,
    #[serde(skip)]
    batch: OrderProgressBatch,
}

#[derive(Debug, Clone, Serialize)]
pub struct MixedStageBackfillPlan {
    pub version: u32,
    pub source: String,
    pub rows: Vec<MixedStageBackfillPlanRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MixedStageBackfillReport {
    pub version: u32,
    pub source: String,
    pub planned: usize,
    pub applied: usize,
    pub already_present: usize,
    pub rows: Vec<MixedStageBackfillPlanRow>,
}

impl ProductionMapService {
    /// Validate and resolve a backfill manifest without changing production
    /// state. The database is read to verify order and stage topology and to
    /// classify safe idempotent re-runs.
    pub async fn plan_mixed_stage_backfill(
        &self,
        manifest: &MixedStageBackfillManifest,
    ) -> Result<MixedStageBackfillPlan, ProductionMapError> {
        validate_manifest(manifest)?;
        let _guard = self.queue_action_guard().await;
        let maps = self.store.maps().await?;
        let source = manifest.source.trim().to_string();
        let mut rows = Vec::with_capacity(manifest.records.len());

        for (index, input) in manifest.records.iter().enumerate() {
            let mut input = normalize_record(input)?;
            let map = maps
                .iter()
                .find(|map| map.id.trim().eq_ignore_ascii_case(&input.order_id))
                .ok_or_else(|| {
                    ProductionMapError::MixedStageBackfillInput(format!(
                        "record {index}: order_id '{}' was not found",
                        input.order_id
                    ))
                })?;
            input.order_id = map.id.trim().to_string();
            validate_route(index, map, &input)?;
            let (session, event, batch) = build_artifacts(manifest.version, &source, &input)?;
            let status = existing_status(self.store.as_ref(), &event, &session, &batch).await?;
            rows.push(MixedStageBackfillPlanRow {
                input,
                batch_id: batch.batch_id.clone(),
                session_id: session.session_id.clone(),
                event_id: event.event_id.clone(),
                qr_payload: batch.qr_payload.clone(),
                status,
                session,
                event,
                batch,
            });
        }

        Ok(MixedStageBackfillPlan {
            version: manifest.version,
            source,
            rows,
        })
    }

    /// Apply a previously planned manifest. Each row is committed atomically
    /// by the production store; a failed later row leaves earlier rows safe to
    /// retry because the store operation is idempotent and conflict-checked.
    pub async fn apply_mixed_stage_backfill(
        &self,
        plan: &MixedStageBackfillPlan,
    ) -> Result<MixedStageBackfillReport, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let mut rows = Vec::with_capacity(plan.rows.len());
        let mut applied = 0;
        let mut already_present = 0;

        for row in &plan.rows {
            let mut output = row.clone();
            output.status = match self
                .store
                .put_mixed_stage_backfill(row.session.clone(), row.event.clone(), row.batch.clone())
                .await?
            {
                MixedStageBackfillWriteResult::Applied => {
                    applied += 1;
                    MixedStageBackfillPlanStatus::Applied
                }
                MixedStageBackfillWriteResult::AlreadyPresent => {
                    already_present += 1;
                    MixedStageBackfillPlanStatus::AlreadyPresent
                }
            };
            rows.push(output);
        }

        if applied > 0 {
            self.notify_live();
        }
        Ok(MixedStageBackfillReport {
            version: plan.version,
            source: plan.source.clone(),
            planned: plan.rows.len(),
            applied,
            already_present,
            rows,
        })
    }
}

fn validate_manifest(manifest: &MixedStageBackfillManifest) -> Result<(), ProductionMapError> {
    if manifest.version != MIXED_STAGE_BACKFILL_VERSION {
        return Err(ProductionMapError::MixedStageBackfillInput(format!(
            "unsupported manifest version {}; expected {}",
            manifest.version, MIXED_STAGE_BACKFILL_VERSION
        )));
    }
    if manifest.source.trim().is_empty() {
        return Err(ProductionMapError::MixedStageBackfillInput(
            "source is required".to_string(),
        ));
    }
    if manifest.records.is_empty() {
        return Err(ProductionMapError::MixedStageBackfillInput(
            "records must not be empty".to_string(),
        ));
    }

    let mut keys = BTreeSet::new();
    for (index, record) in manifest.records.iter().enumerate() {
        let normalized = normalize_record(record)?;
        if !keys.insert(normalized.idempotency_key.clone()) {
            return Err(ProductionMapError::MixedStageBackfillInput(format!(
                "record {index}: duplicate idempotency_key '{}'",
                normalized.idempotency_key
            )));
        }
    }
    Ok(())
}

fn normalize_record(
    record: &MixedStageBackfillRecord,
) -> Result<MixedStageBackfillRecord, ProductionMapError> {
    let mut record = record.clone();
    record.idempotency_key = record.idempotency_key.trim().to_string();
    record.order_id = record.order_id.trim().to_ascii_lowercase();
    record.source_apparatus = record.source_apparatus.trim().to_string();
    record.next_apparatus = record.next_apparatus.trim().to_string();
    record.current_location = record.current_location.trim().to_string();
    record.source_ref = record.source_ref.trim().to_string();
    record.uom = normalize_uom(&record.uom)?;
    record.label_item_code = record.label_item_code.trim().to_string();
    record.label_item_name = record.label_item_name.trim().to_string();
    record.executor_name = record.executor_name.trim().to_string();
    record.worker_role = record.worker_role.trim().to_string();
    record.worker_ref = record.worker_ref.trim().to_string();
    record.worker_display_name = record.worker_display_name.trim().to_string();
    record.description = record.description.trim().to_string();

    if record.idempotency_key.is_empty() || record.idempotency_key.chars().any(char::is_whitespace)
    {
        return Err(ProductionMapError::MixedStageBackfillInput(
            "idempotency_key must be non-empty and contain no whitespace".to_string(),
        ));
    }
    for (name, value) in [
        ("order_id", record.order_id.as_str()),
        ("source_apparatus", record.source_apparatus.as_str()),
        ("next_apparatus", record.next_apparatus.as_str()),
        ("current_location", record.current_location.as_str()),
        ("source_ref", record.source_ref.as_str()),
        ("label_item_code", record.label_item_code.as_str()),
        ("label_item_name", record.label_item_name.as_str()),
    ] {
        if value.is_empty() {
            return Err(ProductionMapError::MixedStageBackfillInput(format!(
                "{name} is required"
            )));
        }
    }
    for (name, value) in [
        ("started_at_unix", record.started_at_unix),
        ("completed_at_unix", record.completed_at_unix),
        ("observed_at_unix", record.observed_at_unix),
    ] {
        if value <= 0 {
            return Err(ProductionMapError::MixedStageBackfillInput(format!(
                "{name} must be positive"
            )));
        }
    }
    if record.started_at_unix > record.completed_at_unix {
        return Err(ProductionMapError::MixedStageBackfillInput(
            "started_at_unix must not be after completed_at_unix".to_string(),
        ));
    }
    if record.completed_at_unix > record.observed_at_unix {
        return Err(ProductionMapError::MixedStageBackfillInput(
            "completed_at_unix must not be after observed_at_unix".to_string(),
        ));
    }
    if !record.produced_qty.is_finite() || record.produced_qty <= 0.0 {
        return Err(ProductionMapError::MixedStageBackfillInput(
            "produced_qty must be finite and positive".to_string(),
        ));
    }

    for (name, value) in [
        ("gross_qty", record.gross_qty),
        ("return_ink_kg", record.return_ink_kg),
        (
            "lamination_print_leftover_rolls",
            record.lamination_print_leftover_rolls,
        ),
        (
            "lamination_film_leftover_rolls",
            record.lamination_film_leftover_rolls,
        ),
        ("rezka_bosma_waste", record.rezka_bosma_waste),
        ("rezka_lamination_waste", record.rezka_lamination_waste),
        ("rezka_edge_waste", record.rezka_edge_waste),
        ("total_waste", record.total_waste),
        ("finished_goods_kg", record.finished_goods_kg),
        ("finished_goods_meter", record.finished_goods_meter),
    ] {
        validate_non_negative_metric(name, value)?;
    }
    validate_positive_metric("bobina_kg", record.bobina_kg)?;
    validate_positive_metric("diameter", record.diameter)?;
    if record.uom == "m" && record.finished_goods_meter.is_none() {
        return Err(ProductionMapError::MixedStageBackfillInput(
            "uom=m requires finished_goods_meter".to_string(),
        ));
    }
    if record.uom == "kg" && record.finished_goods_kg.is_none() {
        return Err(ProductionMapError::MixedStageBackfillInput(
            "uom=kg requires finished_goods_kg".to_string(),
        ));
    }
    Ok(record)
}

fn normalize_uom(value: &str) -> Result<String, ProductionMapError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "m" | "meter" | "meters" => Ok("m".to_string()),
        "kg" | "kilogram" | "kilograms" => Ok("kg".to_string()),
        _ => Err(ProductionMapError::MixedStageBackfillInput(
            "uom must be m or kg".to_string(),
        )),
    }
}

fn validate_non_negative_metric(name: &str, value: Option<f64>) -> Result<(), ProductionMapError> {
    if let Some(value) = value
        && (!value.is_finite() || value < 0.0)
    {
        return Err(ProductionMapError::MixedStageBackfillInput(format!(
            "{name} must be finite and non-negative"
        )));
    }
    Ok(())
}

fn validate_positive_metric(name: &str, value: Option<f64>) -> Result<(), ProductionMapError> {
    if let Some(value) = value
        && (!value.is_finite() || value <= 0.0)
    {
        return Err(ProductionMapError::MixedStageBackfillInput(format!(
            "{name} must be finite and positive"
        )));
    }
    Ok(())
}

fn validate_route(
    index: usize,
    map: &ProductionMapDefinition,
    input: &MixedStageBackfillRecord,
) -> Result<(), ProductionMapError> {
    let expected_next =
        chain::next_work_stage_station(map, &input.source_apparatus).ok_or_else(|| {
            ProductionMapError::MixedStageBackfillInput(format!(
                "record {index}: source_apparatus '{}' is not a non-final work stage",
                input.source_apparatus
            ))
        })?;
    if !queue_state::next_stage_title_matches_apparatus(&expected_next, &input.next_apparatus) {
        return Err(ProductionMapError::MixedStageBackfillInput(format!(
            "record {index}: next_apparatus '{}' is not the mapped next stage '{}'",
            input.next_apparatus, expected_next
        )));
    }
    Ok(())
}

fn build_artifacts(
    version: u32,
    source: &str,
    input: &MixedStageBackfillRecord,
) -> Result<(OrderRunSession, OrderProgressEvent, OrderProgressBatch), ProductionMapError> {
    let fingerprint = fingerprint(version, source, input)?;
    let batch_id = deterministic_id("batch", &input.idempotency_key);
    let session_id = deterministic_id("session", &input.idempotency_key);
    let event_id = deterministic_id("event", &input.idempotency_key);
    let qr_payload = deterministic_id("qr", &input.idempotency_key);
    let payload_json = serde_json::json!({
        "historical_backfill": true,
        "backfill_kind": "mixed_stage_prior_wip",
        "backfill_manifest_version": version,
        "backfill_source": source,
        "backfill_fingerprint": fingerprint,
        "idempotency_key": input.idempotency_key.as_str(),
        "source_ref": input.source_ref.as_str(),
        "observed_at_unix": input.observed_at_unix,
        "gross_qty": input.gross_qty,
        "input": input,
        "queue_state_mutated": false,
    });

    let session = OrderRunSession {
        session_id: session_id.clone(),
        apparatus: input.source_apparatus.clone(),
        order_id: input.order_id.clone(),
        status: OrderRunStatus::Completed,
        worker_role: input.worker_role.clone(),
        worker_ref: input.worker_ref.clone(),
        worker_display_name: input.worker_display_name.clone(),
        started_at_unix: input.started_at_unix,
        updated_at_unix: input.observed_at_unix,
        payload_json: payload_json.clone(),
    };
    let event = OrderProgressEvent {
        event_id,
        session_id: session_id.clone(),
        batch_id: batch_id.clone(),
        apparatus: input.source_apparatus.clone(),
        order_id: input.order_id.clone(),
        action: queue_state::ApparatusQueueAction::Complete,
        produced_qty: input.produced_qty,
        uom: input.uom.clone(),
        worker_role: input.worker_role.clone(),
        worker_ref: input.worker_ref.clone(),
        worker_display_name: input.worker_display_name.clone(),
        qr_payload: qr_payload.clone(),
        return_ink_kg: input.return_ink_kg,
        lamination_print_leftover_rolls: input.lamination_print_leftover_rolls,
        lamination_film_leftover_rolls: input.lamination_film_leftover_rolls,
        rezka_bosma_waste: input.rezka_bosma_waste,
        rezka_lamination_waste: input.rezka_lamination_waste,
        rezka_edge_waste: input.rezka_edge_waste,
        total_waste: input.total_waste,
        finished_goods_kg: input.finished_goods_kg,
        bobina_kg: input.bobina_kg,
        finished_goods_meter: input.finished_goods_meter,
        diameter: input.diameter,
        description: input.description.clone(),
        payload_json: payload_json.clone(),
    };
    let mut batch = OrderProgressBatch {
        batch_id,
        revision: 1,
        session_id,
        started_at_unix: input.started_at_unix,
        completed_at_unix: input.completed_at_unix,
        apparatus: input.source_apparatus.clone(),
        order_id: input.order_id.clone(),
        action: queue_state::ApparatusQueueAction::Complete,
        status: OrderProgressBatchStatus::Completed,
        produced_qty: input.produced_qty,
        uom: input.uom.clone(),
        qr_payload,
        label_item_code: input.label_item_code.clone(),
        label_item_name: input.label_item_name.clone(),
        executor_name: input.executor_name.clone(),
        worker_role: input.worker_role.clone(),
        worker_ref: input.worker_ref.clone(),
        worker_display_name: input.worker_display_name.clone(),
        wip_status: OrderProgressBatchWipStatus::Waiting,
        status_detail: Default::default(),
        current_apparatus: input.source_apparatus.clone(),
        current_apparatus_key: queue_state::apparatus_search_key(&input.source_apparatus),
        current_location: input.current_location.clone(),
        next_apparatus: input.next_apparatus.clone(),
        parent_batch_id: String::new(),
        used_by_session_id: String::new(),
        used_by_apparatus: String::new(),
        processed_by_session_id: String::new(),
        processed_by_apparatus: String::new(),
        return_ink_kg: input.return_ink_kg,
        lamination_print_leftover_rolls: input.lamination_print_leftover_rolls,
        lamination_film_leftover_rolls: input.lamination_film_leftover_rolls,
        rezka_bosma_waste: input.rezka_bosma_waste,
        rezka_lamination_waste: input.rezka_lamination_waste,
        rezka_edge_waste: input.rezka_edge_waste,
        total_waste: input.total_waste,
        finished_goods_kg: input.finished_goods_kg,
        bobina_kg: input.bobina_kg,
        finished_goods_meter: input.finished_goods_meter,
        diameter: input.diameter,
        description: input.description.clone(),
        payload_json,
    };
    batch.refresh_status_detail();
    Ok((session, event, batch))
}

async fn existing_status(
    store: &dyn ProductionMapStorePort,
    event: &OrderProgressEvent,
    session: &OrderRunSession,
    batch: &OrderProgressBatch,
) -> Result<MixedStageBackfillPlanStatus, ProductionMapError> {
    let by_id = store.progress_batch(&batch.batch_id).await?;
    let by_qr = store.progress_batch_by_qr(&batch.qr_payload).await?;
    let mut existing = Vec::new();
    if let Some(value) = by_id {
        existing.push(value);
    }
    if let Some(value) = by_qr
        && !existing
            .iter()
            .any(|current: &OrderProgressBatch| current.batch_id == value.batch_id)
    {
        existing.push(value);
    }
    if existing.is_empty() {
        return Ok(MixedStageBackfillPlanStatus::New);
    }
    if existing.len() == 1 {
        let current = &existing[0];
        let current_fingerprint = current
            .payload_json
            .get("backfill_fingerprint")
            .and_then(serde_json::Value::as_str);
        let requested_fingerprint = batch
            .payload_json
            .get("backfill_fingerprint")
            .and_then(serde_json::Value::as_str);
        if current.batch_id == batch.batch_id
            && current.qr_payload == batch.qr_payload
            && current.session_id == session.session_id
            && current_fingerprint == requested_fingerprint
            && event.batch_id == batch.batch_id
        {
            return Ok(MixedStageBackfillPlanStatus::AlreadyPresent);
        }
    }
    Err(ProductionMapError::MixedStageBackfillConflict(format!(
        "batch_id '{}' or qr_payload '{}' is already used by another record",
        batch.batch_id, batch.qr_payload
    )))
}

fn fingerprint(
    version: u32,
    source: &str,
    input: &MixedStageBackfillRecord,
) -> Result<String, ProductionMapError> {
    let bytes = serde_json::to_vec(&(version, source, input)).map_err(|_| {
        ProductionMapError::MixedStageBackfillInput("record cannot be encoded".to_string())
    })?;
    Ok(hex_digest(&bytes))
}

fn deterministic_id(kind: &str, idempotency_key: &str) -> String {
    let digest = hex_digest(idempotency_key.as_bytes());
    format!("mixed-stage-backfill-{kind}-{digest}")
}

fn hex_digest(input: &[u8]) -> String {
    Sha256::digest(input)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
