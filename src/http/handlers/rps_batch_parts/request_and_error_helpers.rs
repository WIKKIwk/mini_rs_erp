fn method_not_allowed() -> (StatusCode, Json<RpsBatchErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(RpsBatchErrorResponse::new(
            "method_not_allowed",
            "method not allowed",
        )),
    )
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
        PrincipalRole::ChiqindiQabulchi => "chiqindi_qabulchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    }
}

#[derive(Debug, Serialize)]
pub struct RpsBatchErrorResponse {
    pub ok: bool,
    pub error: &'static str,
    pub detail: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct RpsBatchHistoryQuery {
    pub limit: Option<usize>,
}

impl RpsBatchErrorResponse {
    fn new(error: &'static str, detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            error,
            detail: detail.into(),
        }
    }
}

impl RpsBatchServiceError {
    fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::BatchNotActive => "batch_not_active",
            Self::BatchAlreadyActive => "batch_already_active",
            Self::BatchContextConflict => "batch_context_conflict",
            Self::StoreFailed => "batch_store_failed",
        }
    }
}
