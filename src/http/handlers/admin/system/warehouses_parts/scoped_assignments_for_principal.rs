fn scoped_assignments_for_principal(
    assignments: Vec<WarehouseAssignment>,
    principal: &Principal,
) -> Vec<WarehouseAssignment> {
    assignments
        .into_iter()
        .filter(|assignment| {
            assignment.principal_role == principal.role
                && assignment
                    .principal_ref
                    .trim()
                    .eq_ignore_ascii_case(principal.ref_.trim())
        })
        .collect()
}

fn warehouse_error(error: WarehouseError) -> AdminError {
    match error {
        WarehouseError::MissingWarehouse => bad_request("warehouse is required"),
        WarehouseError::MissingPrincipalRef => bad_request("principal ref is required"),
        WarehouseError::InvalidApparatus => bad_request("apparatus is invalid"),
        WarehouseError::NotFound => not_found("warehouse_not_found"),
        WarehouseError::AssignmentNotFound => not_found("warehouse_assignment_not_found"),
        WarehouseError::NotEmpty(_) => (
            StatusCode::CONFLICT,
            Json(AdminErrorResponse::new("warehouse_not_empty")),
        ),
        WarehouseError::HasActiveReservations(_) => (
            StatusCode::CONFLICT,
            Json(AdminErrorResponse::new("warehouse_has_active_reservations")),
        ),
        WarehouseError::HasChildren => (
            StatusCode::CONFLICT,
            Json(AdminErrorResponse::new("warehouse_has_children")),
        ),
        WarehouseError::TypeMismatch => bad_request("warehouse_type_mismatch"),
        WarehouseError::HasWasteReceipts => (
            StatusCode::CONFLICT,
            Json(AdminErrorResponse::new("warehouse_has_waste_receipts")),
        ),
        WarehouseError::StoreFailed => server_error("warehouse store failed"),
    }
}
