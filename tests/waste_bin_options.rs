#![cfg(feature = "verification")]

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use mini_rs_erp::{
    app::AppState,
    config::AppConfig,
    core::{
        admin::{
            models::AdminState,
            ports::{AdminPortError, AdminStatePort},
            service::AdminService,
        },
        auth::models::{Principal, PrincipalRole},
        authz::RoleAssignmentUpsert,
        session::manager::SessionManager,
        system_users::{MemorySystemUserStore, SystemUserService, SystemUserUpsert},
        workers::{MemoryWorkerStore, WorkerService, WorkerUpsert},
    },
    http::router::build_router,
};
use serde_json::{Value, json};
use tower::ServiceExt;

const CUT: &str = "apparatus:default:asset-010";
const OTHER_CUT: &str = "apparatus:default:asset-011";
const OPTIONS: &str = "/v1/mobile/waste/bins/options";
const REGISTER: &str = "/v1/mobile/waste/bins/register";
const CONFIGURE: &str = "/v1/mobile/waste/bins/configure";

struct States;

#[async_trait]
impl AdminStatePort for States {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        Ok(BTreeMap::from([(
            "blocked".into(),
            AdminState {
                blocked: true,
                ..AdminState::default()
            },
        )]))
    }

    async fn put_state(&self, _id: &str, _state: AdminState) -> Result<(), AdminPortError> {
        Ok(())
    }
}

fn config(directory: &std::path::Path) -> AppConfig {
    AppConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        default_target_warehouse: String::new(),
        http_timeout: Duration::from_secs(5),
        session_store_path: directory.join("sessions.json"),
        profile_store_path: directory.join("profiles.json"),
        push_token_store_path: directory.join("push.json"),
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

async fn token(state: &AppState, role: PrincipalRole, id: &str) -> String {
    state
        .sessions
        .create(Principal {
            role,
            ref_: id.into(),
            display_name: id.into(),
            legal_name: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .unwrap()
}

async fn call(
    router: &Router,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(if body.is_some() { "POST" } else { "GET" })
        .uri(path)
        .header("authorization", format!("Bearer {token}"));
    let request = match body {
        Some(body) => request
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn receiver_options_and_configuration_use_current_operator_assignments() {
    let temporary = tempfile::tempdir().unwrap();
    let config = config(temporary.path());
    let mut state = AppState::verification(config.clone());
    state.admin = AdminService::new(&config).with_state_port(Arc::new(States));
    state.sessions = SessionManager::memory(None);
    state.workers = WorkerService::new(Arc::new(MemoryWorkerStore::new()));
    state.system_users = SystemUserService::new(Arc::new(MemorySystemUserStore::new()));
    state.apparatus.bootstrap_factory_defaults().await.unwrap();
    for (id, phone) in [("operator-a", "998901112201"), ("blocked", "998901112202")] {
        state
            .workers
            .upsert_worker(WorkerUpsert {
                id: id.into(),
                phone: phone.into(),
                name: id.into(),
                level: "Master".into(),
            })
            .await
            .unwrap();
    }
    let master = state
        .system_users
        .upsert_user(SystemUserUpsert {
            id: String::new(),
            role: PrincipalRole::RezkaMaster,
            name: "Rezka master".into(),
            phone: "998901112203".into(),
        })
        .await
        .unwrap();
    for id in ["operator-a", "blocked", "missing"] {
        state
            .admin
            .upsert_role_assignment(RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: id.into(),
                role_id: "aparatchi".into(),
                assigned_apparatus: vec![CUT.into()],
                assigned_item_groups: Vec::new(),
            })
            .await
            .unwrap();
    }
    let receiver = token(&state, PrincipalRole::ChiqindiQabulchi, "receiver").await;
    let worker = token(&state, PrincipalRole::Aparatchi, "operator-a").await;
    let admin = token(&state, PrincipalRole::Admin, "admin").await;
    let router = build_router(state);
    for token in [&worker, &admin] {
        assert_eq!(
            call(&router, OPTIONS, token, None).await.0,
            StatusCode::FORBIDDEN
        );
    }
    let (status, options) = call(&router, OPTIONS, &receiver, None).await;
    assert_eq!(status, StatusCode::OK, "{options}");
    let operators = options["operators"].as_array().unwrap();
    assert_eq!(operators.len(), 2, "{operators:?}");
    let worker_option = operators
        .iter()
        .find(|row| row["ref"] == "operator-a")
        .unwrap();
    assert_eq!(worker_option["apparatus_ids"], json!([CUT]));
    assert!(worker_option.get("phone").is_none());
    let master_option = operators
        .iter()
        .find(|row| row["ref"] == master.id)
        .unwrap();
    assert_eq!(master_option["role"], "rezka_master");
    assert!(
        master_option["apparatus_ids"]
            .as_array()
            .unwrap()
            .contains(&json!(OTHER_CUT))
    );

    let mut input = json!({
        "request_id": "waste-test-register", "name": "Qirqim", "tare_kg": 5,
        "apparatus_ids": [CUT, OTHER_CUT],
        "operators": [{"role": "aparatchi", "ref": "operator-a"}]
    });
    let (status, error) = call(&router, REGISTER, &receiver, Some(input.clone())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["code"], "waste_access_invalid");
    input["operators"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role": "rezka_master", "ref": master.id}));
    // In verification mode the store is absent after the valid access checks.
    assert_eq!(
        call(&router, REGISTER, &receiver, Some(input.clone()))
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    input.as_object_mut().unwrap().remove("tare_kg");
    input.as_object_mut().unwrap().remove("request_id");
    input["code"] = json!("CQ-001");
    assert_eq!(
        call(&router, CONFIGURE, &worker, Some(input.clone()))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    input["operators"] = json!([{"role": "aparatchi", "ref": "blocked"}]);
    assert_eq!(
        call(&router, CONFIGURE, &receiver, Some(input)).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &router,
            &format!("/v1/mobile/waste/bins?status=empty&apparatus_id={OTHER_CUT}"),
            &worker,
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
