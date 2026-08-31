use sqlx::{PgPool, Postgres, Transaction};

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{
    FinishedGoodsStockEntry, OrderProgressBatch, OrderProgressBatchStatus,
    OrderProgressBatchStatusDetail, OrderProgressBatchWipStatus, OrderProgressEvent,
    OrderRunInputLink, OrderRunInputSourceKind, OrderRunInputStatus, OrderRunSession,
    OrderRunStatus, ProductionMapError, ProductionOrderLogEntry, ProgressBatchCorrectionInput,
    ProgressBatchCorrectionRecord, ProgressBatchInputLink, QueueActionActor,
    order_run_input_links_from_payload, progress_batch_input_links_from_payload, queue_state,
    rezka_active_partial_rolls_from_payload, rezka_merge_state_is_consistent,
};

use super::queue_helpers::{queue_action_as_str, queue_action_from_str};
use super::transaction_locks::lock_order_and_apparatuses_tx;

include!("helpers_parts/part_01.rs");
include!("helpers_parts/part_02.rs");
