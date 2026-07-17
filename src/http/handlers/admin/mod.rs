mod customers;
mod production_maps;
mod supplier_mutations;
mod suppliers;
mod system;
mod system_users;
mod warehouse_live;
mod workers;

pub use customers::{
    activity, customer_code_regenerate, customer_detail, customer_item_add, customer_item_remove,
    customer_list, customer_phone, customer_remove, customers, item_group_tree, item_groups, items,
    material_taminotchi_code_regenerate, material_taminotchi_detail, material_taminotchi_phone,
    material_taminotchilar,
};
pub use production_maps::{
    production_map_audit, production_map_closed_orders, production_map_completed_orders,
    production_map_completion_request_decision, production_map_completion_request_decisions,
    production_map_completion_requests, production_map_finished_goods_receive, production_map_live,
    production_map_move, production_map_move_batch, production_map_progress_qr_history,
    production_map_progress_qr_lookup, production_map_progress_qr_report,
    production_map_progress_qr_reprint, production_map_qolip_validate,
    production_map_queue_action, production_map_queue_policies,
    production_map_run, production_map_save_with_order, production_map_sequence,
    production_map_wip_batches, production_maps, raw_material_assignment_lookup,
    raw_material_assignments, raw_material_history, raw_material_rules, raw_material_stock,
};
pub use supplier_mutations::{
    supplier_code_regenerate, supplier_item_add, supplier_item_remove, supplier_items,
    supplier_phone, supplier_remove, supplier_restore, supplier_status,
};
pub use suppliers::{
    admin_profile_avatar_view, assigned_supplier_items, inactive_suppliers, settings,
    supplier_detail, supplier_list, supplier_summary, suppliers, user_list,
};
pub use system::{
    apparatus_create, apparatus_groups, capabilities, items_bulk_move_group, role_assignments,
    roles, system_backup_create, system_backup_download, system_monitor, system_monitor_live,
    warehouse_assignments, warehouse_summaries, warehouses, werka_code_regenerate,
};
pub use system_users::{system_user_code_regenerate, system_user_detail, system_users};
use system::{authorize_any_capability, authorize_capability, require_capability};
pub use warehouse_live::warehouse_live;
pub use workers::{
    worker_code_regenerate, worker_delete_check, worker_detail, worker_groups,
    worker_profile_detail, workers,
};

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::core::admin::models::{
    AdminBulkMoveItemsRequest, AdminCreateCustomerRequest, AdminCreateItemGroupRequest,
    AdminCreateItemRequest, AdminCreateMaterialTaminotchiRequest, AdminCreateSupplierRequest,
    AdminCustomerDetail, AdminItemGroupBulkMoveResult, AdminMoveItemGroupRequest,
    AdminPhoneUpdateRequest, AdminSettings, AdminSupplier, AdminSupplierDetail,
    AdminSupplierItemMutationRequest, AdminSupplierItemsUpdateRequest,
    AdminSupplierStatusUpdateRequest, AdminSupplierSummary, AdminSuppliersPage, AdminUserListEntry,
    AdminUserListPage,
};
use crate::core::admin::ports::AdminPortError;
use crate::core::apparatus_groups::{ApparatusGroupError, ApparatusGroupUpsert, ApparatusUpsert};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::authz::{
    Capability, RoleAssignmentUpsert, RoleDefinitionUpsert, capability_catalog_entries,
};
use crate::core::warehouses::{
    WarehouseAssignmentUpsert, WarehouseDeleteRequest, WarehouseError, WarehouseUpsert,
};
use crate::core::werka::models::{CustomerDirectoryEntry, DispatchRecord, SupplierItem};
use crate::http::handlers::auth::{bearer_token, profile_avatar_proxy_url};

type AdminError = (StatusCode, Json<AdminErrorResponse>);

fn required_ref(value: Option<&str>) -> Result<&str, AdminError> {
    let ref_ = value.unwrap_or("").trim();
    if ref_.is_empty() {
        Err(bad_request("ref is required"))
    } else {
        Ok(ref_)
    }
}

fn required_ref_item<'a>(
    ref_: Option<&'a str>,
    item_code: Option<&'a str>,
) -> Result<(&'a str, &'a str), AdminError> {
    let ref_ = ref_.unwrap_or("").trim();
    let item_code = item_code.unwrap_or("").trim();
    if ref_.is_empty() || item_code.is_empty() {
        Err(bad_request("ref and item_code are required"))
    } else {
        Ok((ref_, item_code))
    }
}

fn parse_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, AdminError> {
    serde_json::from_slice(body).map_err(|_| bad_request("invalid json"))
}

fn json_response<T: Serialize>(value: T) -> Response {
    Json(value).into_response()
}

fn with_admin_profile_avatar_proxy(
    headers: &HeaderMap,
    avatar_url: String,
    role_key: &str,
    ref_: &str,
) -> String {
    if !avatar_url.trim().starts_with("local://") {
        return avatar_url;
    }
    let Some(token) = bearer_token(headers) else {
        return avatar_url;
    };
    profile_avatar_proxy_url(headers, &avatar_url, role_key, ref_, &token).unwrap_or(avatar_url)
}

fn profile_role_key(role: &PrincipalRole) -> &'static str {
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

fn proxy_user_list_avatars(headers: &HeaderMap, page: &mut AdminUserListPage) {
    for entry in &mut page.items {
        entry.avatar_url = with_admin_profile_avatar_proxy(
            headers,
            std::mem::take(&mut entry.avatar_url),
            profile_role_key(&entry.principal_role),
            &entry.entity_ref,
        );
    }
}

fn optional_search_limit(value: Option<&str>, default: usize, max: usize) -> usize {
    match value.unwrap_or("").trim().parse::<usize>() {
        Ok(limit) if limit > 0 && max > 0 && limit > max => max,
        Ok(limit) if limit > 0 => limit,
        _ => default,
    }
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

#[cfg(test)]
mod tests {
    use super::optional_search_limit;

    #[test]
    fn optional_search_limit_matches_go_defaults_and_clamp() {
        assert_eq!(optional_search_limit(None, 20, 50), 20);
        assert_eq!(optional_search_limit(Some(""), 20, 50), 20);
        assert_eq!(optional_search_limit(Some("abc"), 20, 50), 20);
        assert_eq!(optional_search_limit(Some("0"), 20, 50), 20);
        assert_eq!(optional_search_limit(Some("5"), 20, 50), 5);
        assert_eq!(optional_search_limit(Some("500"), 20, 50), 50);
    }
}

fn unauthorized() -> AdminError {
    (
        StatusCode::UNAUTHORIZED,
        Json(AdminErrorResponse::new("unauthorized")),
    )
}

fn forbidden() -> AdminError {
    (
        StatusCode::FORBIDDEN,
        Json(AdminErrorResponse::new("forbidden")),
    )
}

fn method_not_allowed() -> AdminError {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(AdminErrorResponse::new("method not allowed")),
    )
}

fn bad_request(error: impl Into<String>) -> AdminError {
    (
        StatusCode::BAD_REQUEST,
        Json(AdminErrorResponse::new(error)),
    )
}

fn server_error(error: impl Into<String>) -> AdminError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(AdminErrorResponse::new(error)),
    )
}

fn not_found(error: impl Into<String>) -> AdminError {
    (StatusCode::NOT_FOUND, Json(AdminErrorResponse::new(error)))
}

fn too_many_requests(error: impl Into<String>) -> AdminError {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(AdminErrorResponse::new(error)),
    )
}

#[derive(Serialize)]
pub struct AdminErrorResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apparatus_options: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_width_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roll_width_mm: Option<f64>,
}

impl AdminErrorResponse {
    fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            apparatus_options: None,
            order_width_mm: None,
            roll_width_mm: None,
        }
    }

    fn roll_size_mismatch(order_width_mm: f64, roll_width_mm: f64) -> Self {
        Self {
            error: "raw_material_roll_size_mismatch".to_string(),
            apparatus_options: None,
            order_width_mm: Some(order_width_mm),
            roll_width_mm: Some(roll_width_mm),
        }
    }
}

#[derive(Serialize)]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub q: Option<String>,
    pub limit: Option<String>,
    pub offset: Option<String>,
    pub role: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RefQuery {
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RefItemQuery {
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    pub item_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AdminProfileAvatarQuery {
    pub role: Option<String>,
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ItemQuery {
    pub q: Option<String>,
    pub warehouse: Option<String>,
    pub parent: Option<String>,
    pub group: Option<String>,
    pub limit: Option<String>,
    pub offset: Option<String>,
}
