use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::core::admin::ports::AdminPortError;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::authz::Capability;
use crate::core::gscale::{GscaleServiceError, MaterialReceiptPrintRequest};
use crate::core::werka::models::SupplierItem;
use crate::http::handlers::auth::{ErrorResponse, bearer_token};
use crate::http::handlers::material_catalog::{
    MaterialCatalogError, normalize_material_receipt_item, requires_material_dimensions,
    roll_material_item_group_roots,
};

pub async fn items(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<GscaleItemsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<GscaleErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    let can_read_gscale = state
        .admin
        .principal_has_capability(&principal, Capability::GscaleCatalogRead)
        .await;
    let can_manage_rezka = state
        .admin
        .principal_has_capability(&principal, Capability::RezkaSplitManage)
        .await;
    if !can_read_gscale && !can_manage_rezka {
        return Err(forbidden());
    }
    let items = gscale_items_for_principal(&state, &principal, &query)
        .await
        .map_err(admin_read_error)?;
    let dimension_groups = state
        .admin
        .item_group_scope(roll_material_item_group_roots())
        .await
        .map_err(admin_read_error)?;
    let items = items
        .into_iter()
        .map(|item| GscaleCatalogItem {
            requires_dimensions: requires_material_dimensions(&item, &dimension_groups),
            item,
        })
        .collect::<Vec<_>>();
    Ok(Json(
        serde_json::to_value(items).unwrap_or_else(|_| serde_json::json!([])),
    ))
}

#[derive(Serialize)]
struct GscaleCatalogItem {
    #[serde(flatten)]
    item: SupplierItem,
    requires_dimensions: bool,
}

async fn gscale_items_for_principal(
    state: &AppState,
    principal: &Principal,
    query: &GscaleItemsQuery,
) -> Result<Vec<SupplierItem>, AdminPortError> {
    let group = query.group.as_deref().unwrap_or("");
    let search = query.q.as_deref().unwrap_or("");
    let limit = positive_int(query.limit.as_deref(), 50).min(200);
    let offset = optional_offset(query.offset.as_deref());
    if principal.role == PrincipalRole::TayyorlovMasteri {
        return gscale_items_for_tayyorlov(&state, &principal, group, search, limit, offset)
            .await;
    }
    if principal.role != PrincipalRole::MaterialTaminotchi {
        return state
            .admin
            .items_page_by_group(group, search, limit, offset)
            .await;
    }
    let scoped_groups = state
        .admin
        .principal_assigned_item_group_scope(principal)
        .await?;
    if scoped_groups.is_empty() {
        return Ok(Vec::new());
    }
    let requested_group = group.trim();
    let groups = if requested_group.is_empty() {
        scoped_groups
    } else {
        let requested_scope = state
            .admin
            .item_group_scope(vec![requested_group.to_string()])
            .await?;
        scoped_groups
            .into_iter()
            .filter(|group| {
                requested_scope
                    .iter()
                    .any(|requested| requested.trim().eq_ignore_ascii_case(group.trim()))
            })
            .collect()
    };
    if groups.is_empty() {
        return Ok(Vec::new());
    }

    state
        .admin
        .items_page_in_groups(&groups, search, limit, offset)
        .await
}

/// Tayyorlov masteri katalogi: faqat o'ziga biriktirilgan homashyo
/// oilalariga mos itemlar (kod/nom exact yoki "nomi + bo'sh joy" prefiksi,
/// masalan PET -> "PET 615/12"). Biriktirilmagan bo'lsa bo'sh (fail-closed).
async fn gscale_items_for_tayyorlov(
    state: &AppState,
    principal: &Principal,
    group: &str,
    search: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<SupplierItem>, AdminPortError> {
    let materials = match state.preparation.as_ref() {
        Some(store) => store
            .assigned_material_names(&principal.ref_)
            .await
            .map_err(|_| AdminPortError::LookupFailed)?,
        None => Vec::new(),
    };
    if materials.is_empty() {
        return Ok(Vec::new());
    }
    let items = state
        .admin
        .items_page_by_group(group, search, 200, 0)
        .await?;
    let mut out: Vec<SupplierItem> = items
        .into_iter()
        .filter(|item| {
            let code = item.code.trim().to_lowercase();
            let name = item.name.trim().to_lowercase();
            materials.iter().any(|(id, material_name)| {
                code == *id
                    || name == *id
                    || code == *material_name
                    || name == *material_name
                    || code.starts_with(&format!("{material_name} "))
                    || name.starts_with(&format!("{material_name} "))
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.code.to_lowercase().cmp(&b.code.to_lowercase()))
    });
    Ok(out.into_iter().skip(offset).take(limit).collect())
}

pub async fn material_receipt_print(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<GscaleErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    if !state
        .admin
        .principal_has_capability(&principal, Capability::GscalePrint)
        .await
    {
        return Err(forbidden());
    }
    let mut request: MaterialReceiptPrintRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json", "invalid json"))?;
    require_material_warehouse_access(&state, &principal, &request.warehouse).await?;
    normalize_material_receipt_item(&state, &principal, &mut request)
        .await
        .map_err(material_catalog_error)?;
    request.actor_role = principal_role_code(&principal.role).to_string();
    request.actor_ref = principal.ref_.trim().to_string();
    request.actor_display_name = principal.display_name.trim().to_string();
    let response = state
        .gscale
        .print_material_receipt_driver_first(request)
        .await
        .map_err(gscale_error)?;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

async fn require_material_warehouse_access(
    state: &AppState,
    principal: &Principal,
    warehouse: &str,
) -> Result<(), (StatusCode, Json<GscaleErrorResponse>)> {
    if !matches!(
        principal.role,
        PrincipalRole::MaterialTaminotchi | PrincipalRole::TayyorlovMasteri
    ) {
        return Ok(());
    }
    let assigned = state
        .warehouses
        .assigned_warehouse_names(principal)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(GscaleErrorResponse::new(
                    "warehouse_scope_failed",
                    "warehouse scope failed",
                )),
            )
        })?;
    if assigned
        .iter()
        .any(|assigned| assigned.trim().eq_ignore_ascii_case(warehouse.trim()))
    {
        return Ok(());
    }
    let detail = if principal.role == PrincipalRole::TayyorlovMasteri {
        "warehouse is not assigned to tayyorlov masteri"
    } else {
        "warehouse is not assigned to material taminotchi"
    };
    Err((
        StatusCode::FORBIDDEN,
        Json(GscaleErrorResponse::new("warehouse_not_assigned", detail)),
    ))
}

async fn authenticated_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<GscaleErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

fn gscale_error(error: GscaleServiceError) -> (StatusCode, Json<GscaleErrorResponse>) {
    let status = match error {
        GscaleServiceError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        GscaleServiceError::NotConfigured(_) => StatusCode::SERVICE_UNAVAILABLE,
        GscaleServiceError::EpcGenerationFailed => StatusCode::INTERNAL_SERVER_ERROR,
        GscaleServiceError::StoreWrite(_)
        | GscaleServiceError::PrintFailed { .. }
        | GscaleServiceError::SubmitFailed(_) => StatusCode::FAILED_DEPENDENCY,
    };
    (
        status,
        Json(GscaleErrorResponse {
            ok: false,
            error: error.code(),
            detail: error.to_string(),
        }),
    )
}

fn material_catalog_error(error: MaterialCatalogError) -> (StatusCode, Json<GscaleErrorResponse>) {
    match error {
        MaterialCatalogError::ReadFailed => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(GscaleErrorResponse::new(
                "catalog_read_failed",
                "catalog read failed",
            )),
        ),
        MaterialCatalogError::ItemNotFound => {
            bad_request("catalog_item_not_found", "catalog item not found")
        }
        MaterialCatalogError::Forbidden => forbidden(),
        MaterialCatalogError::DimensionsRequired => bad_request(
            "material_dimensions_required",
            "positive width_mm and micron are required",
        ),
    }
}

fn unauthorized() -> (StatusCode, Json<GscaleErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(GscaleErrorResponse::new("unauthorized", "unauthorized")),
    )
}

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
}

#[allow(dead_code)]
fn _keeps_error_response_compatible(_response: ErrorResponse) {}
