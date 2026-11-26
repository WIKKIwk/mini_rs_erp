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
    async fn states_for_refs(
        &self,
        refs: &[&str],
    ) -> Result<BTreeMap<String, AdminAccessState>, AuthPortError> {
        let states = self.0.read().await;
        Ok(refs.iter()
            .filter_map(|&id| states.get_key_value(id))
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

#[tokio::test]
async fn waste_receiver_is_a_native_system_account_with_only_waste_access() {
    let config = config();
    let states = Arc::new(AccessState::default());
    let users = Arc::new(SystemUserService::new(Arc::new(
        MemorySystemUserStore::new(),
    )));
    let admin = AdminService::new(&config).with_state_port(states.clone());
    let user = users
        .upsert_user(SystemUserUpsert {
            id: String::new(),
            role: PrincipalRole::ChiqindiQabulchi,
            name: "Qabulchi".into(),
            phone: "+998901112296".into(),
        })
        .await
        .unwrap();
    assert!(user.id.starts_with("chiqindi_qabulchi_"));
    let detail = admin
        .regenerate_system_user_code(user.clone())
        .await
        .unwrap();
    assert!(detail.code.starts_with("96"));
    let auth = AuthService::new(&config).with_system_user_dependencies(users, states);
    let principal = auth.login(&user.phone, &detail.code).await.unwrap();
    assert_eq!(principal.ref_, user.id);
    assert_eq!(principal.role, PrincipalRole::ChiqindiQabulchi);
    let system_role = system_role_definitions()
        .into_iter()
        .find(|role| role.id == "chiqindi_qabulchi")
        .unwrap();
    assert!(system_role.system);
    assert_eq!(system_role.label, "Chiqindi qabul qiluvchi");
    assert_eq!(
        admin.principal_capability_codes(&principal).await,
        ["waste.receive"]
    );

    admin
        .upsert_role_definition(RoleDefinitionUpsert {
            id: "waste-receiver-extra".into(),
            label: "Restricted custom role".into(),
            base_role: Some(PrincipalRole::ChiqindiQabulchi),
            capability_codes: vec!["admin.access".into(), "waste.receive".into()],
        })
        .await
        .unwrap();
    admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: principal.role,
            principal_ref: principal.ref_.clone(),
            role_id: "waste-receiver-extra".into(),
            assigned_apparatus: vec![],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    assert!(
        admin
            .principal_has_capability(&principal, Capability::WasteReceive)
            .await
    );
    assert!(
        !admin
            .principal_has_capability(&principal, Capability::AdminAccess)
            .await
    );
    assert_eq!(
        admin.principal_capability_codes(&principal).await,
        ["waste.receive"]
    );
}

#[cfg(feature = "verification")]
#[tokio::test]
async fn waste_routes_require_native_receiver_and_scope_operators_to_assigned_apparatus() {
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use mini_rs_erp::{
        app::AppState, core::session::manager::SessionManager, http::router::build_router,
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    async fn call(
        router: &Router,
        method: &str,
        path: &str,
        token: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json");
        if !token.is_empty() {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 32 * 1024).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    fn who(role: PrincipalRole, id: &str) -> Principal {
        Principal {
            role,
            ref_: id.into(),
            display_name: id.into(),
            legal_name: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        }
    }

    const APPARATUS: &str = "apparatus:default:asset-010";
    const OTHER_APPARATUS: &str = "apparatus:default:asset-011";
    const RECEIVE: &str = "/v1/mobile/waste/bins/receive";
    const RESET: &str = "/v1/mobile/waste/bins/reset";
    let receive = json!({"code":"BIN-1","cycle_id":"cycle-1","gross_kg":20,
        "measurement_source":"manual"});
    let reset = json!({"code":"BIN-1","cycle_id":"cycle-1"});
    let temp = tempfile::tempdir().unwrap();
    let mut cfg = config();
    cfg.session_store_path = temp.path().join("sessions.json");
    cfg.profile_store_path = temp.path().join("profiles.json");
    cfg.push_token_store_path = temp.path().join("push.json");
    let accounts = Arc::new(AccessState::default());
    let users = Arc::new(SystemUserService::new(Arc::new(
        MemorySystemUserStore::new(),
    )));
    let mut state = AppState::verification(cfg.clone());
    state.admin = AdminService::new(&cfg).with_state_port(accounts.clone());
    state.auth = AuthService::new(&cfg).with_system_user_dependencies(users.clone(), accounts);
    state.sessions = SessionManager::memory(None);
    state.apparatus.bootstrap_factory_defaults().await.unwrap();

    let admin = state
        .sessions
        .create(who(PrincipalRole::Admin, "admin"))
        .await
        .unwrap();
    let worker = state
        .sessions
        .create(who(PrincipalRole::Aparatchi, "worker"))
        .await
        .unwrap();
    let receiver = state
        .sessions
        .create(who(PrincipalRole::ChiqindiQabulchi, "receiver"))
        .await
        .unwrap();
    let router = build_router(state.clone());
    for (path, body) in [(RECEIVE, receive), (RESET, reset.clone())] {
        for token in [&admin, &worker] {
            assert_eq!(
                call(&router, "POST", path, token, body.clone()).await.0,
                StatusCode::FORBIDDEN
            );
        }
        assert_eq!(
            call(&router, "POST", path, "", body.clone()).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(&router, "POST", path, &receiver, body).await.0,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
    let assigned = format!("/v1/mobile/waste/bins/active?apparatus_id={APPARATUS}");
    let other = format!("/v1/mobile/waste/bins/active?apparatus_id={OTHER_APPARATUS}");
    for (index, (role, role_key)) in [
        (PrincipalRole::TayyorlovMasteri, "tayyorlov_masteri"),
        (PrincipalRole::HomashyoRezkachi, "homashyo_rezkachi"),
        (PrincipalRole::Metalayzer, "metalayzer"),
    ]
    .into_iter()
    .enumerate()
    {
        let user = users
            .upsert_user(SystemUserUpsert {
                id: String::new(),
                role,
                name: role_key.into(),
                phone: format!("+99890111230{index}"),
            })
            .await
            .unwrap();
        let code = state
            .admin
            .regenerate_system_user_code(user.clone())
            .await
            .unwrap()
            .code;
        let (status, assignment) = call(&router, "PUT", "/v1/mobile/admin/role-assignments", &admin,
            json!({"principal_role":role,"principal_ref":user.id,"role_id":role_key,"assigned_apparatus":[APPARATUS]}),
        ).await;
        assert_eq!(status, StatusCode::OK, "{assignment}");
        let (status, profile) = call(
            &router,
            "POST",
            "/v1/mobile/auth/login",
            "",
            json!({"phone":user.phone,"code":code}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{profile}");
        assert_eq!(profile["assigned_apparatus"], json!([APPARATUS]));
        let token = profile["token"].as_str().unwrap();
        assert_eq!(
            call(&router, "GET", &assigned, token, Value::Null).await.0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            call(&router, "GET", &other, token, Value::Null).await.0,
            StatusCode::FORBIDDEN
        );
    }
    state.sessions = SessionManager::memory(Some(0));
    let expired = state
        .sessions
        .create(who(PrincipalRole::ChiqindiQabulchi, "expired"))
        .await
        .unwrap();
    assert_eq!(
        call(&build_router(state), "POST", RESET, &expired, reset)
            .await
            .0,
        StatusCode::UNAUTHORIZED,
    );
}
