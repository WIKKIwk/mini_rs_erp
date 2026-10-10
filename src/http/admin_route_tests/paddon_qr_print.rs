use super::*;
use crate::core::production_map::{
    PaddonReceipt, PaddonSnapshot, PaddonSummary, ProductionMapDefinition, ProductionMapError,
    ProductionMapStorePort, RawMaterialAssignment,
};

const PATH: &str = "/v1/mobile/admin/production-maps/paddons/qr/print";
const CUT: &str = "apparatus:default:asset-010";
const CODE: &str = "00001";
type StoreResult<T> = Result<T, ProductionMapError>;

struct PaddonQrStore {
    inner: MemoryProductionMapStore,
    snapshot: PaddonSnapshot,
    reads: AtomicUsize,
}

#[async_trait]
impl ProductionMapStorePort for PaddonQrStore {
    async fn maps(&self) -> StoreResult<Vec<ProductionMapDefinition>> {
        self.inner.maps().await
    }
    async fn put_map(&self, map: ProductionMapDefinition) -> StoreResult<()> {
        self.inner.put_map(map).await
    }
    async fn put_maps_batch(&self, maps: &[ProductionMapDefinition]) -> StoreResult<()> {
        self.inner.put_maps_batch(maps).await
    }
    async fn delete_map(&self, map_id: &str) -> StoreResult<()> {
        self.inner.delete_map(map_id).await
    }
    async fn apparatus_sequences(&self) -> StoreResult<BTreeMap<String, Vec<String>>> {
        self.inner.apparatus_sequences().await
    }
    async fn put_apparatus_sequence(&self, apparatus: &str, ids: Vec<String>) -> StoreResult<()> {
        self.inner.put_apparatus_sequence(apparatus, ids).await
    }
    async fn apparatus_queue_states(
        &self,
    ) -> StoreResult<BTreeMap<String, BTreeMap<String, String>>> {
        self.inner.apparatus_queue_states().await
    }
    async fn put_apparatus_queue_states(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
    ) -> StoreResult<()> {
        self.inner
            .put_apparatus_queue_states(apparatus, states)
            .await
    }
    async fn raw_material_assignments(&self) -> StoreResult<Vec<RawMaterialAssignment>> {
        self.inner.raw_material_assignments().await
    }
    async fn put_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
    ) -> StoreResult<()> {
        self.inner.put_raw_material_assignment(assignment).await
    }
    async fn delete_raw_material_assignment(
        &self,
        order_id: &str,
        barcode: &str,
    ) -> StoreResult<Option<RawMaterialAssignment>> {
        self.inner
            .delete_raw_material_assignment(order_id, barcode)
            .await
    }
    async fn paddon_scan_snapshot(&self, code: &str) -> StoreResult<Option<PaddonSnapshot>> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        Ok((code == CODE).then(|| self.snapshot.clone()))
    }
    async fn paddon_management_settings(&self) -> StoreResult<crate::core::production_map::PaddonManagementSettings> {
        Ok(crate::core::production_map::PaddonManagementSettings {
            worker_visibility_enabled: true,
            ..Default::default()
        })
    }
    async fn paddon_receipt(&self, code: &str) -> StoreResult<Option<PaddonReceipt>> {
        Ok(
            (code == CODE && self.snapshot.paddon.locked_at_unix.is_some()).then(|| {
                PaddonReceipt {
                    paddon: self.snapshot.paddon.clone(),
                    items: self.snapshot.items.clone(),
                    stocks: vec![],
                    warehouse: "WH-1".into(),
                    accepted_by_ref: "original-keeper".into(),
                    accepted_by_display_name: "Original keeper".into(),
                    accepted_at_unix: 4,
                }
            }),
        )
    }
}

fn print_state(
    locked: bool,
) -> (
    AppState,
    Arc<PaddonQrStore>,
    Arc<Mutex<Vec<ScaleDriverPrintRequest>>>,
) {
    let mut state = test_state();
    let store = Arc::new(PaddonQrStore {
        inner: MemoryProductionMapStore::new(),
        snapshot: PaddonSnapshot {
            paddon: PaddonSummary {
                id: "paddon-other-worker".into(),
                code: CODE.into(),
                location: "Cut".into(),
                note: String::new(),
                created_by_ref: "other-worker".into(),
                created_by_display_name: "Other worker".into(),
                created_at_unix: 1,
                updated_at_unix: 2,
                item_count: 0,
                locked_at_unix: locked.then_some(3),
                total_gross_kg: Some(0.0),
                total_net_kg: Some(0.0),
            },
            items: vec![],
            available_items: vec![],
            free_movement_enabled: false,
            can_manage_items: !locked,
        },
        reads: AtomicUsize::new(0),
    });
    state.production_maps = ProductionMapService::new(
        store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(
            state.apparatus.clone(),
        )),
    );
    let requests = Arc::new(Mutex::new(Vec::new()));
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: requests.clone(),
        fail: false,
    }));
    (state, store, requests)
}

fn print_body(code: &str, transport: &str) -> String {
    serde_json::json!({
        "code": code, "printer": "zebra", "print_mode": "qr",
        "print_transport": transport, "print_count": 1,
    })
    .to_string()
}

#[tokio::test]
async fn paddon_qr_reprint_allows_every_authenticated_role_without_changing_the_paddon() {
    let (state, store, requests) = print_state(true);
    let router = build_router(state.clone());
    let roles = [
        PrincipalRole::Supplier,
        PrincipalRole::Werka,
        PrincipalRole::Customer,
        PrincipalRole::Aparatchi,
        PrincipalRole::Qolipchi,
        PrincipalRole::Boyoqchi,
        PrincipalRole::MaterialTaminotchi,
        PrincipalRole::Admin,
        PrincipalRole::TayyorlovMasteri,
        PrincipalRole::HomashyoRezkachi,
    ];
    for role in roles {
        // No apparatus or warehouse assignment, and a different original creator.
        let token = session_for(&state, role, "reprinting-user").await;
        for transport in ["offline", "wifi"] {
            let response = router
                .clone()
                .oneshot(request_with_body(
                    "POST",
                    PATH,
                    &token,
                    &print_body(CODE, transport),
                ))
                .await
                .unwrap();
            let status = response.status();
            let value = json_body(response).await;
            assert_eq!(status, StatusCode::OK, "{role:?} / {transport}: {value}");
            assert_eq!(value["qr_payload"], CODE);
            assert_eq!(value["print"]["qr_payload"], CODE);
            assert_eq!(value["print"]["label_kind"], "paddon_code");
            assert_eq!(
                value["paddon"],
                serde_json::to_value(&store.snapshot.paddon).unwrap()
            );
            assert_eq!(value["can_close_after_print"], false);
        }
    }
    assert_eq!(store.reads.load(Ordering::Relaxed), roles.len() * 2);
    assert_eq!(requests.lock().await.len(), roles.len());

    // A received paddon also prints from another warehouse keeper's profile.
    assign_warehouse_to_principal(&state, PrincipalRole::Werka, "other-keeper", "WH-2").await;
    let token = session_for(&state, PrincipalRole::Werka, "other-keeper").await;
    let response = router
        .oneshot(request_with_body(
            "POST",
            PATH,
            &token,
            &print_body(CODE, "offline"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["qr_payload"], CODE);
}

#[tokio::test]
async fn paddon_qr_print_still_rejects_missing_invalid_revoked_and_expired_sessions() {
    let (state, store, requests) = print_state(true);
    let revoked = session_for(&state, PrincipalRole::Aparatchi, "revoked").await;
    state.sessions.delete(&revoked).await.unwrap();
    let router = build_router(state.clone());
    for token in ["", "unknown-token", revoked.as_str()] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                PATH,
                token,
                &print_body(CODE, "wifi"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let mut expired_state = state;
    let expired_dir = tempfile::tempdir().unwrap();
    let expired_path = expired_dir.path().join("sessions.json");
    tokio::fs::write(&expired_path, r#"{"expired-token":{"principal":{"role":"admin","display_name":"Expired Admin"},"expires_at":"2000-01-01T00:00:00Z"}}"#).await.unwrap();
    expired_state.sessions = SessionManager::persistent(expired_path, None);
    let response = build_router(expired_state)
        .oneshot(request_with_body(
            "POST",
            PATH,
            "expired-token",
            &print_body(CODE, "wifi"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(store.reads.load(Ordering::Relaxed), 0);
    assert!(requests.lock().await.is_empty());
}

#[tokio::test]
async fn paddon_qr_print_requires_a_valid_existing_code_and_preserves_mutation_permissions() {
    let (state, _, requests) = print_state(true);
    let token = session(&state, PrincipalRole::Customer).await;
    let router = build_router(state);
    for (method, path, body, expected) in [
        ("GET", PATH, String::new(), StatusCode::METHOD_NOT_ALLOWED),
        (
            "POST",
            PATH,
            print_body(" ", "wifi"),
            StatusCode::BAD_REQUEST,
        ),
        (
            "POST",
            PATH,
            print_body("missing", "wifi"),
            StatusCode::NOT_FOUND,
        ),
        ("POST", PATH, "{".into(), StatusCode::BAD_REQUEST),
        (
            "POST",
            "/v1/mobile/admin/production-maps/paddons/qr/confirm",
            r#"{"code":"00001"}"#.into(),
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            "/v1/mobile/admin/production-maps/paddons/active/next",
            serde_json::json!({"code":CODE,"apparatus":CUT}).to_string(),
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            "/v1/mobile/admin/production-maps/paddons/delete",
            r#"{"code":"00001"}"#.into(),
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body(method, path, &token, &body))
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{method} {path}: {body}");
    }
    assert!(requests.lock().await.is_empty());
}

#[tokio::test]
async fn paddon_qr_first_print_only_offers_closing_to_an_authorized_cut_worker() {
    let (state, store, _) = print_state(false);
    for (worker, apparatus) in [
        ("cut-worker", CUT),
        ("other-worker", "apparatus:default:asset-007"),
    ] {
        state
            .admin
            .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: worker.into(),
                role_id: "aparatchi".into(),
                assigned_apparatus: vec![apparatus.into()],
                assigned_item_groups: vec![],
            })
            .await
            .unwrap();
    }
    let router = build_router(state.clone());
    for (worker, can_close) in [
        ("cut-worker", true),
        ("other-worker", false),
        ("unassigned-worker", false),
    ] {
        let token = session_for(&state, PrincipalRole::Aparatchi, worker).await;
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                PATH,
                &token,
                &print_body(CODE, "offline"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value = json_body(response).await;
        assert_eq!(value["can_close_after_print"], can_close, "{worker}");
        assert_eq!(
            value["paddon"],
            serde_json::to_value(&store.snapshot.paddon).unwrap()
        );
    }
}
