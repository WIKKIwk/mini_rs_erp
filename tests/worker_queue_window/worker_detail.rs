use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

type StoreResult<T> = Result<T, ProductionMapError>;
type ApparatusSequenceMap = BTreeMap<String, Vec<String>>;
type QueueStateMap = BTreeMap<String, String>;
type ApparatusQueueStateMap = BTreeMap<String, QueueStateMap>;

fn query(order: &str) -> String {
    format!("worker_scope=true&detail=true&apparatus={CUT}&order_id={order}")
}

/// The expensive candidate selection is observable without a wall-clock limit.
/// A detail must begin from one order, rather than its sixty queue neighbours.
struct ScopedReads {
    inner: Arc<MemoryProductionMapStore>,
    scopes: std::sync::Mutex<Vec<(Vec<String>, Vec<String>)>>,
    gate_first: AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

impl ScopedReads {
    fn new(inner: Arc<MemoryProductionMapStore>, gated: bool) -> Self {
        Self {
            inner,
            scopes: std::sync::Mutex::new(Vec::new()),
            gate_first: AtomicBool::new(gated),
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        }
    }
}

#[async_trait::async_trait]
impl ProductionMapStorePort for ScopedReads {
    async fn maps(&self) -> StoreResult<Vec<ProductionMapDefinition>> {
        self.inner.maps().await
    }
    async fn maps_for_snapshot_scope(
        &self,
        apparatus: &[String],
        orders: &[String],
    ) -> StoreResult<Vec<ProductionMapDefinition>> {
        self.scopes
            .lock()
            .unwrap()
            .push((apparatus.to_vec(), orders.to_vec()));
        if self.gate_first.swap(false, Ordering::AcqRel) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        // Match the persistent store's candidate superset. The service still
        // owns the actual selected-alternative and route visibility rules.
        Ok(self
            .inner
            .maps()
            .await?
            .into_iter()
            .filter(|map| {
                orders.contains(&map.id)
                    || map.nodes.iter().any(|node| {
                        apparatus.contains(&node.apparatus_id)
                            || apparatus.contains(&node.alternative_assigned_apparatus_id)
                    })
            })
            .collect())
    }
    async fn put_map(&self, map: ProductionMapDefinition) -> StoreResult<()> {
        self.inner.put_map(map).await
    }
    async fn put_maps_batch(&self, maps: &[ProductionMapDefinition]) -> StoreResult<()> {
        self.inner.put_maps_batch(maps).await
    }
    async fn delete_map(&self, id: &str) -> StoreResult<()> {
        self.inner.delete_map(id).await
    }
    async fn apparatus_sequences(&self) -> StoreResult<ApparatusSequenceMap> {
        self.inner.apparatus_sequences().await
    }
    async fn put_apparatus_sequence(
        &self,
        apparatus: &str,
        orders: Vec<String>,
    ) -> StoreResult<()> {
        self.inner.put_apparatus_sequence(apparatus, orders).await
    }
    async fn apparatus_queue_states(&self) -> StoreResult<ApparatusQueueStateMap> {
        self.inner.apparatus_queue_states().await
    }
    async fn put_apparatus_queue_states(
        &self,
        apparatus: &str,
        states: QueueStateMap,
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
        order: &str,
        barcode: &str,
    ) -> StoreResult<Option<RawMaterialAssignment>> {
        self.inner
            .delete_raw_material_assignment(order, barcode)
            .await
    }
}

fn install_scoped_reads(f: &mut Fixture, gated: bool) -> Arc<ScopedReads> {
    let reads = Arc::new(ScopedReads::new(f.store.clone(), gated));
    f.state.production_maps = ProductionMapService::new(
        reads.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(
            f.state.apparatus.clone(),
        )),
    );
    f.router = build_router(f.state.clone());
    reads
}

#[tokio::test]
async fn worker_detail_cold_read_materializes_one_order_before_its_queue_peers() {
    let _guard = FIXTURE_LOCK.lock().await;
    let mut f = Fixture::new().await;
    let reads = install_scoped_reads(&mut f, false);
    let (status, detail) = f.get(&query("zakaz-059")).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["maps"].as_array().unwrap().len(), 1);
    assert_eq!(detail["maps"][0]["map"]["id"], "zakaz-059");
    let scopes = reads.scopes.lock().unwrap();
    assert_eq!(scopes[0], (Vec::<String>::new(), vec!["zakaz-059".into()]));
    assert!(scopes.iter().all(|(_, ids)| ids == &["zakaz-059"]));
}

#[tokio::test]
async fn worker_detail_matches_full_worker_controls_across_queue_states_and_policies() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    for free_pick in [false, true] {
        if free_pick {
            f.free_pick().await;
        }
        for state in [
            "pending",
            "in_progress",
            "paused",
            "roll_detached",
            "frozen",
            "completed",
        ] {
            f.store
                .put_apparatus_queue_states(
                    CUT,
                    BTreeMap::from([("zakaz-000".into(), state.into())]),
                )
                .await
                .unwrap();
            f.state.production_maps.notify_live();
            // A cold detail goes first, so no global/worker snapshot can hide
            // accidental reconstruction or incorrect dependency selection.
            let (status, detail) = f.get(&query("zakaz-000")).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "free={free_pick} state={state} {detail}"
            );
            let (status, full) = f.get("worker_scope=true").await;
            assert_eq!(status, StatusCode::OK, "{full}");
            for field in ["queue_action_controls", "queue_states"] {
                assert_eq!(
                    detail[field][CUT]["zakaz-000"], full[field][CUT]["zakaz-000"],
                    "{field} free={free_pick} state={state}"
                );
            }
            assert_eq!(
                detail["stage_states"]["zakaz-000"],
                full["stage_states"]["zakaz-000"]
            );
            assert_eq!(detail["rev"], full["rev"]);
            assert_eq!(detail["epoch"], full["epoch"]);
            assert_eq!(
                detail["sequence_versions"][CUT],
                full["sequence_versions"][CUT]
            );
        }
    }
}

#[tokio::test]
async fn worker_detail_does_not_publish_authority_after_assignment_changes_during_read() {
    let _guard = FIXTURE_LOCK.lock().await;
    let mut f = Fixture::new().await;
    let reads = install_scoped_reads(&mut f, true);
    let detail_query = query("zakaz-000");
    let request = f.get(&detail_query);
    let revoke = async {
        reads.entered.notified().await;
        f.state
            .admin
            .upsert_role_assignment(RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: "window-worker".into(),
                role_id: "aparatchi".into(),
                assigned_apparatus: vec![OTHER.into()],
                assigned_item_groups: vec![],
            })
            .await
            .unwrap();
        reads.release.notify_one();
    };
    let ((status, response), _) = tokio::join!(request, revoke);
    assert_eq!(status, StatusCode::FORBIDDEN, "{response}");
    assert!(response.get("maps").is_none());
}

#[tokio::test]
async fn worker_detail_retries_a_changed_revision_before_publishing_its_program() {
    let _guard = FIXTURE_LOCK.lock().await;
    let mut f = Fixture::new().await;
    let reads = install_scoped_reads(&mut f, true);
    let detail_query = query("zakaz-000");
    let request = f.get(&detail_query);
    let edit = async {
        reads.entered.notified().await;
        let mut changed = map("zakaz-000", CUT);
        changed.title = "Edited during detail read".into();
        f.state.production_maps.upsert_map(changed).await.unwrap();
        reads.release.notify_one();
    };
    let ((status, detail), _) = tokio::join!(request, edit);
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(
        detail["maps"][0]["map"]["title"],
        "Edited during detail read"
    );
    assert_eq!(detail["rev"], f.state.production_maps.snapshot_revision());
    assert!(
        reads.scopes.lock().unwrap().len() >= 4,
        "revision churn rebuilt the requested snapshot"
    );
}
