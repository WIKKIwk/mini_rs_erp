use crate::core::auth::models::PrincipalRole;

use super::*;

pub(super) fn raw_material_stock_status_error(
    error: crate::core::gscale::GscaleServiceError,
) -> AdminError {
    match error {
        crate::core::gscale::GscaleServiceError::InvalidInput(detail) => bad_request(detail),
        _ => server_error("raw material stock status update failed"),
    }
}

pub(super) fn calculate_order_error(error: CalculateOrderError) -> AdminError {
    match error {
        CalculateOrderError::InvalidInput(detail) => bad_request(detail),
        CalculateOrderError::StoreFailed => server_error("calculate order save failed"),
    }
}

pub(super) fn production_map_error(error: ProductionMapError) -> AdminError {
    match error {
        ProductionMapError::DuplicateOrderNumber => bad_request("duplicate_order_number"),
        ProductionMapError::OrderNumberImmutable => bad_request("order_number_immutable"),
        ProductionMapError::MoveNotAllowed => bad_request("move_not_allowed"),
        ProductionMapError::QueueActionNotAllowed => bad_request("queue_action_not_allowed"),
        ProductionMapError::PreviousStageNotCompleted => {
            bad_request("previous_stage_not_completed")
        }
        ProductionMapError::ApparatusNotAssigned => bad_request("apparatus_not_assigned"),
        ProductionMapError::LaminatsiyaRubberTooLarge => {
            bad_request("laminatsiya_rubber_too_large")
        }
        ProductionMapError::ApparatusQueuePolicyLocked => bad_request("queue_policy_locked"),
        ProductionMapError::RawMaterialInvalidInput => bad_request("raw_material_invalid_input"),
        ProductionMapError::RawMaterialGroupNotAllowed => {
            bad_request("raw_material_group_not_allowed")
        }
        ProductionMapError::RawMaterialGroupAmbiguous(apparatuses) => {
            ambiguous_raw_material_apparatuses(apparatuses)
        }
        ProductionMapError::RawMaterialAlreadyAssigned => {
            bad_request("raw_material_already_assigned")
        }
        ProductionMapError::RawMaterialAlreadyAssignedToOrder => {
            bad_request("raw_material_already_assigned_to_order")
        }
        ProductionMapError::RawMaterialAssignmentNotFound => {
            bad_request("raw_material_assignment_not_found")
        }
        ProductionMapError::RawMaterialAssignmentLocked => {
            bad_request("raw_material_assignment_locked")
        }
        ProductionMapError::RawMaterialStockUnavailable => {
            bad_request("raw_material_stock_unavailable")
        }
        ProductionMapError::QolipLocationNotFound => bad_request("qolip_location_not_found"),
        ProductionMapError::QolipCodeMismatch => bad_request("qolip_code_mismatch"),
        ProductionMapError::QolipInsufficientStock => bad_request("insufficient_stock"),
        ProductionMapError::QolipLocationIdentityMismatch => {
            bad_request("location_identity_mismatch")
        }
        ProductionMapError::RawMaterialScanRequired => bad_request("raw_material_scan_required"),
        ProductionMapError::RawMaterialMismatch => bad_request("raw_material_mismatch"),
        ProductionMapError::RawMaterialRollSizeMissing => {
            bad_request("raw_material_roll_size_missing")
        }
        ProductionMapError::RawMaterialRollSizeMismatch => {
            bad_request("raw_material_roll_size_mismatch")
        }
        ProductionMapError::ProgressInputInvalid => bad_request("progress_input_invalid"),
        ProductionMapError::ProgressQrRequired => bad_request("progress_qr_required"),
        ProductionMapError::BosmaCompletionMetricsRequired => {
            bad_request("bosma_completion_metrics_required")
        }
        ProductionMapError::LaminatsiyaCompletionMetricsRequired => {
            bad_request("laminatsiya_completion_metrics_required")
        }
        ProductionMapError::RezkaProgressMetricsRequired => {
            bad_request("rezka_progress_metrics_required")
        }
        ProductionMapError::ProgressBatchNotFound => not_found("progress_batch_not_found"),
        ProductionMapError::ProgressBatchNotAccepted => bad_request("progress_batch_not_accepted"),
        ProductionMapError::ProgressBatchNotResumable => {
            bad_request("progress_batch_not_resumable")
        }
        ProductionMapError::MapNotFound => not_found("map_not_found"),
        ProductionMapError::StoreFailed => server_error("store failed"),
        other => bad_request(other.to_string()),
    }
}

fn ambiguous_raw_material_apparatuses(apparatuses: Vec<String>) -> AdminError {
    (
        StatusCode::BAD_REQUEST,
        Json(AdminErrorResponse {
            error: "raw_material_group_ambiguous".to_string(),
            apparatus_options: Some(apparatuses),
            order_width_mm: None,
            roll_width_mm: None,
        }),
    )
}

pub(super) fn warehouse_error(error: WarehouseError) -> AdminError {
    match error {
        WarehouseError::MissingWarehouse => bad_request("warehouse is required"),
        WarehouseError::MissingPrincipalRef => bad_request("principal ref is required"),
        WarehouseError::StoreFailed => server_error("warehouse store failed"),
    }
}

pub(super) fn queue_action_actor(principal: &Principal) -> QueueActionActor {
    QueueActionActor {
        role: principal_role_code(&principal.role).to_string(),
        ref_: principal.ref_.trim().to_string(),
        display_name: principal.display_name.trim().to_string(),
    }
}

fn principal_role_code(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    }
}

pub(super) fn principal_owner_key(principal: &Principal) -> String {
    let role = principal_role_code(&principal.role);
    owner_key(role, &principal.ref_)
}
