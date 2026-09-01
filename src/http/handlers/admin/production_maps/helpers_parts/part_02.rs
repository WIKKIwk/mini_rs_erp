
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
        assert_code(
            ProductionMapError::QueueSequenceOrderNotFound("missing".to_string()),
            "queue_sequence_order_not_found",
        );
        assert_code(
            ProductionMapError::QueueSequenceApparatusMismatch("zakaz-1".to_string()),
            "queue_sequence_apparatus_mismatch",
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
        Json(AdminErrorResponse::with_apparatus_options(
            "raw_material_group_ambiguous",
            apparatuses,
        )),
    )
}

pub(super) fn warehouse_error(error: WarehouseError) -> AdminError {
    match error {
        WarehouseError::MissingWarehouse => bad_request("warehouse is required"),
        WarehouseError::InvalidApparatus => bad_request("apparatus is invalid"),
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
