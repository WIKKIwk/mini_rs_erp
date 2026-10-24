use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;

use super::*;
use crate::core::session::manager::SessionManager;

#[derive(Default)]
struct MemoryState {
    rows: RwLock<BTreeMap<String, AdminState>>,
    unavailable: AtomicBool,
}

#[async_trait]
impl AdminStatePort for MemoryState {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(AdminPortError::LookupFailed);
        }
        Ok(self.rows.read().await.clone())
    }

    async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError> {
        self.rows.write().await.insert(ref_.to_string(), state);
        Ok(())
    }
}

fn fixture() -> (AdminService, SessionManager, Arc<MemoryState>) {
    let config = AppConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        default_target_warehouse: String::new(),
        http_timeout: std::time::Duration::from_secs(5),
        session_store_path: Default::default(),
        profile_store_path: Default::default(),
        push_token_store_path: Default::default(),
        session_ttl_seconds: Some(60),
        supplier_prefix: "10".into(),
        werka_prefix: "20".into(),
        werka_code: String::new(),
        werka_name: String::new(),
        werka_phone: String::new(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: String::new(),
        material_taminotchi_phone: String::new(),
        admin_phone: String::new(),
        admin_name: String::new(),
        admin_code: String::new(),
    };
    let states = Arc::new(MemoryState::default());
    let admin = AdminService::new(&config).with_state_port(states.clone());
    let sessions = SessionManager::memory(Some(60)).with_access_validator(Arc::new(admin.clone()));
    let admin = admin.with_session_revoker(Arc::new(sessions.clone()));
    (admin, sessions, states)
}

fn principal(role: PrincipalRole, ref_: &str) -> Principal {
    Principal {
        role,
        ref_: ref_.into(),
        display_name: "Synthetic account".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    }
}

#[tokio::test]
async fn blocked_account_rejects_cached_token_and_capabilities_without_deadlock() {
    let (admin, sessions, states) = fixture();
    let user = principal(PrincipalRole::Supplier, "supplier-a");
    let token = sessions.create(user.clone()).await.unwrap();
    assert!(sessions.get(&token).await.is_ok());
    states
        .put_state(
            &user.ref_,
            AdminState {
                blocked: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let result = tokio::time::timeout(std::time::Duration::from_secs(1), sessions.get(&token))
        .await
        .expect("cached revocation must not retain the cache read lock");
    assert!(matches!(result, Err(crate::error::AppError::Unauthorized)));
    assert!(
        !admin
            .principal_has_capability(&user, Capability::SupplierAccess)
            .await
    );
    assert!(admin.principal_capability_codes(&user).await.is_empty());
    assert!(sessions.update(&token, user.clone()).await.is_err());
    assert!(sessions.create(user).await.is_err());
}

#[tokio::test]
async fn blocking_revokes_every_role_and_unblocking_does_not_restore_tokens() {
    let (admin, sessions, _) = fixture();
    let supplier = sessions
        .create(principal(PrincipalRole::Supplier, "account-a"))
        .await
        .unwrap();
    let material = sessions
        .create(principal(PrincipalRole::MaterialTaminotchi, "account-a"))
        .await
        .unwrap();
    let other = sessions
        .create(principal(PrincipalRole::Supplier, "account-b"))
        .await
        .unwrap();

    admin
        .persist_access_state(
            "account-a",
            AdminState {
                blocked: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    admin
        .persist_access_state("account-a", AdminState::default())
        .await
        .unwrap();

    assert!(sessions.get(&supplier).await.is_err());
    assert!(sessions.get(&material).await.is_err());
    assert!(sessions.get(&other).await.is_ok());
    assert!(
        sessions
            .create(principal(PrincipalRole::Supplier, "account-a"))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn changing_access_code_revokes_old_tokens() {
    let (admin, sessions, states) = fixture();
    states
        .put_state(
            "account-a",
            AdminState {
                custom_code: "old-credential".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let token = sessions
        .create(principal(PrincipalRole::Supplier, "account-a"))
        .await
        .unwrap();
    admin
        .persist_access_state(
            "account-a",
            AdminState {
                custom_code: "replacement-credential".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(sessions.get(&token).await.is_err());
}

#[tokio::test]
async fn unchanged_access_state_keeps_active_sessions() {
    let (admin, sessions, _) = fixture();
    let token = sessions
        .create(principal(PrincipalRole::Supplier, "account-a"))
        .await
        .unwrap();
    admin
        .persist_access_state("account-a", AdminState::default())
        .await
        .unwrap();
    assert!(sessions.get(&token).await.is_ok());
}

#[tokio::test]
async fn unavailable_access_store_fails_closed_for_cached_sessions() {
    let (admin, sessions, states) = fixture();
    let user = principal(PrincipalRole::Supplier, "account-a");
    let token = sessions.create(user.clone()).await.unwrap();
    states.unavailable.store(true, Ordering::SeqCst);
    assert!(sessions.get(&token).await.is_err());
    assert!(
        !admin
            .principal_has_capability(&user, Capability::SupplierAccess)
            .await
    );
}

#[tokio::test]
async fn removed_account_rejects_persisted_session_after_restart() {
    let (admin, _, states) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sessions.json");
    let sessions = SessionManager::persistent(path.clone(), Some(60));
    let token = sessions
        .create(principal(PrincipalRole::Customer, "customer-a"))
        .await
        .unwrap();
    states
        .put_state(
            "customer-a",
            AdminState {
                removed: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let restarted =
        SessionManager::persistent(path, Some(60)).with_access_validator(Arc::new(admin));
    assert!(restarted.get(&token).await.is_err());
}
