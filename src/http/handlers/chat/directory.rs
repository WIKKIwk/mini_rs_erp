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
                role: role.clone(),
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
                role: role.clone(),
                ref_: detail.id,
                display_name: detail.name,
                avatar_url: detail.avatar_url,
            })
        }
        PrincipalRole::Qolipchi | PrincipalRole::Boyoqchi => {
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
            .filter(|item| {
                !item.blocked
                    && item.status != "removed"
                    && can_participate_in_chat(&item.principal_role)
            })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use crate::config::AppConfig;
    use crate::core::admin::models::AdminState;
    use crate::core::admin::ports::AdminStatePort;
    use crate::core::admin::service::AdminService;
    use crate::core::system_users::{MemorySystemUserStore, SystemUserService, SystemUserUpsert};
    use crate::core::workers::{MemoryWorkerStore, WorkerService, WorkerUpsert};

    #[derive(Default)]
    struct TestAdminStatePort {
        states: Mutex<BTreeMap<String, AdminState>>,
    }

    #[async_trait]
    impl AdminStatePort for TestAdminStatePort {
        async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
            Ok(self.states.lock().await.clone())
        }

        async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError> {
            self.states.lock().await.insert(ref_.to_string(), state);
            Ok(())
        }
    }

    fn test_state() -> AppState {
        let mut state = AppState::new(AppConfig {
            bind_addr: "127.0.0.1:8081".parse().expect("addr"),
            default_target_warehouse: String::new(),
            http_timeout: std::time::Duration::from_secs(15),
            session_store_path: "data/mobile_sessions.json".into(),
            profile_store_path: "data/mobile_profiles.json".into(),
            push_token_store_path: "data/mobile_push_tokens.json".into(),
            session_ttl_seconds: Some(3600),
            supplier_prefix: "10".to_string(),
            werka_prefix: "20".to_string(),
            werka_code: "20ABCDEF1234".to_string(),
            werka_name: "Werka".to_string(),
            werka_phone: "+99888862440".to_string(),
            material_taminotchi_code: String::new(),
            material_taminotchi_name: "Material taminotchisi".to_string(),
            material_taminotchi_phone: String::new(),
            admin_phone: "+998880000000".to_string(),
            admin_name: "Admin".to_string(),
            admin_code: "19621978".to_string(),
        });
        state.workers = WorkerService::new(Arc::new(MemoryWorkerStore::new()));
        state.system_users = SystemUserService::new(Arc::new(MemorySystemUserStore::new()));
        state.admin = AdminService::new(&state.config)
            .with_state_port(Arc::new(TestAdminStatePort::default()));
        state
    }

    #[tokio::test]
    async fn exact_target_resolution_finds_worker_beyond_directory_cap() {
        let state = test_state();
        for index in 0..=500 {
            state
                .workers
                .upsert_worker(WorkerUpsert {
                    id: format!("worker_{index:04}"),
                    name: format!("Worker {index:04}"),
                    phone: String::new(),
                    level: "Master".to_string(),
                })
                .await
                .expect("worker");
        }

        let capped = state.workers.workers("", 500).await.expect("workers");
        assert_eq!(capped.len(), 500);
        assert!(capped.iter().all(|worker| worker.id != "worker_0500"));
        assert_eq!(
            state
                .workers
                .workers("Worker 0500", 500)
                .await
                .expect("searched workers")
                .len(),
            1
        );

        let target = resolve_target(&state, &PrincipalRole::Aparatchi, "worker_0500")
            .await
            .expect("exact target");

        assert_eq!(target.role, PrincipalRole::Aparatchi);
        assert_eq!(target.ref_, "worker_0500");
        assert_eq!(target.display_name, "Worker 0500");
    }

    #[tokio::test]
    async fn exact_target_resolution_validates_system_user_role_beyond_cap() {
        let state = test_state();
        for index in 0..=500 {
            state
                .system_users
                .upsert_user(SystemUserUpsert {
                    id: format!("qolipchi_{index:04}"),
                    role: PrincipalRole::Qolipchi,
                    name: format!("Qolipchi {index:04}"),
                    phone: format!("+99890{index:07}"),
                })
                .await
                .expect("system user");
        }

        let capped = state
            .system_users
            .users(&PrincipalRole::Qolipchi, "", 500)
            .await
            .expect("system users");
        assert_eq!(capped.len(), 500);
        assert!(capped.iter().all(|user| user.id != "qolipchi_0500"));

        let target = resolve_target(&state, &PrincipalRole::Qolipchi, "qolipchi_0500")
            .await
            .expect("exact target");
        assert_eq!(target.role, PrincipalRole::Qolipchi);
        assert_eq!(target.ref_, "qolipchi_0500");

        assert!(
            resolve_target(&state, &PrincipalRole::Boyoqchi, "qolipchi_0500")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn exact_target_resolution_rejects_blocked_and_deactivated_users() {
        let mut state = test_state();
        state
            .system_users
            .upsert_user(SystemUserUpsert {
                id: "qolipchi_blocked".to_string(),
                role: PrincipalRole::Qolipchi,
                name: "Blocked qolipchi".to_string(),
                phone: "+998901234567".to_string(),
            })
            .await
            .expect("system user");
        let state_port = Arc::new(TestAdminStatePort::default());
        state_port
            .put_state(
                "qolipchi_blocked",
                AdminState {
                    blocked: true,
                    ..AdminState::default()
                },
            )
            .await
            .expect("blocked state");
        state.admin = AdminService::new(&state.config).with_state_port(state_port);

        assert!(
            resolve_target(&state, &PrincipalRole::Qolipchi, "qolipchi_blocked",)
                .await
                .is_err()
        );

        state
            .workers
            .upsert_worker(WorkerUpsert {
                id: "worker_deactivated".to_string(),
                name: "Deactivated worker".to_string(),
                phone: String::new(),
                level: "Master".to_string(),
            })
            .await
            .expect("worker");
        state
            .workers
            .deactivate_worker("worker_deactivated")
            .await
            .expect("deactivate worker");

        assert!(
            resolve_target(&state, &PrincipalRole::Aparatchi, "worker_deactivated",)
                .await
                .is_err()
        );
    }
}
