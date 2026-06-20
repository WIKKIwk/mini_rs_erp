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
        ProductionMapError::RawMaterialGroupAmbiguous => {
            bad_request("raw_material_group_ambiguous")
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
        ProductionMapError::RawMaterialScanRequired => bad_request("raw_material_scan_required"),
        ProductionMapError::RawMaterialMismatch => bad_request("raw_material_mismatch"),
        ProductionMapError::ProgressInputInvalid => bad_request("progress_input_invalid"),
        ProductionMapError::ProgressBatchNotFound => not_found("progress_batch_not_found"),
        ProductionMapError::ProgressBatchNotResumable => {
            bad_request("progress_batch_not_resumable")
        }
        ProductionMapError::MapNotFound => not_found("map_not_found"),
        ProductionMapError::StoreFailed => server_error("store failed"),
        other => bad_request(other.to_string()),
    }
}

pub(super) fn gscale_progress_error(error: crate::core::gscale::GscaleServiceError) -> AdminError {
    match error {
        crate::core::gscale::GscaleServiceError::InvalidInput(detail) => bad_request(detail),
        crate::core::gscale::GscaleServiceError::NotConfigured(_) => {
            service_unavailable("scale_driver_not_configured")
        }
        crate::core::gscale::GscaleServiceError::PrintFailed { detail, .. } => {
            failed_dependency(detail)
        }
        crate::core::gscale::GscaleServiceError::EpcGenerationFailed
        | crate::core::gscale::GscaleServiceError::StoreWrite(_)
        | crate::core::gscale::GscaleServiceError::SubmitFailed(_) => {
            failed_dependency(error.to_string())
        }
    }
}

fn service_unavailable(error: impl Into<String>) -> AdminError {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(AdminErrorResponse {
            error: error.into(),
        }),
    )
}

fn failed_dependency(error: impl Into<String>) -> AdminError {
    (
        StatusCode::FAILED_DEPENDENCY,
        Json(AdminErrorResponse {
            error: error.into(),
        }),
    )
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
        PrincipalRole::Admin => "admin",
    }
}

pub(super) fn principal_owner_key(principal: &Principal) -> String {
    let role = principal_role_code(&principal.role);
    owner_key(role, &principal.ref_)
}
