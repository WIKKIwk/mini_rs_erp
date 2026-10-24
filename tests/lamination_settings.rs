#![cfg(feature = "verification")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use mini_rs_erp::{
    app::AppState,
    config::AppConfig,
    core::{
        admin::service::AdminService,
        apparatus_standard::ExecutionOperation,
        auth::models::{Principal, PrincipalRole},
        authz::{RoleAssignmentUpsert, RoleDefinitionUpsert},
        production_map::*,
        session::manager::SessionManager,
    },
    http::router::build_router,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};
use tower::ServiceExt;

const SETTINGS: &str = "/v1/mobile/admin/production-maps/lamination-settings";
const ORDER: &str = "zakaz-lamination-settings";

async fn request(
    router: &Router,
    method: &str,
    token: &str,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(if method == "GET" {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn token(state: &AppState, role: PrincipalRole, ref_: &str) -> String {
    state
        .sessions
        .create(Principal {
            role,
            ref_: ref_.into(),
            display_name: "Operator".into(),
            legal_name: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .unwrap()
}

fn config(path: &Path) -> AppConfig {
    AppConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        default_target_warehouse: String::new(),
        http_timeout: Duration::from_secs(2),
        session_store_path: path.join("sessions.json"),
        profile_store_path: path.join("profiles.json"),
        push_token_store_path: path.join("push.json"),
        session_ttl_seconds: Some(3600),
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
async fn only_admin_can_disable_qr_returns_while_legacy_completion_and_astatka_keep_working() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = AppState::verification(config(directory.path()));
    state.sessions = SessionManager::memory(Some(3600));
    state.admin = AdminService::new(&state.config);
    state.apparatus.bootstrap_factory_defaults().await.unwrap();
    let resolver = Arc::new(CanonicalServiceApparatusResolver::new(
        state.apparatus.clone(),
    ));
    let apparatus = resolver
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.runtime.execution_profile.operation == ExecutionOperation::Laminate)
        .unwrap();
    let lam = apparatus.runtime.apparatus_id.as_str();
    let store = Arc::new(MemoryProductionMapStore::new());
    state.production_maps = ProductionMapService::new(store.clone(), resolver.clone());
    // An operator with an administrator capability still cannot change this setting.
    state
        .admin
        .upsert_role_definition(RoleDefinitionUpsert {
            id: "delegated-lamination-admin".into(),
            label: "Delegated".into(),
            base_role: Some(PrincipalRole::Aparatchi),
            capability_codes: vec!["admin.access".into()],
        })
        .await
        .unwrap();
    for (ref_, role_id) in [
        ("lam-worker", "aparatchi"),
        ("delegated-worker", "delegated-lamination-admin"),
    ] {
        state
            .admin
            .upsert_role_assignment(RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: ref_.into(),
                role_id: role_id.into(),
                assigned_apparatus: vec![lam.into()],
                assigned_item_groups: vec![],
            })
            .await
            .unwrap();
    }
    let admin = token(&state, PrincipalRole::Admin, "admin").await;
    let worker = token(&state, PrincipalRole::Aparatchi, "lam-worker").await;
    let delegated = token(&state, PrincipalRole::Aparatchi, "delegated-worker").await;
    let router = build_router(state.clone());
    let (status, body) = request(&router, "GET", &admin, SETTINGS, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["settings"]["layer_remainders_required"], true);
    for unauthorized in [&worker, &delegated, "invalid"] {
        let (status, _) = request(
            &router,
            "PUT",
            unauthorized,
            SETTINGS,
            json!({"layer_remainders_required":false}),
        )
        .await;
        assert!(matches!(
            status,
            StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED
        ));
        assert!(
            state
                .production_maps
                .lamination_settings()
                .await
                .unwrap()
                .layer_remainders_required
        );
    }
    let (status, body) = request(
        &router,
        "PUT",
        &admin,
        SETTINGS,
        json!({"layer_remainders_required":false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["settings"]["layer_remainders_required"], false);
    let (status, body) = request(&router, "GET", &worker, SETTINGS, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["settings"]["layer_remainders_required"], false);
    // Disabled mode works without loading or requiring any historical physical sources.
    let uri = format!(
        "/v1/mobile/admin/production-maps/laminatsiya-astatka?apparatus={lam}&order_id={ORDER}"
    );
    let (status, snapshot) = request(&router, "GET", &worker, &uri, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{snapshot}");
    assert_eq!(snapshot["layer_remainders_required"], false);
    assert_eq!(snapshot["sources"], json!([]));

    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "lam-worker".into(),
        display_name: "Laminator".into(),
    };
    let error = state
        .production_maps
        .record_layer_remainder(
            LayerRemainderInput {
                apparatus: lam.into(),
                order_id: ORDER.into(),
                session_id: "lam-session".into(),
                source_kind: "progress_batch".into(),
                source_id: "original".into(),
                expected_revision: 1,
                remaining_net_kg: 2.0,
                request_id: "disabled-request".into(),
            },
            actor.clone(),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, ProductionMapError::QueueActionNotAllowed));

    let map = serde_json::from_value(json!({
        "id":ORDER, "title":"Lamination", "product_code":"PET", "code":"PET",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"lam","kind":"apparatus","title":"Lamination","apparatus_id":lam},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"lam"},{"from":"lam","to":"end"}]
    }))
    .unwrap();
    state.production_maps.upsert_map(map).await.unwrap();
    store
        .put_apparatus_sequence(lam, vec![ORDER.into()])
        .await
        .unwrap();
    store
        .put_apparatus_queue_states(lam, BTreeMap::from([(ORDER.into(), "in_progress".into())]))
        .await
        .unwrap();
    store
        .put_order_run_session(OrderRunSession {
            session_id: "lam-session".into(),
            apparatus: lam.into(),
            order_id: ORDER.into(),
            stage_node_id: "lam".into(),
            status: OrderRunStatus::Active,
            worker_role: actor.role.clone(),
            worker_ref: actor.ref_.clone(),
            worker_display_name: actor.display_name.clone(),
            started_at_unix: 10,
            updated_at_unix: 20,
            payload_json: json!({}),
        })
        .await
        .unwrap();
    let (status, report) = request(
        &router,
        "POST",
        &worker,
        "/v1/mobile/admin/production-maps/laminatsiya-astatka",
        json!({
            "apparatus":lam,"order_id":ORDER,"lamination_print_leftover_rolls":2,
            "lamination_film_leftover_rolls":1,"total_waste":3,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(
        report["report"]["lamination_print_leftover_rolls"].as_f64(),
        Some(2.0)
    );
    assert_eq!(
        report["report"]["lamination_film_leftover_rolls"].as_f64(),
        Some(1.0)
    );
    assert_eq!(report["report"]["total_waste"].as_f64(), Some(3.0));
    let result = state
        .production_maps
        .apply_apparatus_queue_action_with_progress(
            lam,
            ORDER,
            queue_state::ApparatusQueueAction::Complete,
            &[lam.into()],
            actor,
            QueueProgressInput {
                finished_goods_meter: Some(100.0),
                finished_goods_kg: Some(11.0),
                bobina_kg: Some(1.0),
                lamination_print_leftover_rolls: Some(2.0),
                lamination_film_leftover_rolls: Some(1.0),
                total_waste: Some(3.0),
                force_full_completion_metrics: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        result.states.get(ORDER).map(String::as_str),
        Some("completed")
    );
    let outputs = store.progress_batches_for_order(ORDER).await.unwrap();
    assert!(!outputs.is_empty());
    assert!(
        outputs
            .iter()
            .all(|b| b.payload_json["origin"] != "layer_remainder")
    );
    let (status, body) = request(
        &router,
        "PUT",
        &admin,
        SETTINGS,
        json!({"layer_remainders_required":true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        state
            .production_maps
            .lamination_settings()
            .await
            .unwrap()
            .layer_remainders_required
    );
}
