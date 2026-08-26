use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{
    ApparatusDowntime, ApparatusQueueActionEvent, ApparatusScheduleCancelRequest,
    ApparatusScheduleReservation, CompletedQueueOrder, CompletionRequestDecision,
    CompletionRequestDecisionNotification, CompletionRequestNotification,
    CompletionRequestStateResolution, FinishedGoodsStockEntry, LaminatsiyaAstatkaReport,
    OrderControlRecord, OrderProgressBatch, OrderProgressEvent, OrderRunSession, PaddonCreateInput,
    PaddonSnapshot, PaddonSummary, ProductionMapApparatusTransferRecord,
    ProductionMapApparatusTransferWrite, ProductionMapDefinition, ProductionMapError,
    ProductionMapStorePort, ProductionOrderLifecycleRecord, ProductionOrderLifecycleStatus,
    ProductionOrderLogEntry, ProgressBatchCorrectionInput, ProgressBatchCorrectionRecord,
    QueueActionActor, QueueActionProgressWrite, QueueActionProgressWriteResult,
    RawMaterialAssignment, RawMaterialStockTransition, RawMaterialStockTransitionKind,
    RezkaAstatkaReport, WipProgressBatchQuery, validate_queue_progress_write,
};
use crate::core::qolip::QolipError;

mod astatka_helpers;
mod capacity_helpers;
mod catalog_helpers;
mod completion_helpers;
mod lifecycle;
mod map_helpers;
mod material_helpers;
mod order_control_helpers;
mod order_query_helpers;
mod paddon_helpers;
mod progress_helpers;
mod qolip_session_helpers;
mod queue_helpers;
mod raw_material_stock_helpers;
mod transaction_locks;
mod transfer_helpers;
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
    load_apparatus_sequences, load_maps, load_maps_by_lifecycle_statuses, save_apparatus_sequence,
};
use self::completion_helpers::{
    load_completion_request_by_event_id, load_completion_request_decisions_for_actor,
    load_completion_requests, resolve_completion_request_decision as resolve_completion_request,
};
use self::lifecycle::{load_production_order_lifecycles, refresh_production_order_lifecycle_tx};
use self::map_helpers::{
    put_map_inner, put_map_inner_tx, reject_duplicate_order_number,
    reject_duplicate_order_number_tx, reject_order_number_immutable,
    reject_order_number_immutable_tx,
};
use self::material_helpers::{
    delete_raw_material_assignment, load_raw_material_assignments, save_raw_material_assignment,
};
use self::order_control_helpers::{
    load_order_control_states, load_order_freeze_requests_for_audit, save_order_control_state,
    save_order_control_state_tx,
};
use self::order_query_helpers::{
    load_active_order_run_session, load_active_order_run_session_for_qolip,
    load_active_order_run_sessions_for_orders, load_active_order_run_sessions_for_worker,
    load_completed_queue_orders_for_actor, load_order_run_session,
    load_order_run_sessions_for_audit, load_order_run_sessions_for_order,
    load_order_run_sessions_for_orders, load_progress_batch, load_progress_batch_by_qr,
    load_progress_batches_for_audit, load_progress_batches_for_order,
    load_progress_batches_for_orders, load_progress_batches_for_worker,
    load_queue_action_logs_for_orders, load_queue_action_logs_for_worker,
};
use self::paddon_helpers::{
    add_paddon_item, add_paddon_items, create_paddon, load_paddon_scan_snapshot,
    load_paddon_snapshot, load_paddon_summary, load_paddons, remove_paddon_item,
    remove_paddon_items,
};
use self::progress_helpers::{
    correct_progress_batch, load_progress_batch_corrections_for_order, put_order_progress_batch,
    put_order_progress_batch_tx, put_order_progress_event, put_order_progress_event_tx,
    put_order_run_session, put_order_run_session_tx, receive_finished_goods_batch_tx,
};
use self::qolip_session_helpers::reject_qolip_in_use_tx;
use self::queue_helpers::{
    insert_queue_action_event_tx, put_queue_action_state_tx, put_queue_states_tx,
    queue_action_event_replay_tx, validate_queue_action_event_transition_tx,
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
}

impl PostgresProductionMapStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

include!("postgres_production_map_impl_parts/part_01.rs");
include!("postgres_production_map_impl_parts/part_02.rs");

include!("postgres_production_map_trait_impl.rs");

fn production_map_qolip_checkout_error(error: QolipError) -> ProductionMapError {
    match error {
        QolipError::LocationNotFound => ProductionMapError::QolipLocationNotFound,
        QolipError::QolipCodeNotFound | QolipError::QolipCodeMismatch => {
            ProductionMapError::QolipCodeMismatch
        }
        QolipError::InsufficientStock => ProductionMapError::QolipInsufficientStock,
        QolipError::LocationIdentityMismatch => ProductionMapError::QolipLocationIdentityMismatch,
        QolipError::QolipInUse => ProductionMapError::QolipAlreadyInUse,
        _ => ProductionMapError::StoreFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qolip_in_use_checkout_error_is_returned_as_business_error() {
        assert_eq!(
            production_map_qolip_checkout_error(QolipError::QolipInUse),
            ProductionMapError::QolipAlreadyInUse
        );
    }
}
