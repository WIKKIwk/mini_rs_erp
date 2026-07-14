use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method};
use serde::Deserialize;

use super::auth::authorize;
use super::{ChatHttpError, http_error};
use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::chat::{ChatDirectoryEntry, ChatDirectoryPage, ChatPrincipalInput};
use crate::core::profile::identity::ProfileIdentity;
use crate::http::handlers::auth::profile_avatar_proxy_url;

#[derive(Default, Deserialize)]
pub struct DirectoryQuery {
    q: Option<String>,
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
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let offset = query.offset.unwrap_or(0);
    let mut items = load_directory_entries(
        &state,
        query.q.as_deref().unwrap_or_default(),
        Some(&viewer),
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
    let items = load_directory_entries(state, "", None).await?;
    let item = items
        .into_iter()
        .find(|item| item.role == *role && item.ref_.trim() == ref_.trim())
        .ok_or_else(|| http_error(axum::http::StatusCode::NOT_FOUND, "chat_user_not_found"))?;
    Ok(ChatPrincipalInput {
        role: item.role,
        ref_: item.ref_,
        display_name: item.display_name,
        avatar_url: item.avatar_url,
    })
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
) -> Result<Vec<ChatDirectoryEntry>, ChatHttpError> {
    let mut items = Vec::new();
    let admin_page = state
        .admin
        .user_list_page(query, 500, 0, None)
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
            .filter(|item| !item.blocked && item.status != "removed")
            .map(|item| ChatDirectoryEntry {
                role: item.principal_role,
                ref_: item.entity_ref,
                display_name: item.name,
                avatar_url: item.avatar_url,
            }),
    );

    if let Ok(workers) = state.workers.workers(query, 500).await {
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

    for role in [PrincipalRole::Qolipchi, PrincipalRole::Boyoqchi] {
        if let Ok(users) = state.system_users.users(&role, query, 500).await {
            for user in users {
                let detail = match state.admin.system_user_detail(user).await {
                    Ok(detail) if !detail.blocked => detail,
                    _ => continue,
                };
                items.push(ChatDirectoryEntry {
                    role: role.clone(),
                    ref_: detail.id,
                    display_name: detail.name,
                    avatar_url: detail.avatar_url,
                });
            }
        }
    }

    let needle = query.trim().to_lowercase();
    let admin_ref = "admin";
    if needle.is_empty()
        || state.config.admin_name.to_lowercase().contains(&needle)
        || admin_ref.contains(&needle)
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
