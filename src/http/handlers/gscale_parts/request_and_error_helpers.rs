fn forbidden() -> (StatusCode, Json<GscaleErrorResponse>) {
    (
        StatusCode::FORBIDDEN,
        Json(GscaleErrorResponse::new("forbidden", "forbidden")),
    )
}

fn bad_request(
    error: &'static str,
    detail: &'static str,
) -> (StatusCode, Json<GscaleErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(GscaleErrorResponse::new(error, detail)),
    )
}

fn method_not_allowed() -> (StatusCode, Json<GscaleErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(GscaleErrorResponse::new(
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
pub struct GscaleErrorResponse {
    pub ok: bool,
    pub error: &'static str,
    pub detail: String,
}

impl GscaleErrorResponse {
    fn new(error: &'static str, detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            error,
            detail: detail.into(),
        }
    }
}

fn admin_read_error(error: AdminPortError) -> (StatusCode, Json<GscaleErrorResponse>) {
    let status = match error {
        AdminPortError::NotFound => StatusCode::NOT_FOUND,
        #[cfg(test)]
        AdminPortError::PermissionDenied => StatusCode::FORBIDDEN,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(GscaleErrorResponse::new(
            "catalog_read_failed",
            "catalog read failed",
        )),
    )
}

fn positive_int(value: Option<&str>, default: usize) -> usize {
    match value.unwrap_or("").trim().parse::<usize>() {
        Ok(value) if value > 0 => value,
        _ => default,
    }
}

fn optional_offset(value: Option<&str>) -> usize {
    value
        .unwrap_or("")
        .trim()
        .parse::<isize>()
        .ok()
        .filter(|value| *value >= 0)
        .unwrap_or(0) as usize
}

#[derive(Debug, Deserialize)]
pub struct GscaleItemsQuery {
    pub q: Option<String>,
    pub group: Option<String>,
    pub limit: Option<String>,
    pub offset: Option<String>,
    pub order_id: Option<String>,
}

#[allow(dead_code)]
fn _keeps_error_response_compatible(_response: ErrorResponse) {}
