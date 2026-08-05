use std::collections::{BTreeMap, BTreeSet};

use crate::core::auth::models::PrincipalRole;
use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::production_map::{ProductionMapDefinition, ProductionMapSaved};

use super::*;

pub(super) async fn production_map_order_customers(
    state: &AppState,
    maps: &[ProductionMapSaved],
) -> BTreeMap<String, String> {
    let order_maps = maps
        .iter()
        .filter(|saved| is_customer_order_map(&saved.map))
        .collect::<Vec<_>>();
    let mut customers = order_maps
        .iter()
        .filter_map(|saved| {
            let map_id = saved.map.id.trim();
            let customer = saved.map.customer_name.trim();
            (!map_id.is_empty() && !customer.is_empty())
                .then(|| (map_id.to_string(), customer.to_string()))
        })
        .collect::<BTreeMap<_, _>>();

    if customers.len() == order_maps.len() {
        return customers;
    }
    let templates = match state.calculate_orders.list_all().await {
        Ok(templates) => templates,
        Err(error) => {
            tracing::warn!(%error, "production map customer fallback load failed");
            return customers;
        }
    };
    for saved in order_maps {
        let map_id = saved.map.id.trim();
        if map_id.is_empty() || customers.contains_key(map_id) {
            continue;
        }
        if let Some(customer) = resolve_production_map_customer(&saved.map, &templates) {
            customers.insert(map_id.to_string(), customer);
        }
    }
    customers
}

fn is_customer_order_map(map: &ProductionMapDefinition) -> bool {
    let map_id = map.id.trim();
    !map_id.is_empty()
        && !map_id.starts_with("template-")
        && (map_id.starts_with("zakaz-")
            || !map.order_number.trim().is_empty()
            || !map.code.trim().is_empty())
}

fn resolve_production_map_customer(
    map: &ProductionMapDefinition,
    templates: &[CalculateOrderTemplate],
) -> Option<String> {
    let map_id = map.id.trim();
    let template_map_id = (!map_id.is_empty()).then(|| format!("template-{map_id}"));
    let source_matches = templates
        .iter()
        .filter(|template| {
            let source_map_id = template.source_map_id.trim();
            source_map_id == map_id
                || template_map_id
                    .as_deref()
                    .is_some_and(|template_map_id| source_map_id == template_map_id)
        })
        .collect::<Vec<_>>();
    if !source_matches.is_empty() {
        return unique_template_customer(source_matches.into_iter());
    }

    let id_suffix = map_id.strip_prefix("zakaz-").unwrap_or("").trim();
    let order_keys = [map.order_number.trim(), map.code.trim(), id_suffix]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    let order_matches = templates
        .iter()
        .filter(|template| {
            !order_keys.is_empty()
                && (order_keys.contains(template.order_number.trim())
                    || order_keys.contains(template.code.trim()))
        })
        .collect::<Vec<_>>();
    if !order_matches.is_empty() {
        return unique_template_customer(order_matches.into_iter());
    }

    let mut product_keys = [map.product_code.as_str(), map.title.as_str()]
        .into_iter()
        .map(normalized_customer_match_key)
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    product_keys.extend(
        map.nodes
            .iter()
            .map(|node| normalized_customer_match_key(&node.item_code))
            .filter(|value| !value.is_empty()),
    );
    if product_keys.is_empty() {
        return None;
    }
    unique_template_customer(templates.iter().filter(|template| {
        let product_matches = product_keys.contains(&normalized_customer_match_key(
            &template.product,
        )) || product_keys.contains(&normalized_customer_match_key(&template.item_code));
        if !product_matches {
            return false;
        }
        match (map.width_mm, template.width_mm) {
            (Some(map_width), template_width)
                if map_width > 0.0 && template_width > 0.0 =>
            {
                (map_width - template_width).abs() <= 0.5
            }
            _ => true,
        }
    }))
}

fn unique_template_customer<'a>(
    templates: impl Iterator<Item = &'a CalculateOrderTemplate>,
) -> Option<String> {
    let customers = templates
        .filter_map(|template| {
            let customer = template.customer.trim();
            (!customer.is_empty()).then(|| {
                (
                    normalized_customer_match_key(customer),
                    customer.to_string(),
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    (customers.len() == 1)
        .then(|| customers.into_values().next())
        .flatten()
}

fn normalized_customer_match_key(value: &str) -> String {
    value.trim().to_lowercase()
}

#[cfg(test)]
mod customer_resolution_tests {
    use super::*;

    fn test_map() -> ProductionMapDefinition {
        serde_json::from_value(serde_json::json!({
            "id": "zakaz-8768",
            "product_code": "YASHIL",
            "title": "yashil",
            "order_number": "8768"
        }))
        .expect("test production map")
    }

    #[test]
    fn resolves_legacy_order_from_its_template_map() {
        let template = CalculateOrderTemplate {
            customer: "555 kukuruz".to_string(),
            source_map_id: "template-zakaz-8768".to_string(),
            ..CalculateOrderTemplate::default()
        };

        assert_eq!(
            resolve_production_map_customer(&test_map(), &[template]).as_deref(),
            Some("555 kukuruz")
        );
    }

    #[test]
    fn refuses_ambiguous_product_customer_fallback() {
        let templates = [
            CalculateOrderTemplate {
                customer: "Customer A".to_string(),
                product: "yashil".to_string(),
                ..CalculateOrderTemplate::default()
            },
            CalculateOrderTemplate {
                customer: "Customer B".to_string(),
                product: "yashil".to_string(),
                ..CalculateOrderTemplate::default()
            },
        ];

        assert_eq!(resolve_production_map_customer(&test_map(), &templates), None);
    }
}

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
        ProductionMapError::MissingId => bad_request("map_id_required"),
        ProductionMapError::MissingProductCode => bad_request("map_product_code_required"),
        ProductionMapError::MissingTitle => bad_request("map_title_required"),
        ProductionMapError::MissingStart => bad_request("map_start_required"),
        ProductionMapError::MissingEnd => bad_request("map_end_required"),
        ProductionMapError::DuplicateNode(_) => bad_request("duplicate_node_id"),
        ProductionMapError::DuplicateOrderNumber => bad_request("duplicate_order_number"),
        ProductionMapError::OrderNumberImmutable => bad_request("order_number_immutable"),
        ProductionMapError::MissingEdgeNode(_) => bad_request("missing_edge_node"),
        ProductionMapError::Cycle => bad_request("production_map_cycle"),
        ProductionMapError::MissingFormulaTarget => bad_request("formula_target_required"),
        ProductionMapError::MissingFormulaExpression => bad_request("formula_expression_required"),
        ProductionMapError::InvalidFormulaTarget(_) => bad_request("invalid_formula_target"),
        ProductionMapError::InvalidFormulaExpression(_) => {
            bad_request("invalid_formula_expression")
        }
        ProductionMapError::MapNotFound => not_found("map_not_found"),
        ProductionMapError::InvalidOrderQty => bad_request("invalid_order_qty"),
        ProductionMapError::InvalidNodeQty(_) => bad_request("invalid_node_qty"),
        ProductionMapError::InvalidLocation(_) => bad_request("invalid_location"),
        ProductionMapError::UnknownFormulaVariable(_) => bad_request("unknown_formula_variable"),
        ProductionMapError::FormulaDivisionByZero => bad_request("formula_division_by_zero"),
        ProductionMapError::MissingConditionBranch => bad_request("condition_branch_required"),
        ProductionMapError::MoveNotAllowed => bad_request("move_not_allowed"),
        ProductionMapError::StartedOrderMoveRequiresTransfer => {
            conflict("started_order_move_requires_transfer")
        }
        ProductionMapError::StartedProductionMapStageLocked => {
            conflict("production_map_started_stage_locked")
        }
        ProductionMapError::ApparatusTransferReasonRequired => {
            bad_request("apparatus_transfer_reason_required")
        }
        ProductionMapError::ApparatusTransferIdempotencyRequired => {
            bad_request("apparatus_transfer_idempotency_required")
        }
        ProductionMapError::ApparatusTransferIdempotencyConflict => {
            conflict("apparatus_transfer_idempotency_conflict")
        }
        ProductionMapError::ApparatusTransferOrderNotPaused => {
            conflict("apparatus_transfer_order_not_paused")
        }
        ProductionMapError::ApparatusTransferSessionNotFound => {
            conflict("apparatus_transfer_session_not_found")
        }
        ProductionMapError::ApparatusTransferProgressNotFound => {
            conflict("apparatus_transfer_progress_not_found")
        }
        ProductionMapError::ApparatusTransferSessionMismatch => {
            conflict("apparatus_transfer_session_mismatch")
        }
        ProductionMapError::ApparatusTransferProgressMismatch => {
            conflict("apparatus_transfer_progress_mismatch")
        }
        ProductionMapError::ApparatusTransferTargetConflict => {
            conflict("apparatus_transfer_target_conflict")
        }
        ProductionMapError::StoreFailed => server_error("store_failed"),
        ProductionMapError::QueueActionNotAllowed => bad_request("queue_action_not_allowed"),
        ProductionMapError::OrderNotStarted => conflict("order_not_started"),
        ProductionMapError::OrderAlreadyCompleted => conflict("order_already_completed"),
        ProductionMapError::OrderFreezeRequested => conflict("order_freeze_requested"),
        ProductionMapError::OrderFrozen => conflict("order_frozen"),
        ProductionMapError::OrderControlActionNotAllowed => {
            conflict("order_control_action_not_allowed")
        }
        ProductionMapError::OrderFreezeTargetNotFound => conflict("order_freeze_target_not_found"),
        ProductionMapError::OrderFreezeTargetAmbiguous => conflict("order_freeze_target_ambiguous"),
        ProductionMapError::OrderFreezeRequestMismatch => conflict("order_freeze_request_mismatch"),
        ProductionMapError::OrderDeleteBlocked(blockers) => (
            StatusCode::CONFLICT,
            Json(AdminErrorResponse {
                error: "order_delete_blocked".to_string(),
                blockers: Some(blockers),
                apparatus_options: None,
                order_width_mm: None,
                roll_width_mm: None,
            }),
        ),
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
        ProductionMapError::RawMaterialOrderNotActive => {
            conflict("raw_material_order_not_active")
        }
        ProductionMapError::QolipLocationNotFound => bad_request("qolip_location_not_found"),
        ProductionMapError::QolipCodeMismatch => bad_request("qolip_code_mismatch"),
        ProductionMapError::QolipAlreadyInUse => bad_request("qolip_already_in_use"),
        ProductionMapError::QolipInsufficientStock => bad_request("insufficient_stock"),
        ProductionMapError::QolipLocationIdentityMismatch => {
            bad_request("location_identity_mismatch")
        }
        ProductionMapError::RawMaterialScanRequired => bad_request("raw_material_scan_required"),
        ProductionMapError::RawMaterialMismatch => bad_request("raw_material_mismatch"),
        ProductionMapError::RawMaterialStateNotReady => {
            bad_request("raw_material_state_not_ready")
        }
        ProductionMapError::RawMaterialScanIncomplete => {
            bad_request("raw_material_scan_incomplete")
        }
        ProductionMapError::RawMaterialRequirementNotMet => {
            bad_request("raw_material_requirement_not_met")
        }
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
        ProductionMapError::LaminatsiyaAstatkaMetricsRequired => {
            bad_request("laminatsiya_astatka_metrics_required")
        }
        ProductionMapError::RezkaAstatkaMetricsRequired => {
            bad_request("rezka_astatka_metrics_required")
        }
        ProductionMapError::RezkaProgressMetricsRequired => {
            bad_request("rezka_progress_metrics_required")
        }
        ProductionMapError::RezkaKadrCountRequired => {
            bad_request("rezka_kadr_count_required")
        }
        ProductionMapError::RezkaFinalRollRequired => {
            bad_request("rezka_final_roll_required")
        }
        ProductionMapError::ProgressBatchNotFound => not_found("progress_batch_not_found"),
        ProductionMapError::ProgressBatchNotAccepted => bad_request("progress_batch_not_accepted"),
        ProductionMapError::ProgressBatchNotResumable => {
            bad_request("progress_batch_not_resumable")
        }
        ProductionMapError::CapacityProfileInvalid => bad_request("capacity_profile_invalid"),
        ProductionMapError::CapacityProfileNotFound => not_found("capacity_profile_not_found"),
        ProductionMapError::CapabilityNotSupported => bad_request("capability_not_supported"),
        ProductionMapError::CapabilityLevelInsufficient => {
            conflict("capability_level_insufficient")
        }
        ProductionMapError::CapacityConflict => conflict("capacity_conflict"),
        ProductionMapError::CapacityNoWorkingWindow => conflict("capacity_no_working_window"),
        ProductionMapError::CapacityUnavailable => conflict("capacity_unavailable"),
        ProductionMapError::ScheduleInputInvalid => bad_request("schedule_input_invalid"),
        ProductionMapError::ScheduleIdempotencyConflict => {
            conflict("schedule_idempotency_conflict")
        }
        ProductionMapError::ScheduleReservationNotFound => {
            not_found("schedule_reservation_not_found")
        }
        ProductionMapError::ScheduleReservationLocked => conflict("schedule_reservation_locked"),
    }
}

#[cfg(test)]
mod production_map_error_tests {
    use super::*;

    fn assert_code(error: ProductionMapError, expected: &str) {
        let (_, response) = production_map_error(error);
        assert_eq!(response.0.error, expected);
    }

    #[test]
    fn validation_errors_use_stable_codes_instead_of_display_text() {
        assert_code(ProductionMapError::MissingId, "map_id_required");
        assert_code(
            ProductionMapError::DuplicateNode("node-1".to_string()),
            "duplicate_node_id",
        );
        assert_code(
            ProductionMapError::InvalidFormulaExpression("x".to_string()),
            "invalid_formula_expression",
        );
        assert_code(ProductionMapError::Cycle, "production_map_cycle");
    }

    #[test]
    fn capacity_errors_keep_their_specific_http_and_api_identity() {
        let (status, response) = production_map_error(ProductionMapError::CapacityConflict);
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(response.0.error, "capacity_conflict");

        let (status, response) =
            production_map_error(ProductionMapError::ScheduleReservationNotFound);
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(response.0.error, "schedule_reservation_not_found");
    }
}

fn ambiguous_raw_material_apparatuses(apparatuses: Vec<String>) -> AdminError {
    (
        StatusCode::BAD_REQUEST,
        Json(AdminErrorResponse {
            error: "raw_material_group_ambiguous".to_string(),
            blockers: None,
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
        WarehouseError::NotFound => not_found("warehouse not found"),
        WarehouseError::AssignmentNotFound => not_found("warehouse assignment not found"),
        WarehouseError::NotEmpty(_)
        | WarehouseError::HasActiveReservations(_)
        | WarehouseError::HasChildren => bad_request("warehouse operation is not allowed"),
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
        PrincipalRole::Boyoqchi => "boyoqchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    }
}

pub(super) fn principal_owner_key(principal: &Principal) -> String {
    let role = principal_role_code(&principal.role);
    owner_key(role, &principal.ref_)
}
