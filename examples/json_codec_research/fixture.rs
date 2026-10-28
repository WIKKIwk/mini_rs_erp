//! Isolated memory-domain fixture exercising the actual production Router.
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    http::{Request, StatusCode, header},
};
use mini_rs_erp::{
    app::AppState,
    config::AppConfig,
    core::{
        admin::service::AdminService,
        auth::models::{Principal, PrincipalRole},
        authz::RoleAssignmentUpsert,
        production_map::*,
        session::manager::SessionManager,
    },
    http::router::build_router,
};
use serde_json::{Value, json};
use tower::ServiceExt;

pub const CUT: &str = "apparatus:default:asset-010";
pub const QUEUE: &str = "/v1/mobile/admin/production-maps/sequence";
pub const PAGE: &str = "/v1/mobile/admin/production-maps/order-page";
pub const LIVE: &str = "/v1/mobile/admin/production-maps/live";

pub struct Fixture {
    _directory: tempfile::TempDir,
    pub state: AppState,
    pub router: Router,
    pub worker: String,
    pub admin: String,
    store: Arc<MemoryProductionMapStore>,
    changes: AtomicUsize,
}

impl Fixture {
    pub async fn new(count: usize) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut state = AppState::verification(config(directory.path()));
        state.apparatus.bootstrap_factory_defaults().await.unwrap();
        state.sessions = SessionManager::memory(Some(3600));
        state.admin = AdminService::new(&state.config);
        let store = Arc::new(MemoryProductionMapStore::new());
        state.production_maps = ProductionMapService::new(
            store.clone(),
            Arc::new(CanonicalServiceApparatusResolver::new(
                state.apparatus.clone(),
            )),
        );
        state
            .admin
            .upsert_role_assignment(RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: "json-research-worker".into(),
                role_id: "aparatchi".into(),
                assigned_apparatus: vec![CUT.into()],
                assigned_item_groups: vec![],
            })
            .await
            .unwrap();
        let mut ids = Vec::new();
        for index in 0..count {
            let definition = map(index, 0);
            ids.push(definition.id.clone());
            state.production_maps.upsert_map(definition).await.unwrap();
        }
        store.put_apparatus_sequence(CUT, ids).await.unwrap();
        state.production_maps.notify_live();
        let worker = session(&state, PrincipalRole::Aparatchi, "json-research-worker").await;
        let admin = session(&state, PrincipalRole::Admin, "json-research-admin").await;
        let router = build_router(state.clone());
        Self {
            _directory: directory,
            state,
            router,
            worker,
            admin,
            store,
            changes: AtomicUsize::new(0),
        }
    }

    pub async fn get(&self, uri: &str, worker: bool, gzip: bool) -> (Bytes, String) {
        let request = Request::builder()
            .uri(uri)
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", if worker { &self.worker } else { &self.admin }),
            )
            .header(
                header::ACCEPT_ENCODING,
                if gzip { "gzip" } else { "identity" },
            )
            .body(Body::empty())
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        let encoding = response
            .headers()
            .get(header::CONTENT_ENCODING)
            .map(|value| value.to_str().unwrap().to_owned())
            .unwrap_or_else(|| "identity".into());
        (
            to_bytes(response.into_body(), 64 * 1024 * 1024)
                .await
                .unwrap(),
            encoding,
        )
    }

    pub async fn json(&self, uri: &str, worker: bool) -> Value {
        serde_json::from_slice(&self.get(uri, worker, false).await.0).unwrap()
    }

    pub async fn put(&self, body: &[u8]) -> (StatusCode, Bytes) {
        let response = self
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v1/mobile/admin/production-maps")
                    .header(header::AUTHORIZATION, format!("Bearer {}", self.admin))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        (
            status,
            to_bytes(response.into_body(), 64 * 1024 * 1024)
                .await
                .unwrap(),
        )
    }

    pub async fn change(&self) -> u64 {
        let step = self.changes.fetch_add(1, Ordering::Relaxed) + 1;
        let mut definition = self
            .state
            .production_maps
            .raw_map("zakaz-research-00000")
            .await
            .unwrap()
            .unwrap();
        definition.title =
            format!("Packaging label 00000 — O‘zbekcha / Русский / العربية, revision {step}");
        // Synthetic committed label changes, as in the existing stream tests.
        // Do not relax opened-order calculation locks to generate a benchmark.
        self.store.put_map(definition).await.unwrap();
        self.state.production_maps.notify_live();
        self.state.production_maps.snapshot_revision()
    }
}

pub fn map(index: usize, revision: usize) -> ProductionMapDefinition {
    let id = format!("zakaz-research-{index:05}");
    serde_json::from_value(json!({
        "id":id,"product_code":format!("PRODUCT-{index:05}"),"code":id,
        "order_number":format!("RESEARCH-{index:05}"),
        "title":format!("Packaging label {index:05} — O‘zbekcha / Русский / العربية, revision {revision}"),
        "customer_name":format!("Synthetic customer {:03} — packaging and manufacturing", index % 37),
        "roll_count":12,"width_mm":137.125,"order_kg":725.5,"base_length":2500.0,
        "nodes":[
            {"id":"start","kind":"start","title":"Start","x":0,"y":0},
            {"id":"print","kind":"apparatus","title":"Bosma","apparatus_id":"apparatus:default:bosma_9","x":200,"y":0},
            {"id":"laminate","kind":"apparatus","title":"Laminatsiya","apparatus_id":"apparatus:default:asset-008","x":400,"y":0},
            {"id":"cut","kind":"apparatus","title":"Rezka","apparatus_id":CUT,"rezka_kadr_count":3,"x":600,"y":0},
            {"id":"end","kind":"end","title":"End","x":800,"y":0}
        ],
        "edges":[{"from":"start","to":"print"},{"from":"print","to":"laminate"},
            {"from":"laminate","to":"cut"},{"from":"cut","to":"end"}]
    })).unwrap()
}

async fn session(state: &AppState, role: PrincipalRole, name: &str) -> String {
    state
        .sessions
        .create(Principal {
            role,
            ref_: name.into(),
            display_name: name.into(),
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
