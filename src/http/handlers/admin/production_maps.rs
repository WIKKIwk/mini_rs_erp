use super::*;
use crate::core::apparatus_standard::{
    ApparatusCapacity, ApparatusId, ApparatusOperationalPolicies, CanonicalApparatusPatch,
    QueueDiscipline,
};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderTemplate, owner_key, validate_template,
};
use crate::core::formula::{CalculateRequest, calculate_with_material_catalog};
use crate::core::gscale::models::{
    ProgressLabelPrintRequest, RawMaterialStockEntry, RawMaterialStockUpdateInput,
};
use crate::core::production_map::{
    ApparatusDowntime, ApparatusScheduleCancelRequest, ApparatusScheduleRequest,
    CompletionRequestDecision, MaterialScanProgressAction, OrderProgressBatchWipStatus,
    ProductionMapApparatusTransferRequest, ProductionMapBatchMoveRequest, ProductionMapDefinition,
    ProductionMapError, ProductionMapMoveRequest, ProductionMapNodeKind, ProductionMapRunRequest,
    QueueActionActor, QueueProgressInput, RawMaterialAssignment, RawMaterialAssignmentDeleteInput,
    RawMaterialAssignmentInput, RawMaterialStockTransition, RawMaterialStockTransitionKind,
    RezkaFrameProgressInput, TrustedQolipStartValidation, WipProgressBatchQuery, queue_state,
};
use crate::google_sheets::is_sheet_order_map;

mod astatka;
mod completion;
mod helpers;
mod move_run;
mod order_control;
mod paddons;
mod progress_qr;
mod qolip_order_notes;
mod qolip_validation;
mod queue_actions;
mod raw_material_details;
mod raw_material_reprint;
mod raw_materials;
mod wip;

pub use self::astatka::{production_map_laminatsiya_astatka, production_map_rezka_astatka};
pub use self::completion::{
    production_map_closed_orders, production_map_completed_orders,
    production_map_completion_request_decision, production_map_completion_request_decisions,
    production_map_completion_requests, production_map_live,
};
use self::helpers::*;
pub use self::move_run::{
    production_map_apparatus_transfer, production_map_move, production_map_move_batch,
    production_map_run,
};
pub use self::order_control::production_map_order_control;
pub use self::paddons::{
    production_map_paddon_create, production_map_paddon_detail, production_map_paddon_item_add,
    production_map_paddon_item_remove, production_map_paddon_items_add,
    production_map_paddon_items_remove, production_map_paddon_qr_print,
    production_map_paddon_qr_report, production_map_paddons,
};
pub use self::progress_qr::{
    production_map_progress_batch_correct, production_map_progress_qr_history,
    production_map_progress_qr_lookup, production_map_progress_qr_report,
    production_map_progress_qr_reprint,
};
pub use self::qolip_order_notes::production_map_qolip_order_notes;
pub use self::qolip_validation::production_map_qolip_validate;
pub use self::queue_actions::production_map_queue_action;
pub use self::raw_material_reprint::{
    raw_material_stock_reprint_confirm, raw_material_stock_reprint_prepare,
};
pub use self::raw_materials::{
    raw_material_assignment_candidate_orders, raw_material_assignment_candidates,
    raw_material_assignment_lookup, raw_material_assignment_orders, raw_material_assignments,
    raw_material_history, raw_material_intake, raw_material_intake_candidates, raw_material_rules,
    raw_material_start_requirements, raw_material_stock,
};
pub use self::wip::{production_map_finished_goods_receive, production_map_wip_batches};

include!("production_maps_parts/part_01.rs");
include!("production_maps_parts/part_02.rs");
