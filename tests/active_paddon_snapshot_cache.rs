#![cfg(feature = "verification")]
//! Selection is actor-scoped state, independent of the shared production cache.
//! Runs the real service/router over memory stores, without the legacy lib suite.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use mini_rs_erp::{
    app::AppState,
    config::AppConfig,
    core::{
        admin::service::AdminService,
        apparatus_standard::{ApparatusId, RuntimeApparatusConfiguration},
        auth::models::{Principal, PrincipalRole},
        authz::RoleAssignmentUpsert,
        production_map::*,
        session::manager::SessionManager,
    },
    http::router::build_router,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;

const CUT: &str = "apparatus:default:asset-010";
const OTHER_CUT: &str = "apparatus:default:asset-011";
const NON_CUT: &str = "apparatus:default:asset-007";
const ACTIVE: &str = "/v1/mobile/admin/production-maps/paddons/active";

struct CountedResolver {
    inner: CanonicalServiceApparatusResolver,
    lists: AtomicUsize,
}

#[async_trait::async_trait]
impl CanonicalApparatusResolver for CountedResolver {
    async fn resolve(
        &self,
        id: &ApparatusId,
    ) -> Result<Option<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        self.inner.resolve(id).await
    }

    async fn list(&self) -> Result<Vec<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        // The canonical snapshot builder lists once per rebuild. Selection
        // validation resolves one ID and must not cause another list/build.
        self.lists.fetch_add(1, Ordering::Relaxed);
        self.inner.list().await
    }
}

struct Fixture {
    state: AppState,
    router: Router,
    resolver: Arc<CountedResolver>,
    worker: String,
    other_worker: String,
    admin: String,
    denied: String,
    codes: [String; 2],
}

struct SharedApp {
    _directory: tempfile::TempDir,
    state: AppState,
}

static SHARED_APP: tokio::sync::OnceCell<SharedApp> = tokio::sync::OnceCell::const_new();

impl Fixture {
    async fn new() -> Self {
        // AppState also owns unrelated process-local SQLite/LMDB stores. Open
        // those once; every tested service/session/permission store is isolated.
        let shared = SHARED_APP
            .get_or_init(|| async {
                let directory = tempfile::tempdir().unwrap();
                let state = AppState::verification(config(directory.path()));
                state.apparatus.bootstrap_factory_defaults().await.unwrap();
                SharedApp {
                    _directory: directory,
                    state,
                }
            })
            .await;
        let mut state = shared.state.clone();
        state.sessions = SessionManager::memory(Some(3600));
        state.admin = AdminService::new(&state.config);
        let resolver = Arc::new(CountedResolver {
            inner: CanonicalServiceApparatusResolver::new(state.apparatus.clone()),
            lists: AtomicUsize::new(0),
        });
        state.production_maps =
            ProductionMapService::new(Arc::new(MemoryProductionMapStore::new()), resolver.clone());
        state
            .production_maps
            .upsert_map(map("Original order"))
            .await
            .unwrap();
        for ref_ in ["operator-a", "operator-b"] {
            state
                .admin
                .upsert_role_assignment(RoleAssignmentUpsert {
                    principal_role: PrincipalRole::Aparatchi,
                    principal_ref: ref_.into(),
                    role_id: "aparatchi".into(),
                    assigned_apparatus: vec![CUT.into(), OTHER_CUT.into()],
                    assigned_item_groups: vec![],
                })
                .await
                .unwrap();
        }
        let worker = session(&state, PrincipalRole::Aparatchi, "operator-a").await;
        let other_worker = session(&state, PrincipalRole::Aparatchi, "operator-b").await;
        // Same reference, different role: it must be a different selection.
        let admin = session(&state, PrincipalRole::Admin, "operator-a").await;
        let denied = session(&state, PrincipalRole::Customer, "customer").await;
        let actor = actor();
        let first = state
            .production_maps
            .create_paddon("Cutting", "First", &actor)
            .await
            .unwrap();
        let second = state
            .production_maps
            .create_paddon("Cutting", "Second", &actor)
            .await
            .unwrap();
        let router = build_router(state.clone());
        Self {
            state,
            router,
            resolver,
            worker,
            other_worker,
            admin,
            denied,
            codes: [first.code, second.code],
        }
    }

    async fn request(
        &self,
        method: &str,
        token: &str,
        apparatus: &str,
        code: &str,
    ) -> (StatusCode, Value) {
        let uri = if method == "GET" {
            format!("{ACTIVE}?apparatus={}", urlencoding::encode(apparatus))
        } else {
            ACTIVE.into()
        };
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(if method == "GET" {
                Body::empty()
            } else {
                Body::from(json!({"apparatus":apparatus, "code":code}).to_string())
            })
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    async fn selection(
        &self,
        method: &str,
        token: &str,
        apparatus: &str,
        code: &str,
        expected: Option<&str>,
    ) {
        let (status, body) = self.request(method, token, apparatus, code).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body,
            json!({"ok":true, "apparatus":apparatus.trim(), "code":expected})
        );
    }
}

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "operator-a".into(),
        display_name: "Operator".into(),
    }
}

fn map(title: &str) -> ProductionMapDefinition {
    serde_json::from_value(json!({
        "id":"active-paddon-order", "product_code":"P-1", "code":"P-1", "title":title,
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"cut","kind":"apparatus","title":"Rezka","apparatus_id":CUT,
                "rezka_kadr_count":1},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"cut"},{"from":"cut","to":"end"}]
    }))
    .unwrap()
}

async fn session(state: &AppState, role: PrincipalRole, ref_: &str) -> String {
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
async fn selection_set_change_repeat_clear_leave_shared_snapshot_untouched() {
    let f = Fixture::new().await;
    let service = &f.state.production_maps;
    let (before, revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    let epoch = service.snapshot_epoch().to_string();
    let content = serde_json::to_value(before.as_ref()).unwrap();
    assert!(!before.maps.is_empty());
    assert!(
        before
            .queue_action_controls
            .get(CUT)
            .is_some_and(|v| !v.is_empty()),
        "snapshot: {content}"
    );
    let initial_builds = f.resolver.lists.load(Ordering::Relaxed);
    assert_eq!(initial_builds, 1);
    let mut events = service.subscribe_live();
    f.selection("GET", &f.worker, CUT, "", None).await;
    let mut observations = Vec::new();
    for code in [
        format!(" {} ", f.codes[0]),
        f.codes[1].clone(),
        f.codes[1].clone(),
        " ".into(),
        String::new(),
    ] {
        let expected = (!code.trim().is_empty()).then_some(code.trim());
        f.selection("PUT", &f.worker, CUT, &code, expected).await;
        f.selection("GET", &f.worker, CUT, "", expected).await;
        assert_eq!(
            service
                .active_rezka_paddon(CUT, &actor())
                .await
                .unwrap()
                .as_deref(),
            expected
        );
        let (after, after_revision) = service.live_snapshot_shared_with_revision().await.unwrap();
        assert_eq!(serde_json::to_value(after.as_ref()).unwrap(), content);
        observations.push((after_revision, Arc::ptr_eq(&before, &after)));
    }
    // Complete every transition before asserting the optimization, so the old
    // implementation demonstrates five redundant builds with identical data.
    assert_eq!(
        f.resolver.lists.load(Ordering::Relaxed),
        initial_builds,
        "selection writes must not rebuild canonical production snapshots"
    );
    assert_eq!(observations, vec![(revision, true); 5]);
    assert_eq!(service.snapshot_epoch(), epoch);
    assert!(matches!(
        events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn selection_is_scoped_by_actor_role_reference_and_apparatus() {
    let f = Fixture::new().await;
    let service = &f.state.production_maps;
    let (before, revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    for (token, apparatus, code) in [
        (&f.worker, CUT, &f.codes[0]),
        (&f.other_worker, CUT, &f.codes[1]),
        (&f.admin, CUT, &f.codes[1]),
        (&f.worker, OTHER_CUT, &f.codes[1]),
    ] {
        f.selection("GET", token, apparatus, "", None).await;
        f.selection("PUT", token, apparatus, code, Some(code)).await;
    }
    for (token, apparatus, code) in [
        (&f.worker, CUT, &f.codes[0]),
        (&f.other_worker, CUT, &f.codes[1]),
        (&f.admin, CUT, &f.codes[1]),
        (&f.worker, OTHER_CUT, &f.codes[1]),
    ] {
        f.selection("GET", token, apparatus, "", Some(code)).await;
    }
    f.selection("PUT", &f.worker, CUT, "", None).await;
    f.selection("GET", &f.worker, CUT, "", None).await;
    for (token, apparatus) in [
        (&f.other_worker, CUT),
        (&f.admin, CUT),
        (&f.worker, OTHER_CUT),
    ] {
        f.selection("GET", token, apparatus, "", Some(&f.codes[1]))
            .await;
    }
    let (after, after_revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    assert_eq!(
        serde_json::to_value(after.as_ref()).unwrap(),
        serde_json::to_value(before.as_ref()).unwrap()
    );
    assert_eq!(after_revision, revision);
    assert!(Arc::ptr_eq(&before, &after));
    assert_eq!(f.resolver.lists.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn rejected_selection_preserves_selection_and_snapshot() {
    let f = Fixture::new().await;
    f.selection("PUT", &f.worker, CUT, &f.codes[0], Some(&f.codes[0]))
        .await;
    let service = &f.state.production_maps;
    let (before, revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    let mut events = service.subscribe_live();
    let oversized = "x".repeat(129);
    f.state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "unassigned".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![OTHER_CUT.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let unassigned = session(&f.state, PrincipalRole::Aparatchi, "unassigned").await;
    for (token, apparatus, code, status, error) in [
        (
            f.worker.as_str(),
            CUT,
            "missing-paddon",
            StatusCode::NOT_FOUND,
            "paddon_not_found",
        ),
        (
            f.worker.as_str(),
            CUT,
            oversized.as_str(),
            StatusCode::BAD_REQUEST,
            "paddon_invalid_input",
        ),
        (
            f.admin.as_str(),
            NON_CUT,
            f.codes[0].as_str(),
            StatusCode::BAD_REQUEST,
            "paddon_invalid_input",
        ),
        (
            unassigned.as_str(),
            CUT,
            f.codes[1].as_str(),
            StatusCode::BAD_REQUEST,
            "apparatus_not_assigned",
        ),
        (
            f.worker.as_str(),
            "Rezka",
            f.codes[0].as_str(),
            StatusCode::BAD_REQUEST,
            "canonical_apparatus_id_required",
        ),
    ] {
        let (actual, body) = f.request("PUT", token, apparatus, code).await;
        assert_eq!(actual, status, "{body}");
        assert_eq!(body["error"], error);
    }
    for token in ["invalid-token", f.denied.as_str()] {
        for method in ["GET", "PUT"] {
            let (status, _) = f.request(method, token, CUT, &f.codes[1]).await;
            assert_eq!(
                status,
                if token == "invalid-token" {
                    StatusCode::UNAUTHORIZED
                } else {
                    StatusCode::FORBIDDEN
                }
            );
        }
    }
    for bad_actor in [
        QueueActionActor {
            role: String::new(),
            ..actor()
        },
        QueueActionActor {
            ref_: String::new(),
            ..actor()
        },
    ] {
        assert!(matches!(
            service
                .set_active_rezka_paddon(CUT, &bad_actor, &f.codes[1])
                .await,
            Err(ProductionMapError::PaddonInvalidInput)
        ));
    }
    f.selection("GET", &f.worker, CUT, "", Some(&f.codes[0]))
        .await;
    let (after, after_revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    assert_eq!(after_revision, revision);
    assert!(Arc::ptr_eq(&before, &after));
    assert_eq!(f.resolver.lists.load(Ordering::Relaxed), 1);
    assert!(matches!(
        events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn production_map_mutation_still_invalidates_and_rebuilds() {
    let f = Fixture::new().await;
    f.selection("PUT", &f.worker, CUT, &f.codes[0], Some(&f.codes[0]))
        .await;
    let service = &f.state.production_maps;
    let (before, revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    let mut events = service.subscribe_live();
    service.upsert_map(map("Changed order")).await.unwrap();
    assert_eq!(
        events.try_recv().unwrap(),
        ProductionMapLiveEvent::Invalidate
    );
    let (after, after_revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    assert_eq!(after_revision, revision + 1);
    assert!(!Arc::ptr_eq(&before, &after));
    assert_eq!(f.resolver.lists.load(Ordering::Relaxed), 2);
    assert_ne!(
        serde_json::to_value(after.as_ref()).unwrap(),
        serde_json::to_value(before.as_ref()).unwrap()
    );
    assert_eq!(after.maps[0].map.title, "Changed order");
    assert!(Arc::ptr_eq(
        &after,
        &service.live_snapshot_shared().await.unwrap()
    ));
    f.selection("GET", &f.worker, CUT, "", Some(&f.codes[0]))
        .await;
}
