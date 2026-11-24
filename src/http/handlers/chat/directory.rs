use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method};
use serde::Deserialize;

use super::auth::authorize;
use super::{ChatHttpError, http_error};
use crate::app::AppState;
use crate::core::admin::ports::AdminPortError;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::chat::{
    ChatDirectoryEntry, ChatDirectoryPage, ChatPrincipalInput, can_participate_in_chat,
};
use crate::core::profile::identity::ProfileIdentity;
use crate::http::handlers::auth::profile_avatar_proxy_url;

#[derive(Default, Deserialize)]
pub struct DirectoryQuery {
    q: Option<String>,
    role: Option<PrincipalRole>,
    limit: Option<usize>,
    offset: Option<usize>,
}

pub async fn directory(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<ChatDirectoryPage>, ChatHttpError> {
    if method != Method::GET {
        return Err(http_error(
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ));
    }
    let (token, viewer) = authorize(&state, &headers).await?;
    if !can_participate_in_chat(&viewer.role) {
        return Err(http_error(
            axum::http::StatusCode::FORBIDDEN,
            "chat_forbidden",
        ));
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let offset = query.offset.unwrap_or(0);
    let mut items = load_directory_entries(
        &state,
        query.q.as_deref().unwrap_or_default(),
        Some(&viewer),
        query.role,
    )
    .await?;
    for item in &mut items {
        item.avatar_url =
            proxied_avatar_url(&headers, &item.avatar_url, &item.role, &item.ref_, &token);
    }
    let has_more = items.len() > offset.saturating_add(limit);
    let items = items.into_iter().skip(offset).take(limit).collect();
    Ok(Json(ChatDirectoryPage { items, has_more }))
}

pub(super) async fn resolve_target(
    state: &AppState,
    role: &PrincipalRole,
    ref_: &str,
) -> Result<ChatPrincipalInput, ChatHttpError> {
    let ref_ = ref_.trim();
    if ref_.is_empty() || !can_participate_in_chat(role) {
        return Err(chat_target_not_found());
    }

    match role {
        PrincipalRole::Admin => {
            if ref_ != "admin" {
                return Err(chat_target_not_found());
            }
            let avatar_url = state
                .admin
                .profile_avatar_url_for_principal(role, ref_)
                .await;
            Ok(ChatPrincipalInput {
                role: *role,
                ref_: ref_.to_string(),
                display_name: state.config.admin_name.clone(),
                avatar_url,
            })
        }
        PrincipalRole::Aparatchi => {
            let worker = state
                .workers
                .workers_by_ids(&[ref_.to_string()])
                .await
                .map_err(|_| chat_directory_failed())?
                .into_iter()
                .find(|worker| worker.id.trim() == ref_)
                .ok_or_else(chat_target_not_found)?;
            let detail = state
                .admin
                .worker_detail(worker)
                .await
                .map_err(map_exact_admin_error)?;
            Ok(ChatPrincipalInput {
                role: *role,
                ref_: detail.id,
                display_name: detail.name,
                avatar_url: detail.avatar_url,
            })
        }
        PrincipalRole::Qolipchi
        | PrincipalRole::Boyoqchi
        | PrincipalRole::TayyorlovMasteri
        | PrincipalRole::HomashyoRezkachi
        | PrincipalRole::SalesManager
        | PrincipalRole::Brigader
        | PrincipalRole::RezkaMaster
        | PrincipalRole::Metalayzer => {
            let user = state
                .system_users
                .users_by_ids(&[ref_.to_string()])
                .await
                .map_err(|_| chat_directory_failed())?
                .into_iter()
                .find(|user| user.id.trim() == ref_ && user.role == *role)
                .ok_or_else(chat_target_not_found)?;
            let detail = state
                .admin
                .system_user_detail(user)
                .await
                .map_err(map_exact_admin_error)?;
            if detail.blocked {
                return Err(chat_target_not_found());
            }
            Ok(ChatPrincipalInput {
                role: detail.role,
                ref_: detail.id,
                display_name: detail.name,
                avatar_url: detail.avatar_url,
            })
        }
        PrincipalRole::Supplier
        | PrincipalRole::Werka
        | PrincipalRole::Customer
        | PrincipalRole::MaterialTaminotchi => {
            let item = state
                .admin
                .user_list_entry_for_principal(role, ref_)
                .await
                .map_err(|_| chat_directory_failed())?
                .filter(|item| {
                    !item.blocked
                        && item.status != "removed"
                        && item.principal_role == *role
                        && item.entity_ref.trim() == ref_
                })
                .ok_or_else(chat_target_not_found)?;
            Ok(ChatPrincipalInput {
                role: item.principal_role,
                ref_: item.entity_ref,
                display_name: item.name,
                avatar_url: item.avatar_url,
            })
        }
    }
}

fn map_exact_admin_error(error: AdminPortError) -> ChatHttpError {
    if matches!(error, AdminPortError::NotFound) {
        chat_target_not_found()
    } else {
        chat_directory_failed()
    }
}

fn chat_target_not_found() -> ChatHttpError {
    http_error(axum::http::StatusCode::NOT_FOUND, "chat_user_not_found")
}

fn chat_directory_failed() -> ChatHttpError {
    http_error(
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "chat_directory_failed",
    )
}

pub(super) fn proxied_avatar_url(
    headers: &HeaderMap,
    avatar_url: &str,
    role: &PrincipalRole,
    ref_: &str,
    token: &str,
) -> String {
    if !avatar_url.trim().starts_with("local://") {
        return avatar_url.trim().to_string();
    }
    let Some(identity) = ProfileIdentity::from_principal(role, ref_) else {
        return avatar_url.trim().to_string();
    };
    profile_avatar_proxy_url(headers, avatar_url, identity.role_key(), ref_, token)
        .unwrap_or_else(|| avatar_url.trim().to_string())
}

async fn load_directory_entries(
    state: &AppState,
    query: &str,
    exclude: Option<&Principal>,
    role: Option<PrincipalRole>,
) -> Result<Vec<ChatDirectoryEntry>, ChatHttpError> {
    let mut items = Vec::new();
    let catalog_role = match role {
        Some(PrincipalRole::Supplier) => Some("supplier"),
        Some(PrincipalRole::Werka) => Some("werka"),
        Some(PrincipalRole::Customer) => Some("customer"),
        Some(PrincipalRole::MaterialTaminotchi) => Some("material_taminotchi"),
        _ => None,
    };
    if role.is_none() || catalog_role.is_some() {
        let admin_page = state
            .admin
            .user_list_page(query, 500, 0, catalog_role)
            .await
            .map_err(|_| {
                http_error(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "chat_directory_failed",
                )
            })?;
        items.extend(
            admin_page
                .items
                .into_iter()
                .filter(|item| {
                    !item.blocked
                        && item.status != "removed"
                        && role.is_none_or(|role| item.principal_role == role)
                        && can_participate_in_chat(&item.principal_role)
                })
                .map(|item| ChatDirectoryEntry {
                    role: item.principal_role,
                    ref_: item.entity_ref,
                    display_name: item.name,
                    avatar_url: item.avatar_url,
                }),
        );
    }

    if role.is_none_or(|role| role == PrincipalRole::Aparatchi)
        && let Ok(workers) = state.workers.workers(query, 500).await
    {
        for worker in workers {
            let (display_name, avatar_url, ref_) = match state.admin.worker_detail(worker).await {
                Ok(detail) => (detail.name, detail.avatar_url, detail.id),
                Err(_) => continue,
            };
            items.push(ChatDirectoryEntry {
                role: PrincipalRole::Aparatchi,
                ref_,
                display_name,
                avatar_url,
            });
        }
    }

    for system_role in [
        PrincipalRole::Qolipchi,
        PrincipalRole::Boyoqchi,
        PrincipalRole::TayyorlovMasteri,
        PrincipalRole::HomashyoRezkachi,
        PrincipalRole::Metalayzer,
        PrincipalRole::SalesManager,
        PrincipalRole::Brigader,
    ] {
        if role.is_some_and(|role| role != system_role) {
            continue;
        }
        if let Ok(users) = state.system_users.users(&system_role, query, 500).await {
            for user in users {
                let detail = match state.admin.system_user_detail(user).await {
                    Ok(detail) if !detail.blocked => detail,
                    _ => continue,
                };
                items.push(ChatDirectoryEntry {
                    role: system_role,
                    ref_: detail.id,
                    display_name: detail.name,
                    avatar_url: detail.avatar_url,
                });
            }
        }
    }

    let needle = query.trim().to_lowercase();
    let admin_ref = "admin";
    if role.is_none_or(|role| role == PrincipalRole::Admin)
        && (needle.is_empty()
            || state.config.admin_name.to_lowercase().contains(&needle)
            || admin_ref.contains(&needle))
    {
        let admin_avatar = state
            .admin
            .profile_avatar_url_for_principal(&PrincipalRole::Admin, admin_ref)
            .await;
        items.push(ChatDirectoryEntry {
            role: PrincipalRole::Admin,
            ref_: admin_ref.to_string(),
            display_name: state.config.admin_name.clone(),
            avatar_url: admin_avatar,
        });
    }

    if let Some(exclude) = exclude {
        items.retain(|item| item.role != exclude.role || item.ref_.trim() != exclude.ref_.trim());
    }
    items.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
            .then_with(|| left.ref_.cmp(&right.ref_))
    });
    items.dedup_by(|left, right| left.role == right.role && left.ref_ == right.ref_);
    Ok(items)
}

#[cfg(test)]
#[path = "directory_tests.rs"]
mod tests;
