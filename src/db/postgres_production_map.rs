use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::PgPool;

mod output_stats;

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{
    ApparatusDowntime, ApparatusQueueActionEvent, ApparatusScheduleCancelRequest,
    ApparatusScheduleReservation, CompletedQueueOrder, CompletionRequestDecision,
    CompletionRequestDecisionNotification, CompletionRequestNotification,
    CompletionRequestStateResolution, FinishedGoodsStockEntry, LaminatsiyaAstatkaReport,
    OpeningWipBatchRecord, OpeningWipCreateWrite, OpeningWipDeleteWrite, OpeningWipQuery,
    OpeningWipRecord, OrderControlRecord, OrderProgressBatch, OrderProgressEvent, OrderRunSession,
    PaddonCreateInput, PaddonManagementSettings, PaddonSnapshot, PaddonSummary, ProductionMapApparatusTransferRecord,
    ProductionMapApparatusTransferWrite, ProductionMapDefinition, ProductionMapError,
    ProductionMapStorePort, ProductionOrderLifecycleRecord, ProductionOrderLifecycleStatus,
    ProductionOrderLogEntry, ProductionQrSessionResources, ProgressBatchCorrectionInput, ProgressBatchCorrectionRecord,
    QueueActionActor, QueueActionProgressWrite, QueueActionProgressWriteResult,
    RawMaterialAssignment, RawMaterialStockTransition, RawMaterialStockTransitionKind,
    RezkaAstatkaReport, WipProgressBatchQuery, validate_queue_progress_write,
};
use crate::core::qolip::QolipError;
use crate::core::production_map::BosmaAstatkaReport;

#[path = "postgres_production_map/astatka/helpers.rs"]
mod astatka_helpers;
#[path = "postgres_production_map/capacity/helpers.rs"]
mod capacity_helpers;
#[path = "postgres_production_map/catalog/catalog.rs"]
mod catalog_helpers;
#[path = "postgres_production_map/completion/requests.rs"]
mod completion_helpers;
mod lifecycle;
pub(crate) mod realtime;
mod layer_remainder;
mod lamination_astatka;
mod lamination_settings;
mod qolip_start_settings;
mod sequence_move;
mod re_pause;
mod temporary_pause;
mod cancel_start;
mod scanned_wip;
mod reactivation;
#[cfg(test)]
mod sequence_move_tests;
mod stage_execution;
pub(crate) mod pending_orders;
#[path = "postgres_production_map/catalog/maps.rs"]
mod map_helpers;
#[path = "postgres_production_map/materials/rules.rs"]
mod material_helpers;
mod opening_wip_helpers;
#[path = "postgres_production_map/order_control/helpers.rs"]
mod order_control_helpers;
#[path = "postgres_production_map/order_query/helpers.rs"]
mod order_query_helpers;
#[path = "postgres_production_map/paddon/helpers.rs"]
mod paddon_helpers;
#[path = "postgres_production_map/paddon/weights.rs"]
mod paddon_weights;
#[path = "postgres_production_map/paddon/delete.rs"]
mod paddon_delete;
#[path = "postgres_production_map/paddon/active.rs"]
mod active_paddon;
#[path = "postgres_production_map/paddon/print_lock.rs"]
mod paddon_print_lock;
#[path = "postgres_production_map/paddon/management.rs"]
mod paddon_management;
#[path = "postgres_production_map/paddon/movement.rs"]
mod paddon_movement;
#[path = "postgres_production_map/paddon/transfer.rs"]
mod paddon_transfer;
#[path = "postgres_production_map/paddon/receipts.rs"]
mod paddon_receipts;
#[path = "postgres_production_map/print_preflight.rs"]
mod print_preflight;
#[path = "postgres_production_map/progress/helpers.rs"]
mod progress_helpers;
#[path = "postgres_production_map/paddon/output_assignment.rs"]
mod output_paddon_assignment;
#[path = "postgres_production_map/qolip/session_helpers.rs"]
mod qolip_session_helpers;
#[path = "postgres_production_map/qolip/returns.rs"]
mod qolip_returns;
pub(crate) use qolip_returns::{prepare_qolip_return_tx, release_returned_qolips_tx};
#[path = "postgres_production_map/qolip/rework_availability.rs"]
mod qolip_rework_availability;
#[path = "postgres_production_map/queue/helpers.rs"]
mod queue_helpers;
#[path = "postgres_production_map/materials/stock.rs"]
mod raw_material_stock_helpers;
mod transaction_locks;
mod warehouse_wip;
pub mod material_link_requests;
pub(crate) mod worker_material_use;

pub(crate) async fn save_edited_map_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    map: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    reject_order_number_immutable_tx(tx, map).await?;
    put_map_inner_tx(tx, map).await
}

pub(crate) async fn create_order_maps_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    map: &ProductionMapDefinition,
    template_map: Option<&ProductionMapDefinition>,
) -> Result<(), ProductionMapError> {
    transaction_locks::lock_order_tx(tx, &map.id).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM mini_production_maps WHERE id=$1)
            OR EXISTS(SELECT 1 FROM mini_orders WHERE id=$1)",
    )
    .bind(&map.id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if exists {
        return Err(ProductionMapError::DuplicateOrderNumber);
    }
    for map in std::iter::once(map).chain(template_map) {
        reject_order_number_immutable_tx(tx, map).await?;
        reject_duplicate_order_number_tx(tx, map).await?;
        put_map_inner_tx(tx, map).await?;
    }
    Ok(())
}

pub(crate) async fn lock_order_for_edit_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, id: &str,
) -> Result<(), ProductionMapError> {
    transaction_locks::lock_order_tx(tx, id).await
}
#[path = "postgres_production_map/transfer/helpers.rs"]
mod transfer_helpers;
#[path = "postgres_production_map/wip/query_helpers.rs"]
mod wip_query_helpers;

use self::astatka_helpers::{
    load_laminatsiya_astatka_reports_for_order, load_rezka_astatka_reports_for_order,
    put_laminatsiya_astatka_report, put_rezka_astatka_report,
};
use self::capacity_helpers::{
    cancel_apparatus_schedule_reservation, load_apparatus_downtimes,
    load_apparatus_schedule_reservation_by_idempotency_key, load_apparatus_schedule_reservations,
    put_apparatus_downtime, put_apparatus_schedule_reservation,
    update_apparatus_schedule_reservation_status_tx,
};
use self::catalog_helpers::{
    apply_apparatus_sequence_delta_tx, delete_map_by_id, load_apparatus_queue_states,
    load_apparatus_sequences, load_maps, load_maps_by_lifecycle_statuses,
    load_raw_material_assignment_maps, save_apparatus_sequence,
};
use self::completion_helpers::{
    load_completion_request_by_event_id, load_completion_request_decisions_for_actor,
    load_completion_requests, resolve_completion_request_decision as resolve_completion_request,
};
pub(crate) use self::lifecycle::{
    load_production_order_lifecycles, refresh_production_order_lifecycle_tx,
};
use self::map_helpers::{
    put_map_inner, put_map_inner_tx, reject_duplicate_order_number,
    reject_duplicate_order_number_tx, reject_order_number_immutable,
    reject_order_number_immutable_tx,
};
use self::material_helpers::{
    delete_raw_material_assignment, load_raw_material_assignments, save_raw_material_assignment,
};
use self::opening_wip_helpers::{
    create_opening_wip, delete_opening_wip_batch, load_opening_wip_batch,
    load_opening_wip_by_idempotency_key, load_opening_wip_records, update_opening_wip_batch_tx,
};
use self::order_control_helpers::{
    load_order_control_states, load_order_freeze_requests_for_audit, save_order_control_state,
    save_order_control_state_tx,
};
use self::order_query_helpers::{
    load_active_order_run_session, load_active_order_run_session_for_qolip,
    load_active_order_run_sessions_for_orders, load_active_order_run_sessions_for_worker,
    load_completed_queue_orders_for_actor, load_completed_queue_orders_for_apparatuses,
    load_order_run_session,
    load_order_run_sessions_for_audit, load_order_run_sessions_for_order,
    load_order_run_sessions_for_orders, load_progress_batch, load_progress_batch_by_qr,
    load_progress_batches_for_audit, load_progress_batches_for_order,
    load_progress_batches_for_orders, load_progress_batches_for_worker,
    load_queue_action_logs_for_orders, load_queue_action_logs_for_worker,
    load_queue_snapshot_logs_for_orders,
};
use self::paddon_helpers::{
    add_paddon_item, add_paddon_items, create_paddon, load_paddon_scan_snapshot,
    load_paddon_snapshot, load_paddon_summary, load_paddons, remove_paddon_item,
    remove_paddon_items,
};
use self::print_preflight::{
    cancel_print_preflight_hold_tx, consume_print_preflight_hold_tx,
    load_active as load_active_print_preflight_holds,
    load_by_id as load_print_preflight_hold_by_id,
    load_by_idempotency_key as load_print_preflight_hold_by_idempotency_key,
    put as put_print_preflight_hold, update as update_print_preflight_hold,
};
use self::progress_helpers::{
    correct_progress_batch, load_progress_batch_corrections_for_order, put_order_progress_batch,
    put_order_progress_batch_tx, put_order_progress_event, put_order_progress_event_tx,
    put_order_run_session, put_order_run_session_tx, receive_finished_goods_batch_tx,
};
use self::qolip_session_helpers::reject_qolip_in_use_tx;
use self::queue_helpers::{
    insert_queue_action_event_tx, put_queue_action_state_tx, put_queue_states_tx,
    queue_action_event_replay_tx, validate_merge_session_transition_tx,
    validate_queue_action_event_transition_tx,
};
use self::raw_material_stock_helpers::apply_raw_material_stock_transitions_tx;
use self::transaction_locks::{lock_order_and_apparatuses_tx, lock_orders_and_apparatuses_tx};
use self::transfer_helpers::{
    commit_apparatus_transfer as commit_apparatus_transfer_record,
    load_apparatus_transfer_by_idempotency_key, load_apparatus_transfers_for_audit,
};
use self::wip_query_helpers::load_wip_progress_batches;

#[derive(Clone)]
pub struct PostgresProductionMapStore {
    pool: PgPool,
    // Replays pre-optimization reads for DB parity and A/B benchmarks only.
    #[cfg(test)]
    legacy_queue_reads: bool,
}

impl PostgresProductionMapStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, #[cfg(test)] legacy_queue_reads: false }
    }

    pub async fn reconcile_alternative_order_lifecycles(&self) -> Result<usize, ProductionMapError> {
        lifecycle::reconcile_alternative_order_lifecycles(&self.pool).await
    }

    pub async fn reconcile_frozen_qolip_returns(&self) -> Result<usize, ProductionMapError> {
        progress_helpers::reconcile_frozen_qolip_returns(&self.pool).await
    }

    #[cfg(test)]
    pub(crate) fn with_legacy_queue_reads(mut self) -> Self {
        self.legacy_queue_reads = true;
        self
    }
}

include!("postgres_production_map_parts/maps.rs");

include!("postgres_production_map_parts/queue_action_logs_for_orders.rs");

include!("postgres_production_map_parts/create_opening_wip.rs");

#[cfg(test)]
#[path = "postgres_production_map/qolip/alternative_sets_tests.rs"]
mod qolip_alternative_sets_tests;

#[cfg(test)]
#[path = "postgres_production_map_tests.rs"]
mod tests;

include!("postgres_production_map_parts/apply_queue_progress_tx.rs");
