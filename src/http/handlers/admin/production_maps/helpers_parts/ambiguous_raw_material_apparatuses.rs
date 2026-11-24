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
        PrincipalRole::TayyorlovMasteri => "tayyorlov_masteri",
        PrincipalRole::HomashyoRezkachi => "homashyo_rezkachi",
        PrincipalRole::SalesManager => "sales_manager",
        PrincipalRole::Brigader => "brigader",
        PrincipalRole::RezkaMaster => "rezka_master",
        PrincipalRole::Metalayzer => "metalayzer",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    }
}

pub(super) fn principal_owner_key(principal: &Principal) -> String {
    let role = principal_role_code(&principal.role);
    owner_key(role, &principal.ref_)
}
