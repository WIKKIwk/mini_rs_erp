use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;

use crate::core::apparatus_standard::ApparatusId;
use crate::core::qolip::QolipCheckout;
use crate::core::returned_paint::ReturnedPaintRequest;

use super::capacity::*;
use super::materials::RawMaterialAssignment;
use super::opening_wip::*;
use super::types::*;
use super::LaminationSettings;
use super::BosmaAstatkaReport;
use super::print_preflight::PrintPreflightHold;


pub type StoreResult<T> = Result<T, ProductionMapError>;
pub type ApparatusSequenceMap = BTreeMap<String, Vec<String>>;
pub type QueueStateMap = BTreeMap<String, String>;
pub type ApparatusQueueStateMap = BTreeMap<String, QueueStateMap>;
pub type OrderLogMap = BTreeMap<String, Vec<ProductionOrderLogEntry>>;
pub type OrderControlMap = BTreeMap<String, OrderControlRecord>;

/// Snapshot consumers need the last state and last freeze for each stage, not
/// the intervening audit history. Preserve source order to keep timestamp ties
/// and the latest freeze across stages identical to the complete history.
pub(crate) fn queue_snapshot_logs_from_history(history: OrderLogMap) -> OrderLogMap {
    history
        .into_iter()
        .map(|(order_id, logs)| {
            let mut latest = BTreeMap::new();
            let mut freezes = BTreeMap::new();
            for (index, log) in logs.iter().enumerate() {
                latest.insert(log.stage_node_id.clone(), index);
                if log.action == super::queue_state::ApparatusQueueAction::Freeze
                    && log.to_state == super::queue_state::ApparatusQueueOrderState::Frozen
                {
                    freezes.insert(log.stage_node_id.clone(), index);
                }
            }
            let keep = latest.into_values().chain(freezes.into_values()).collect::<BTreeSet<_>>();
            (order_id, logs.into_iter().enumerate()
                .filter_map(|(index, log)| keep.contains(&index).then_some(log)).collect())
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawMaterialStockTransitionKind {
    InUse,
    Consumed,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMaterialStockTransition {
    pub kind: RawMaterialStockTransitionKind,
    pub barcodes: Vec<String>,
    pub order_id: String,
}

impl RawMaterialStockTransition {
    pub fn new(
        kind: RawMaterialStockTransitionKind,
        barcodes: Vec<String>,
        order_id: &str,
    ) -> Self {
        Self {
            kind,
            barcodes: barcodes
                .into_iter()
                .map(|barcode| barcode.trim().to_string())
                .filter(|barcode| !barcode.is_empty())
                .collect(),
            order_id: order_id.trim().to_string(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.barcodes.is_empty() || self.order_id.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueActionProgressWriteResult {
    pub raw_material_stock_warehouses: Vec<String>,
    pub raw_material_stock_committed: bool,
}

#[derive(Debug, Clone)]
pub struct QueueActionProgressWrite {
    pub apparatus: String,
    /// An alternative-assignment map replacement that must commit with the
    /// queue/session/progress/material write. PostgreSQL applies this through
    /// the same database transaction; test stores apply it at the same store
    /// boundary rather than exposing a caller-managed rollback.
    pub map_update: Option<ProductionMapDefinition>,
    pub states: QueueStateMap,
    /// Persisted sequence replacements that belong to the same queue write.
    /// PostgreSQL applies these in the same transaction as the queue state,
    /// event, session, and control transition.
    pub sequence_updates: BTreeMap<String, Vec<String>>,
    pub event: ApparatusQueueActionEvent,
    pub session: Option<OrderRunSession>,
    pub progress_event: Option<OrderProgressEvent>,
    pub progress_batch: Option<OrderProgressBatch>,
    pub progress_batches: Vec<OrderProgressBatch>,
    pub progress_batch_updates: Vec<OrderProgressBatch>,
    pub opening_wip_batch_updates: Vec<OpeningWipBatch>,
    pub raw_material_stock_transitions: Vec<RawMaterialStockTransition>,
    pub qolip_checkouts: Vec<QolipCheckout>,
    pub returned_paint_report: Option<ReturnedPaintRequest>,
    pub order_control_update: Option<OrderControlRecord>,
    pub schedule_reservation_status: Option<ApparatusScheduleStatus>,
    /// A passed print-preflight reservation is consumed in the same commit as
    /// the official Start transition.
    pub print_preflight_hold_id: Option<String>,
    /// An active colour trial is cancelled in the same commit as the freeze
    /// transition. Its queue row is written as `frozen`, so cancellation must
    /// not restore the hold's previous queue state.
    pub print_preflight_cancel_hold_id: Option<String>,
}

pub struct ProductionMapApparatusTransferWrite {
    pub record: ProductionMapApparatusTransferRecord,
    pub expected_map: Option<ProductionMapDefinition>,
    pub expected_session: Option<OrderRunSession>,
    pub expected_apparatus_revisions: Vec<(String, u64)>,
    pub print_target_configuration: Option<crate::core::apparatus_standard::RuntimeApparatusConfiguration>,
    pub expected_input_batches: Vec<OrderProgressBatch>,
    pub opening_wip_batch_updates: Vec<OpeningWipBatch>,
    pub updated_map: ProductionMapDefinition,
    pub from_sequence: Vec<String>,
    pub to_sequence: Vec<String>,
    pub from_states: QueueStateMap,
    pub to_states: QueueStateMap,
    pub target_apparatus_id: String,
    pub session: OrderRunSession,
    pub progress_batch: Option<OrderProgressBatch>,
    pub progress_batch_updates: Vec<OrderProgressBatch>,
    pub raw_material_assignments: Vec<RawMaterialAssignment>,
}

pub(crate) fn validate_queue_progress_write(write: &QueueActionProgressWrite) -> StoreResult<()> {
    require_live_apparatus(&write.apparatus)?;
    require_queue_event_apparatus(&write.event)?;
    for apparatus in write.sequence_updates.keys() {
        require_live_apparatus(apparatus)?;
    }
    if let Some(session) = &write.session {
        require_live_apparatus(&session.apparatus)?;
    }
    if let Some(event) = &write.progress_event {
        require_live_apparatus(&event.apparatus)?;
    }
    for batch in write
        .progress_batch
        .iter()
        .chain(write.progress_batches.iter())
        .chain(write.progress_batch_updates.iter())
    {
        require_progress_batch_apparatus(batch)?;
    }
    for batch in &write.opening_wip_batch_updates {
        if batch.order_id.trim().is_empty() || batch.batch_id.trim().is_empty() {
            return Err(ProductionMapError::StoreFailed);
        }
        for apparatus in [
            batch.used_by_apparatus.as_str(),
            batch.processed_by_apparatus.as_str(),
        ] {
            if !apparatus.trim().is_empty() {
                require_live_apparatus(apparatus)?;
            }
        }
    }
    if let Some(report) = &write.returned_paint_report {
        require_live_apparatus(&report.apparatus)?;
    }
    if let Some(record) = &write.order_control_update
        && let Some(request) = &record.freeze_request
        && !request.target_apparatus.trim().is_empty()
    {
        require_live_apparatus(&request.target_apparatus)?;
    }
    Ok(())
}

fn require_live_apparatus(value: &str) -> StoreResult<ApparatusId> {
    ApparatusId::new(value.trim().to_string()).map_err(|_| ProductionMapError::StoreFailed)
}

fn require_queue_event_apparatus(event: &ApparatusQueueActionEvent) -> StoreResult<()> {
    require_live_apparatus(&event.apparatus)?;
    for apparatus in &event.assigned_apparatus {
        require_live_apparatus(apparatus)?;
    }
    Ok(())
}

fn require_progress_batch_apparatus(batch: &OrderProgressBatch) -> StoreResult<()> {
    require_live_apparatus(&batch.apparatus)?;
    for apparatus in [
        batch.current_apparatus.as_str(),
        batch.next_apparatus.as_str(),
        batch.used_by_apparatus.as_str(),
        batch.processed_by_apparatus.as_str(),
    ] {
        if !apparatus.trim().is_empty() && !is_warehouse_processing_marker(apparatus) {
            require_live_apparatus(apparatus)?;
        }
    }
    Ok(())
}

fn is_warehouse_processing_marker(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("warehouse:")
}

// Compose the domain sections before async_trait expands the complete store interface.
include!("store_port_parts/production_map_catalog_methods.rs");
include!("store_port_parts/production_map_queue_methods.rs");
include!("store_port_parts/production_map_wip_methods.rs");


#[cfg(test)]
#[path = "store_port_tests.rs"]
mod tests;
