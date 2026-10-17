#![cfg(feature = "verification")]
//! The real commit path must invalidate cached reads before fallible response reads.
use mini_rs_erp::core::{apparatus_standard::*, production_map::*};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Semaphore, broadcast::error::TryRecvError};

type Result<T> = std::result::Result<T, ProductionMapError>;
const APPARATUS: &str = "apparatus:test:commit-notification";
const ORDER: &str = "zakaz-commit-notification";

struct CommitStore {
    inner: MemoryProductionMapStore,
    commits: AtomicUsize,
    reject_write: AtomicBool,
    response_mode: AtomicUsize,
    response_read: Semaphore,
}

impl Default for CommitStore {
    fn default() -> Self {
        Self {
            inner: MemoryProductionMapStore::default(),
            commits: AtomicUsize::new(0),
            reject_write: AtomicBool::new(false),
            response_mode: AtomicUsize::new(0),
            response_read: Semaphore::new(0),
        }
    }
}

#[async_trait::async_trait]
impl ProductionMapStorePort for CommitStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>> {
        self.inner.maps().await
    }
    async fn put_map(&self, map: ProductionMapDefinition) -> Result<()> {
        self.inner.put_map(map).await
    }
    async fn put_maps_batch(&self, maps: &[ProductionMapDefinition]) -> Result<()> {
        self.inner.put_maps_batch(maps).await
    }
    async fn delete_map(&self, id: &str) -> Result<()> {
        self.inner.delete_map(id).await
    }
    async fn apparatus_sequences(&self) -> Result<BTreeMap<String, Vec<String>>> {
        self.inner.apparatus_sequences().await
    }
    async fn put_apparatus_sequence(&self, id: &str, orders: Vec<String>) -> Result<()> {
        self.inner.put_apparatus_sequence(id, orders).await
    }
    async fn apparatus_queue_states(&self) -> Result<BTreeMap<String, BTreeMap<String, String>>> {
        self.inner.apparatus_queue_states().await
    }
    async fn put_apparatus_queue_states(
        &self,
        id: &str,
        states: BTreeMap<String, String>,
    ) -> Result<()> {
        self.inner.put_apparatus_queue_states(id, states).await
    }
    async fn raw_material_assignments(&self) -> Result<Vec<RawMaterialAssignment>> {
        self.inner.raw_material_assignments().await
    }
    async fn put_raw_material_assignment(&self, row: RawMaterialAssignment) -> Result<()> {
        self.inner.put_raw_material_assignment(row).await
    }
    async fn delete_raw_material_assignment(
        &self,
        order: &str,
        barcode: &str,
    ) -> Result<Option<RawMaterialAssignment>> {
        self.inner
            .delete_raw_material_assignment(order, barcode)
            .await
    }
    async fn order_control_states(&self) -> Result<BTreeMap<String, OrderControlRecord>> {
        self.inner.order_control_states().await
    }
    async fn active_order_run_session(
        &self,
        apparatus: &str,
        order: &str,
    ) -> Result<Option<OrderRunSession>> {
        self.inner.active_order_run_session(apparatus, order).await
    }
    async fn order_run_sessions_for_order(&self, order: &str) -> Result<Vec<OrderRunSession>> {
        self.inner.order_run_sessions_for_order(order).await
    }
    async fn queue_action_logs_for_orders(
        &self,
        orders: &[String],
    ) -> Result<BTreeMap<String, Vec<ProductionOrderLogEntry>>> {
        self.inner.queue_action_logs_for_orders(orders).await
    }
    async fn production_order_lifecycles(
        &self,
        orders: &[String],
    ) -> Result<BTreeMap<String, ProductionOrderLifecycleRecord>> {
        // Fault only the response read after the real store has persisted Start.
        if self.commits.load(Ordering::SeqCst) > 0 {
            match self.response_mode.load(Ordering::SeqCst) {
                1 => return Err(ProductionMapError::StoreFailed),
                2 => {
                    self.response_read.add_permits(1);
                    std::future::pending::<()>().await;
                }
                _ => {}
            }
        }
        self.inner.production_order_lifecycles(orders).await
    }
    async fn put_apparatus_queue_states_with_event_and_progress(
        &self,
        write: &QueueActionProgressWrite,
    ) -> Result<QueueActionProgressWriteResult> {
        if self.reject_write.load(Ordering::SeqCst) {
            return Err(ProductionMapError::StoreFailed);
        }
        let result = self
            .inner
            .put_apparatus_queue_states_with_event_and_progress(write)
            .await?;
        self.commits.fetch_add(1, Ordering::SeqCst);
        Ok(result)
    }
}

struct Resolver(Arc<RuntimeApparatusConfiguration>);
#[async_trait::async_trait]
impl CanonicalApparatusResolver for Resolver {
    async fn resolve(
        &self,
        id: &ApparatusId,
    ) -> Result<Option<Arc<RuntimeApparatusConfiguration>>> {
        Ok((&self.0.runtime.apparatus_id == id).then(|| self.0.clone()))
    }
    async fn list(&self) -> Result<Vec<Arc<RuntimeApparatusConfiguration>>> {
        Ok(vec![self.0.clone()])
    }
}

async fn fixture() -> (ProductionMapService, Arc<CommitStore>) {
    let mut revision = mini_rs_apparatus_contract::isa95::test_support::revision_with(
        APPARATUS,
        "physical-asset:commit-notification",
        "Commit notification",
    );
    revision.policies.material = MaterialExecutionPolicy::NotRequired {
        item_group_ids: vec![],
    };
    revision.policies.tooling = ToolingExecutionPolicy::NotRequired;
    revision.capacity.availability = CapacityAvailability::Always;
    revision.validate().unwrap();
    let resolver = Resolver(Arc::new(
        project_apparatus_revision(&revision, AasxSha256::digest(APPARATUS.as_bytes())).into(),
    ));
    let store = Arc::new(CommitStore::default());
    store.inner.put_map(serde_json::from_value(json!({
        "id": ORDER, "title": "Notification order", "product_code": "P",
        "nodes": [
            {"id": "start", "kind": "start", "title": "Start"},
            {"id": "print", "kind": "apparatus", "title": "Print", "apparatus_id": APPARATUS},
            {"id": "end", "kind": "end", "title": "End"}
        ],
        "edges": [{"from": "start", "to": "print"}, {"from": "print", "to": "end"}]
    })).unwrap()).await.unwrap();
    store
        .inner
        .put_apparatus_sequence(APPARATUS, vec![ORDER.into()])
        .await
        .unwrap();
    store
        .inner
        .put_apparatus_queue_states(
            APPARATUS,
            BTreeMap::from([(ORDER.into(), "pending".into())]),
        )
        .await
        .unwrap();
    (
        ProductionMapService::new(store.clone(), Arc::new(resolver)),
        store,
    )
}

async fn action(
    service: &ProductionMapService,
    action: queue_state::ApparatusQueueAction,
) -> Result<BTreeMap<String, String>> {
    service
        .apply_apparatus_queue_action(
            APPARATUS,
            ORDER,
            action,
            &[APPARATUS.into()],
            QueueActionActor {
                role: "aparatchi".into(),
                ref_: "notification-worker".into(),
                display_name: "Worker".into(),
            },
        )
        .await
}

async fn assert_single_persisted_start(store: &CommitStore) {
    assert_eq!(store.commits.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.inner.apparatus_queue_states().await.unwrap()[APPARATUS][ORDER],
        "in_progress"
    );
    assert_eq!(
        store
            .inner
            .order_run_sessions_for_order(ORDER)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .inner
            .queue_action_logs_for_orders(&[ORDER.into()])
            .await
            .unwrap()[ORDER]
            .len(),
        1
    );
}

async fn assert_fresh_snapshot(
    service: &ProductionMapService,
    previous: &Arc<ProductionMapLiveSnapshot>,
    revision: u64,
) {
    let (current, current_revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    assert_eq!(current_revision, revision + 1);
    assert!(
        !Arc::ptr_eq(previous, &current),
        "a committed action must evict the warm snapshot"
    );
    assert_eq!(current.queue_states[APPARATUS][ORDER], "in_progress");
}

#[tokio::test]
async fn successful_start_invalidates_once() {
    let (service, store) = fixture().await;
    let (cached, revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    let mut live = service.subscribe_live();
    action(&service, queue_state::ApparatusQueueAction::Start)
        .await
        .unwrap();
    assert!(matches!(
        live.try_recv(),
        Ok(ProductionMapLiveEvent::Invalidate)
    ));
    assert!(matches!(live.try_recv(), Err(TryRecvError::Empty)));
    assert_single_persisted_start(&store).await;
    assert_fresh_snapshot(&service, &cached, revision).await;
}

#[tokio::test]
async fn committed_start_invalidates_even_when_response_status_read_fails() {
    let (service, store) = fixture().await;
    let (cached, revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    let mut live = service.subscribe_live();
    store.response_mode.store(1, Ordering::SeqCst);
    assert!(matches!(
        action(&service, queue_state::ApparatusQueueAction::Start).await,
        Err(ProductionMapError::StoreFailed)
    ));
    assert_single_persisted_start(&store).await;
    assert_eq!(service.snapshot_revision(), revision + 1);
    assert!(matches!(
        live.try_recv(),
        Ok(ProductionMapLiveEvent::Invalidate)
    ));
    store.response_mode.store(0, Ordering::SeqCst);
    assert_fresh_snapshot(&service, &cached, revision).await;
    assert!(
        action(&service, queue_state::ApparatusQueueAction::Start)
            .await
            .is_err()
    );
    assert_single_persisted_start(&store).await;
    assert_eq!(service.snapshot_revision(), revision + 1);
    assert!(matches!(live.try_recv(), Err(TryRecvError::Empty)));
}

#[tokio::test]
async fn committed_start_invalidates_before_response_read_can_be_cancelled() {
    let (service, store) = fixture().await;
    let (cached, revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    let mut live = service.subscribe_live();
    store.response_mode.store(2, Ordering::SeqCst);
    let task_service = service.clone();
    let task = tokio::spawn(async move {
        action(&task_service, queue_state::ApparatusQueueAction::Start).await
    });
    tokio::time::timeout(Duration::from_secs(10), store.response_read.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_single_persisted_start(&store).await;
    assert_eq!(service.snapshot_revision(), revision + 1);
    assert!(matches!(
        live.try_recv(),
        Ok(ProductionMapLiveEvent::Invalidate)
    ));
    store.response_mode.store(0, Ordering::SeqCst);
    assert_fresh_snapshot(&service, &cached, revision).await;
    assert!(
        action(&service, queue_state::ApparatusQueueAction::Start)
            .await
            .is_err()
    );
    assert_single_persisted_start(&store).await;
    assert!(matches!(live.try_recv(), Err(TryRecvError::Empty)));
}

#[tokio::test]
async fn rejected_actions_do_not_invalidate_or_persist() {
    let (service, store) = fixture().await;
    let (cached, revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    let mut live = service.subscribe_live();
    // A domain rejection and a failed commit both precede invalidation.
    assert!(
        action(&service, queue_state::ApparatusQueueAction::Resume)
            .await
            .is_err()
    );
    store.reject_write.store(true, Ordering::SeqCst);
    assert!(matches!(
        action(&service, queue_state::ApparatusQueueAction::Start).await,
        Err(ProductionMapError::StoreFailed)
    ));
    assert_eq!(store.commits.load(Ordering::SeqCst), 0);
    assert!(
        store
            .inner
            .order_run_sessions_for_order(ORDER)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .inner
            .queue_action_logs_for_orders(&[ORDER.into()])
            .await
            .unwrap()
            .values()
            .all(Vec::is_empty)
    );
    assert_eq!(service.snapshot_revision(), revision);
    assert!(matches!(live.try_recv(), Err(TryRecvError::Empty)));
    let (current, _) = service.live_snapshot_shared_with_revision().await.unwrap();
    assert!(Arc::ptr_eq(&cached, &current));
    assert_eq!(current.queue_states[APPARATUS][ORDER], "pending");
}
