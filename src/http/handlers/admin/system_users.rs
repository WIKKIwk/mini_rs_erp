use super::*;

use crate::core::admin::models::{AdminSystemUserDetail, AdminUserListEntry, AdminUserListPage};
use crate::core::system_users::{SystemUser, SystemUserError, SystemUserUpsert};

#[derive(Debug, Deserialize)]
pub struct SystemUserIdQuery {
    id: Option<String>,
}

pub async fn system_users(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    match method {
        Method::GET => {
            let role = required_system_role(query.role.as_deref())?;
            state
                .system_users
                .users(
                    &role,
                    query.q.as_deref().unwrap_or_default(),
                    optional_search_limit(query.limit.as_deref(), 50, 500),
                )
                .await
                .map(json_response)
                .map_err(system_user_error)
        }
        Method::POST | Method::PUT => {
            let input: SystemUserUpsert = parse_json(&body)?;
            state
                .system_users
                .upsert_user(input)
                .await
                .map(json_response)
                .map_err(system_user_error)
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn system_user_detail(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<SystemUserIdQuery>,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let user = required_system_user(&state, query.id.as_deref()).await?;
    let mut detail = state
        .admin
        .system_user_detail(user)
        .await
        .map_err(|_| server_error("system user detail failed"))?;
    detail.avatar_url = with_admin_profile_avatar_proxy(
        &headers,
        detail.avatar_url,
        "qolipchi",
        &detail.id,
    );
    Ok(json_response(detail))
}

pub async fn system_user_code_regenerate(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<SystemUserIdQuery>,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let user = required_system_user(&state, query.id.as_deref()).await?;
    state
        .admin
        .regenerate_system_user_code(user)
        .await
        .map(json_response)
        .map_err(|_| server_error("system user code regenerate failed"))
}

pub(super) async fn system_user_list_page(
    state: &AppState,
    query: &PageQuery,
) -> Result<AdminUserListPage, AdminError> {
    let limit = optional_search_limit(query.limit.as_deref(), 20, 50);
    let offset = optional_offset(query.offset.as_deref());
    let users = state
        .system_users
        .users(
            &PrincipalRole::Qolipchi,
            query.q.as_deref().unwrap_or_default(),
            offset.saturating_add(limit).saturating_add(1),
        )
        .await
        .map_err(system_user_error)?;
    let has_more = users.len() > offset.saturating_add(limit);
    let mut items = Vec::new();
    for user in users.into_iter().skip(offset).take(limit) {
        let detail = state
            .admin
            .system_user_detail(user)
            .await
            .map_err(|_| server_error("system user detail failed"))?;
        items.push(system_user_list_entry(detail));
    }
    Ok(AdminUserListPage { items, has_more })
}

fn system_user_list_entry(detail: AdminSystemUserDetail) -> AdminUserListEntry {
    AdminUserListEntry {
        id: format!("system_user:{}", detail.id),
        source: "system_user".to_string(),
        entity_ref: detail.id,
        principal_role: detail.role,
        name: detail.name,
        phone: detail.phone,
        role_label: "Qolipchi".to_string(),
        blocked: detail.blocked,
        status: if detail.blocked { "blocked" } else { "active" }.to_string(),
    }
}

fn required_system_role(value: Option<&str>) -> Result<PrincipalRole, AdminError> {
    match value.unwrap_or("qolipchi").trim().to_ascii_lowercase().as_str() {
        "qolipchi" => Ok(PrincipalRole::Qolipchi),
        _ => Err(bad_request("system user role is invalid")),
    }
}

async fn required_system_user(
    state: &AppState,
    id: Option<&str>,
) -> Result<SystemUser, AdminError> {
    let id = id.unwrap_or_default().trim();
    if id.is_empty() {
        return Err(bad_request("system user id is required"));
    }
    state
        .system_users
        .users_by_ids(&[id.to_string()])
        .await
        .map_err(system_user_error)?
        .into_iter()
        .next()
        .ok_or_else(|| not_found("system user not found"))
}

fn system_user_error(error: SystemUserError) -> AdminError {
    match error {
        SystemUserError::MissingName => bad_request("system user name is required"),
        SystemUserError::MissingPhone => bad_request("system user phone is required"),
        SystemUserError::InvalidRole => bad_request("system user role is invalid"),
        SystemUserError::DuplicatePhone => bad_request("system user phone already exists"),
        SystemUserError::NotFound => not_found("system user not found"),
        SystemUserError::StoreFailed => server_error("system user store failed"),
    }
}
