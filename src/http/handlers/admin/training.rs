use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::Response;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::app::AppState;
use crate::core::apparatus_standard::{
    ApparatusId, ExecutionOperation, RuntimeApparatusConfiguration,
};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::authz::Capability;
use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderTemplate, owner_key, validate_template,
};
use crate::core::production_map::pechat;
use crate::core::production_map::{
    ApparatusQueueInteractionMode, ApparatusQueueOrderActionControl, ApparatusQueuePolicy,
    ApparatusQueuePolicyRecord, ApparatusQueuePreviousWipMode, ApparatusQueueQolipMode,
    ApparatusQueueWorkerInteraction, OrderProgressBatch, OrderProgressBatchStatus,
    OrderProgressBatchStatusDetail, OrderProgressBatchWipStatus, ProductionMapDefinition,
    ProductionMapEdge, ProductionMapLiveSnapshot, ProductionMapNode, ProductionMapNodeKind,
    ProductionMapSaved, ProductionOrderStatusDetail, QueueActionPolicyInput,
    QueueActionPolicyProfile, allowed_actions_for_control, chain, progress_batch_id,
    progress_qr_payload, queue_state,
};
use crate::core::returned_paint::{
    ReturnedPaintItem, calculate_returned_paint, returned_paint_astatka_total,
    returned_paint_report_can_close,
};
use crate::db::postgres_training_workspace::{
    PostgresTrainingWorkspaceStore, TRAINING_VIRTUAL_INPUT_BOSMA,
    TRAINING_VIRTUAL_INPUT_LAMINATSIYA, TrainingImage, TrainingInputBatchIdentity,
    TrainingWorkspaceError,
};

include!("training_parts/part_01.rs");
include!("training_parts/part_02.rs");
include!("training_parts/part_03.rs");
include!("training_parts/part_04.rs");
include!("training_parts/part_05.rs");
include!("training_parts/part_06.rs");
include!("training_parts/part_07.rs");
include!("training_parts/part_08.rs");
