use super::*;
use crate::core::production_map::output_stats::{
    OutputStatsError, OutputStatsReport, OutputStatsRequest,
};
use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapError, ProductionMapStorePort, RawMaterialAssignment,
};

const CUT: &str = "apparatus:default:asset-010";
const LAM: &str = "apparatus:default:asset-007";
const URI: &str = "/v1/mobile/admin/production-maps/output-stats?from=2026-10-18T19:00:00Z&to=2026-10-25T19:00:00Z&bucket=day&timezone=Asia%2FTashkent";

#[derive(Default)]
struct ReportOnlyStore {
    scopes: Mutex<Vec<Option<Vec<String>>>>,
}

#[async_trait]
impl ProductionMapStorePort for ReportOnlyStore {
    async fn output_stats(
        &self,
        _: &OutputStatsRequest,
        scope: Option<&[String]>,
    ) -> Result<OutputStatsReport, OutputStatsError> {
        self.scopes.lock().await.push(scope.map(<[String]>::to_vec));
        Ok(OutputStatsReport(r#"{"ok":true,"series":[]}"#.into()))
    }
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        panic!("report must not list maps")
    }
    async fn put_map(&self, _: ProductionMapDefinition) -> Result<(), ProductionMapError> {
        panic!("read only")
    }
    async fn put_maps_batch(
        &self,
        _: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError> {
        panic!("read only")
    }
    async fn delete_map(&self, _: &str) -> Result<(), ProductionMapError> {
        panic!("read only")
    }
    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        panic!("report must not list queues")
    }
    async fn put_apparatus_sequence(
        &self,
        _: &str,
        _: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        panic!("read only")
    }
    async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        panic!("report must not list queue state")
    }
    async fn put_apparatus_queue_states(
        &self,
        _: &str,
        _: BTreeMap<String, String>,
    ) -> Result<(), ProductionMapError> {
        panic!("read only")
    }
    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        panic!("report must not list materials")
    }
    async fn put_raw_material_assignment(
        &self,
        _: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        panic!("read only")
    }
    async fn delete_raw_material_assignment(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
        panic!("read only")
    }
}

async fn fixture() -> (AppState, Arc<ReportOnlyStore>, String, String, String) {
    let mut state = test_state();
    let store = Arc::new(ReportOnlyStore::default());
    state.production_maps = ProductionMapService::new(
        store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(
            state.apparatus.clone(),
        )),
    );
    for (worker, id) in [("stats-cut", CUT), ("stats-lam", LAM)] {
        state
            .admin
            .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: worker.into(),
                role_id: "aparatchi".into(),
                assigned_apparatus: vec![id.into()],
                assigned_item_groups: vec![],
            })
            .await
            .unwrap();
    }
    let admin = session(&state, PrincipalRole::Admin).await;
    let cut = session_for(&state, PrincipalRole::Aparatchi, "stats-cut").await;
    let lam = session_for(&state, PrincipalRole::Aparatchi, "stats-lam").await;
    (state, store, admin, cut, lam)
}

#[tokio::test]
async fn output_stats_authentication_and_scope_are_isolated_per_request() {
    let (state, store, admin, cut, lam) = fixture().await;
    let supplier = session_for(&state, PrincipalRole::Supplier, "stats-supplier").await;
    let unassigned = session_for(&state, PrincipalRole::Aparatchi, "stats-no-assignment").await;
    let router = build_router(state);
    for (token, status) in [
        ("", StatusCode::UNAUTHORIZED),
        (supplier.as_str(), StatusCode::FORBIDDEN),
        (unassigned.as_str(), StatusCode::FORBIDDEN),
    ] {
        let response = router
            .clone()
            .oneshot(request("GET", URI, token))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    assert!(store.scopes.lock().await.is_empty());
    for token in [&cut, &lam, &admin] {
        let response = router
            .clone()
            .oneshot(request("GET", URI, token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["content-type"], "application/json");
        assert_eq!(json_body(response).await["ok"], true);
    }
    assert_eq!(
        *store.scopes.lock().await,
        vec![
            Some(vec![CUT.to_string()]),
            Some(vec![LAM.to_string()]),
            None
        ]
    );
    let response = router
        .oneshot(request("GET", &format!("{URI}&apparatus={LAM}"), &cut))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(store.scopes.lock().await.len(), 3);
}

#[tokio::test]
async fn output_stats_rejects_bad_queries_and_mutating_methods_before_store_access() {
    let (state, store, admin, _, _) = fixture().await;
    let router = build_router(state);
    let base = "/v1/mobile/admin/production-maps/output-stats";
    for suffix in [
        "",
        "?from=bad",
        "?unexpected=1",
        "?from=one&from=two",
        "?from=2026-01-01T00:00:00Z&to=2026-01-01T00:00:00Z&bucket=day&timezone=UTC",
        "?from=2026-01-01T00:00:00Z&to=2026-01-02T00:00:00Z&bucket=day&timezone=Fake%2FZone",
        "?from=2026-01-01T00:00:00Z&to=2026-01-02T00:00:00Z&bucket=month&timezone=UTC",
        "?from=2026-01-01T00:00:00Z&to=2026-01-02T00:00:00Z&bucket=day&timezone=UTC&apparatus=Rezka",
    ] {
        let response = router
            .clone()
            .oneshot(request("GET", &format!("{base}{suffix}"), &admin))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{suffix}");
        assert!(
            json_body(response).await["error"]
                .as_str()
                .unwrap()
                .starts_with("output_stats_")
        );
    }
    let response = router.oneshot(request("POST", URI, &admin)).await.unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(store.scopes.lock().await.is_empty());
}

#[tokio::test]
async fn output_stats_unsupported_store_returns_error_instead_of_empty_success() {
    let state = test_state();
    let admin = session(&state, PrincipalRole::Admin).await;
    let response = build_router(state)
        .oneshot(request("GET", URI, &admin))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "output_stats_store_failed"
    );
}

#[tokio::test]
async fn output_stats_map_manage_uses_existing_global_read_permission() {
    let (state, store, _, _, _) = fixture().await;
    let role = state
        .admin
        .upsert_role_definition(crate::core::authz::RoleDefinitionUpsert {
            id: "stats-manager".into(),
            label: "Stats manager".into(),
            base_role: Some(PrincipalRole::Customer),
            capability_codes: vec!["production.map.manage".into()],
        })
        .await
        .unwrap();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Customer,
            principal_ref: "stats-manager".into(),
            role_id: role.id,
            assigned_apparatus: vec![],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let token = session_for(&state, PrincipalRole::Customer, "stats-manager").await;
    let response = build_router(state)
        .oneshot(request("GET", URI, &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(*store.scopes.lock().await, vec![None]);
}
