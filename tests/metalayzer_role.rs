//! System account provisioning, authentication and privilege isolation without a database.
use async_trait::async_trait;
use mini_rs_erp::{
    config::AppConfig,
    core::{
        admin::{
            models::AdminState,
            ports::{AdminPortError, AdminStatePort},
            service::AdminService,
        },
        auth::{
            models::{Principal, PrincipalRole},
            ports::{AdminAccessState, AdminAccessStateLookup, AuthPortError},
            service::{AuthError, AuthService},
        },
        authz::{
            Capability, RoleAssignmentUpsert, RoleDefinitionUpsert, capability_codes_for_role,
            system_role_definitions,
        },
        profile::identity::ProfileIdentity,
        system_users::{MemorySystemUserStore, SystemUserService, SystemUserUpsert},
    },
};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::RwLock;

#[derive(Default)]
struct AccessState(RwLock<BTreeMap<String, AdminState>>);
#[async_trait]
impl AdminStatePort for AccessState {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        Ok(self.0.read().await.clone())
    }
    async fn put_state(&self, id: &str, state: AdminState) -> Result<(), AdminPortError> {
        self.0.write().await.insert(id.into(), state);
        Ok(())
    }
}
#[async_trait]
impl AdminAccessStateLookup for AccessState {
    async fn list_states(&self) -> Result<BTreeMap<String, AdminAccessState>, AuthPortError> {
        Ok(self
            .0
            .read()
            .await
            .iter()
            .map(|(id, state)| {
                (
                    id.clone(),
                    AdminAccessState {
                        // Authentication reads password hashes; administrative state exposes the issued code.
                        custom_code: mini_rs_erp::core::auth::password::hash_password_sync(
                            &state.custom_code,
                        )
                        .unwrap(),
                        blocked: state.blocked,
                        removed: state.removed,
                    },
                )
            })
            .collect())
    }
}
fn config() -> AppConfig {
    AppConfig {
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
    }
}

#[tokio::test]
async fn metalayzer_accounts_use_the_existing_provisioning_and_login_with_only_their_capability() {
    let config = config();
    let states = Arc::new(AccessState::default());
    let users = Arc::new(SystemUserService::new(Arc::new(
        MemorySystemUserStore::new(),
    )));
    let admin = AdminService::new(&config).with_state_port(states.clone());
    let user = users
        .upsert_user(SystemUserUpsert {
            id: String::new(),
            role: PrincipalRole::Metalayzer,
            name: "Metalayzer worker".into(),
            phone: "+998901112295".into(),
        })
        .await
        .unwrap();
    assert!(user.id.starts_with("metalayzer_"));
    let detail = admin
        .regenerate_system_user_code(user.clone())
        .await
        .unwrap();
    assert!(detail.code.starts_with("95"));
    let auth = AuthService::new(&config).with_system_user_dependencies(users, states.clone());
    let principal = auth.login(&user.phone, &detail.code).await.unwrap();
    assert_eq!(principal.role, PrincipalRole::Metalayzer);
    assert_eq!(principal.ref_, user.id);
    assert_eq!(
        admin.principal_capability_codes(&principal).await,
        vec!["metalayzer.access"]
    );
    assert_eq!(
        auth.login(&user.phone, "951234ABCDEF").await.unwrap_err(),
        AuthError::InvalidCredentials
    );
    let mut blocked = states.0.read().await[&user.id].clone();
    blocked.blocked = true;
    states.put_state(&user.id, blocked).await.unwrap();
    assert_eq!(
        auth.login(&user.phone, &detail.code).await.unwrap_err(),
        AuthError::InvalidCredentials
    );
}

#[tokio::test]
async fn custom_roles_cannot_give_metalayzer_stock_or_admin_powers() {
    let admin = AdminService::new(&config()).with_state_port(Arc::new(AccessState::default()));
    admin
        .upsert_role_definition(RoleDefinitionUpsert {
            id: "metalayzer-extra".into(),
            label: "Restricted custom role".into(),
            base_role: Some(PrincipalRole::Metalayzer),
            capability_codes: vec![
                "metalayzer.access".into(),
                "admin.access".into(),
                "raw_material.assign".into(),
                "raw_material.split".into(),
            ],
        })
        .await
        .unwrap();
    admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::Metalayzer,
            principal_ref: "metalayzer-actor".into(),
            role_id: "metalayzer-extra".into(),
            assigned_apparatus: vec![],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let principal = Principal {
        role: PrincipalRole::Metalayzer,
        ref_: "metalayzer-actor".into(),
        display_name: "Metalayzer".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    assert_eq!(
        admin.principal_capability_codes(&principal).await,
        vec!["metalayzer.access"]
    );
    for capability in [
        Capability::AdminAccess,
        Capability::RawMaterialAssign,
        Capability::RawMaterialSplitAccess,
    ] {
        assert!(!admin.principal_has_capability(&principal, capability).await);
    }
    assert!(
        admin
            .principal_has_capability(&principal, Capability::MetalayzerAccess)
            .await
    );
}

#[test]
fn metalayzer_has_its_own_catalog_and_profile_identity() {
    assert_eq!(
        capability_codes_for_role(PrincipalRole::Metalayzer),
        vec!["metalayzer.access"]
    );
    let role = system_role_definitions()
        .into_iter()
        .find(|role| role.id == "metalayzer")
        .unwrap();
    assert!(role.system);
    assert_eq!(role.base_role, Some(PrincipalRole::Metalayzer));
    assert_eq!(
        ProfileIdentity::new("metalayzer", "worker")
            .unwrap()
            .vault_key(),
        "metalayzer:worker"
    );
}
