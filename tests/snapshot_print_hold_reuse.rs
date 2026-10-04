#![cfg(feature = "verification")]
//! Full-snapshot baselines come from the unchanged production library at ba2e0ac.
//! Store probes exercise the shipping evaluator, cache, retries and error paths.
use mini_rs_erp::core::{apparatus_standard::*, production_map::*};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::sync::Notify;

type Result<T> = std::result::Result<T, ProductionMapError>;
type States = BTreeMap<String, BTreeMap<String, String>>;
const MACHINE: &str = "apparatus:test:machine-0";

#[derive(Default)]
struct Gate {
    entered: Notify,
    resume: Notify,
}
#[derive(Default)]
struct FixtureStore {
    maps: Vec<ProductionMapDefinition>,
    holds: Mutex<Vec<PrintPreflightHold>>,
    hold_reads: AtomicUsize,
    material_reads: AtomicUsize,
    fail_holds: AtomicBool,
    fail_materials: AtomicBool,
    hold_gate: Mutex<Option<Arc<Gate>>>,
}
#[async_trait::async_trait]
impl ProductionMapStorePort for FixtureStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>> {
        Ok(self.maps.clone())
    }
    async fn put_map(&self, _: ProductionMapDefinition) -> Result<()> {
        panic!("read-only projection wrote map")
    }
    async fn put_maps_batch(&self, _: &[ProductionMapDefinition]) -> Result<()> {
        panic!("read-only projection wrote maps")
    }
    async fn delete_map(&self, _: &str) -> Result<()> {
        panic!("read-only projection deleted map")
    }
    async fn apparatus_sequences(&self) -> Result<BTreeMap<String, Vec<String>>> {
        Ok(BTreeMap::new())
    }
    async fn put_apparatus_sequence(&self, _: &str, _: Vec<String>) -> Result<()> {
        panic!("read-only projection wrote sequence")
    }
    async fn apparatus_queue_states(&self) -> Result<States> {
        Ok(BTreeMap::new())
    }
    async fn put_apparatus_queue_states(&self, _: &str, _: BTreeMap<String, String>) -> Result<()> {
        panic!("read-only projection wrote state")
    }
    async fn raw_material_assignments(&self) -> Result<Vec<RawMaterialAssignment>> {
        self.material_reads.fetch_add(1, Ordering::SeqCst);
        if self.fail_materials.load(Ordering::SeqCst) {
            return Err(ProductionMapError::StoreFailed);
        }
        Ok(vec![])
    }
    async fn put_raw_material_assignment(&self, _: RawMaterialAssignment) -> Result<()> {
        panic!("read-only projection wrote material")
    }
    async fn delete_raw_material_assignment(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Option<RawMaterialAssignment>> {
        panic!("read-only projection deleted material")
    }
    async fn production_order_lifecycles(
        &self,
        ids: &[String],
    ) -> Result<BTreeMap<String, ProductionOrderLifecycleRecord>> {
        Ok(ids
            .iter()
            .map(|id| (id.clone(), ProductionOrderLifecycleRecord::released(id)))
            .collect())
    }
    async fn active_print_preflight_holds(&self) -> Result<Vec<PrintPreflightHold>> {
        self.hold_reads.fetch_add(1, Ordering::SeqCst);
        if self.fail_holds.load(Ordering::SeqCst) {
            return Err(ProductionMapError::PrintPreflightNotReady);
        }
        let holds = self.holds.lock().unwrap().clone();
        let gate = self.hold_gate.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.entered.notify_one();
            gate.resume.notified().await;
        }
        Ok(holds)
    }
}

struct Resolver(Vec<Arc<RuntimeApparatusConfiguration>>);
#[async_trait::async_trait]
impl CanonicalApparatusResolver for Resolver {
    async fn resolve(
        &self,
        id: &ApparatusId,
    ) -> Result<Option<Arc<RuntimeApparatusConfiguration>>> {
        Ok(self
            .0
            .iter()
            .find(|c| &c.runtime.apparatus_id == id)
            .cloned())
    }
    async fn list(&self) -> Result<Vec<Arc<RuntimeApparatusConfiguration>>> {
        Ok(self.0.clone())
    }
}
fn service(store: Arc<FixtureStore>) -> ProductionMapService {
    let resolver = Resolver(
        (0..3)
            .map(|i| {
                let id = format!("apparatus:test:machine-{i}");
                let revision = mini_rs_apparatus_contract::isa95::test_support::revision_with(
                    &id,
                    &format!("physical-asset:machine-{i}"),
                    &format!("Machine {i}"),
                );
                Arc::new(
                    project_apparatus_revision(&revision, AasxSha256::digest(id.as_bytes())).into(),
                )
            })
            .collect(),
    );
    ProductionMapService::new(store, Arc::new(resolver))
}
fn map(id: &str) -> ProductionMapDefinition {
    serde_json::from_value(json!({"id":id,"title":"Order","product_code":"P",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"node-0","kind":"apparatus","title":"Machine 0","apparatus_id":MACHINE},
            {"id":"node-1","kind":"apparatus","title":"Machine 1","apparatus_id":"apparatus:test:machine-1"},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"node-0"},{"from":"node-0","to":"node-1"},{"from":"node-1","to":"end"}]})).unwrap()
}
fn hold(
    id: &str,
    apparatus: &str,
    order: &str,
    status: PrintPreflightStatus,
) -> PrintPreflightHold {
    PrintPreflightHold {
        hold_id: id.into(),
        idempotency_key: format!("key-{id}"),
        order_id: order.into(),
        apparatus: apparatus.into(),
        stage_node_id: "node-0".into(),
        status,
        actor: QueueActionActor {
            role: "aparatchi".into(),
            ref_: "fixture".into(),
            display_name: "Fixture".into(),
        },
        created_at_unix: 1,
        updated_at_unix: 2,
        previous_queue_state: Some("pending".into()),
        expires_at_unix: 0,
    }
}
fn fixture(count: usize, holds: Vec<PrintPreflightHold>) -> Arc<FixtureStore> {
    Arc::new(FixtureStore {
        maps: (0..count).map(|i| map(&format!("order-{i}"))).collect(),
        holds: Mutex::new(holds),
        ..Default::default()
    })
}
fn cases() -> Vec<(&'static str, Arc<FixtureStore>)> {
    use PrintPreflightStatus::*;
    let mut rows = vec![
        ("zero", fixture(0, vec![])),
        ("one", fixture(1, vec![])),
        ("many", fixture(285, vec![])),
    ];
    for (name, status) in [
        ("held", Held),
        ("running", Running),
        ("passed", Passed),
        ("failed", Failed),
        ("cancelled", Cancelled),
        ("consumed", Consumed),
    ] {
        rows.push((
            name,
            fixture(3, vec![hold(name, MACHINE, "order-0", status)]),
        ));
    }
    rows.push((
        "mixed",
        fixture(
            3,
            vec![
                hold("running", MACHINE, "order-0", Running),
                hold("passed", "apparatus:test:machine-1", "order-1", Passed),
                hold("failed", "apparatus:test:machine-2", "order-2", Failed),
            ],
        ),
    ));
    rows.push((
        "duplicates",
        fixture(
            3,
            vec![
                hold("first", MACHINE, "order-0", Held),
                hold("last-live", MACHINE, "order-1", Passed),
                hold("last-dead", MACHINE, "order-2", Cancelled),
            ],
        ),
    ));
    rows.push((
        "raw",
        Arc::new(FixtureStore {
            maps: vec![
                map("order-0"),
                map(" order-1 "),
                map(""),
                map(" "),
                map("order-0"),
            ],
            holds: Mutex::new(vec![
                hold("exact", MACHINE, "order-0", Held),
                hold("trimmed", &format!(" {MACHINE} "), " order-0 ", Passed),
                hold("blank", " ", " ", Running),
                hold("unknown", "apparatus:test:unknown", "absent", Held),
            ]),
            ..Default::default()
        }),
    ));
    rows
}
fn digest(snapshot: &ProductionMapLiveSnapshot) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(snapshot).unwrap())
    )
}
#[tokio::test]
async fn complete_snapshots_match_pre_optimization_baseline() {
    for (name, store) in cases() {
        let svc = service(store.clone());
        let snapshot = svc.live_snapshot().await.unwrap();
        let expected = match name {
            "zero" => "8f307a0cefa918e43bc07923b2c706c8ad3e33fe70d18e643d216b0de12a72fa",
            "one" => "04ed1d5d04907e1790dd330a7ef950233a9ed18b91b15f8f86825cfbdbd518e5",
            "many" => "580893f45995790e947c2a6957a219e798c8cbe8c209cedb9c031f9fa6dfa230",
            "held" => "2c66e5c4fc504542c4f6cb9dc9225595a4ba3c7ac547924ec22c5a8016f4f50a",
            "running" => "80f42aa321d27ae2385eb5182871b035e2dcd52b063c0cf35416d57b407d76f4",
            "passed" => "ccd508068e6e0a21f9c8e297b7c26153174b9717595ce458264844828765de39",
            "failed" => "34a2fc1583c3415b5aca57714b464b986ff7d05cae807f8da817be59f2969a85",
            "cancelled" => "34a2fc1583c3415b5aca57714b464b986ff7d05cae807f8da817be59f2969a85",
            "consumed" => "34a2fc1583c3415b5aca57714b464b986ff7d05cae807f8da817be59f2969a85",
            "mixed" => "10bba16b830d85e9de499f50a2cdd891fa305bb5a363443a6f4afed26e02af61",
            "duplicates" => "34ae4ba73dc2d32810a275f8326205571618fc759aa564a274bf1a4a472b3b3e",
            "raw" => "c3e29afe8d7fb1d246cf1378830a5f87f67deb94da4f64f3e4b422f532545c1f",
            _ => unreachable!(),
        };
        assert_eq!(digest(&snapshot), expected, "complete snapshot: {name}");
        assert_eq!(
            store.hold_reads.load(Ordering::SeqCst),
            1,
            "hold reads: {name}"
        );
    }
}

#[tokio::test]
async fn cached_snapshot_shares_arc_without_reloading_holds() {
    let store = fixture(
        1,
        vec![hold(
            "first",
            MACHINE,
            "order-0",
            PrintPreflightStatus::Held,
        )],
    );
    let svc = service(store.clone());
    let (first, revision) = svc.live_snapshot_shared_with_revision().await.unwrap();
    store.fail_holds.store(true, Ordering::SeqCst);
    let (cached, cached_revision) = svc.live_snapshot_shared_with_revision().await.unwrap();
    assert!(Arc::ptr_eq(&first, &cached));
    assert_eq!(cached_revision, revision);
    assert_eq!(store.hold_reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn standalone_and_scoped_controls_read_fresh_holds_despite_cached_snapshot() {
    let store = fixture(2, vec![]);
    let svc = service(store.clone());
    let cached = svc.live_snapshot_shared().await.unwrap();
    assert!(
        cached.queue_action_controls[MACHINE]["order-0"]
            .print_preflight
            .is_none()
    );
    let held = hold("new", MACHINE, "order-0", PrintPreflightStatus::Held);
    *store.holds.lock().unwrap() = vec![held.clone()];
    let controls = svc.queue_action_controls().await.unwrap();
    assert_eq!(controls[MACHINE]["order-0"].print_preflight, Some(held));
    let passed = hold("new", MACHINE, "order-0", PrintPreflightStatus::Passed);
    *store.holds.lock().unwrap() = vec![passed.clone()];
    let scoped = verification_stage_work::scoped_queue_controls(&svc, MACHINE)
        .await
        .unwrap();
    assert_eq!(scoped.as_object().unwrap().len(), 1);
    assert_eq!(scoped[MACHINE]["order-0"]["print_preflight"], json!(passed));
    assert_eq!(store.hold_reads.load(Ordering::SeqCst), 3);
    assert!(Arc::ptr_eq(
        &cached,
        &svc.live_snapshot_shared().await.unwrap()
    ));
}

#[tokio::test]
async fn invalidation_after_hold_capture_retries_with_fresh_holds() {
    let store = fixture(
        2,
        vec![hold("old", MACHINE, "order-0", PrintPreflightStatus::Held)],
    );
    let svc = service(store.clone());
    let gate = Arc::new(Gate::default());
    *store.hold_gate.lock().unwrap() = Some(gate.clone());
    let build = tokio::spawn({
        let svc = svc.clone();
        async move { svc.live_snapshot_shared_with_revision().await }
    });
    gate.entered.notified().await;
    let latest = hold("latest", MACHINE, "order-1", PrintPreflightStatus::Passed);
    *store.holds.lock().unwrap() = vec![latest.clone()];
    svc.notify_live();
    gate.resume.notify_one();
    let (snapshot, revision) = build.await.unwrap().unwrap();
    assert_eq!(revision, 1);
    assert_eq!(
        snapshot.queue_action_controls[MACHINE]["order-1"].print_preflight,
        Some(latest.clone())
    );
    assert!(
        snapshot.queue_action_controls[MACHINE]["order-0"]
            .print_preflight
            .is_none()
    );
    let expected = service(fixture(2, vec![latest]))
        .live_snapshot()
        .await
        .unwrap();
    assert_eq!(json!(snapshot.as_ref()), json!(expected));
    assert_eq!(
        store.hold_reads.load(Ordering::SeqCst),
        2,
        "one hold read in each attempted rebuild"
    );
    assert_eq!(
        store.material_reads.load(Ordering::SeqCst),
        2,
        "first revision was discarded"
    );
    let cached = svc.live_snapshot_shared().await.unwrap();
    assert!(Arc::ptr_eq(&snapshot, &cached));
    assert_eq!(store.hold_reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn concurrent_cold_readers_share_one_rebuild() {
    let store = fixture(3, vec![]);
    let svc = service(store.clone());
    let gate = Arc::new(Gate::default());
    *store.hold_gate.lock().unwrap() = Some(gate.clone());
    let mut requests = vec![];
    for _ in 0..8 {
        let svc = svc.clone();
        requests.push(tokio::spawn(async move {
            svc.live_snapshot_shared().await.unwrap()
        }));
    }
    gate.entered.notified().await;
    // Let all other requests reach the held rebuild lock before release.
    tokio::task::yield_now().await;
    assert_eq!(store.hold_reads.load(Ordering::SeqCst), 1);
    gate.resume.notify_one();
    let mut snapshots = vec![];
    for request in requests {
        snapshots.push(request.await.unwrap());
    }
    assert!(
        snapshots
            .iter()
            .all(|snapshot| Arc::ptr_eq(&snapshots[0], snapshot))
    );
    assert_eq!(store.hold_reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn first_hold_error_propagates_and_failed_build_is_not_cached() {
    let store = fixture(1, vec![]);
    let svc = service(store.clone());
    store.fail_holds.store(true, Ordering::SeqCst);
    assert!(matches!(
        svc.live_snapshot().await,
        Err(ProductionMapError::PrintPreflightNotReady)
    ));
    assert_eq!(store.material_reads.load(Ordering::SeqCst), 0);
    store.fail_holds.store(false, Ordering::SeqCst);
    let first = svc.live_snapshot_shared().await.unwrap();
    assert_eq!(store.hold_reads.load(Ordering::SeqCst), 2);
    svc.notify_live();
    store.fail_holds.store(true, Ordering::SeqCst);
    assert!(matches!(
        svc.live_snapshot().await,
        Err(ProductionMapError::PrintPreflightNotReady)
    ));
    store.fail_holds.store(false, Ordering::SeqCst);
    let (second, revision) = svc.live_snapshot_shared_with_revision().await.unwrap();
    assert_eq!(revision, 1);
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(json!(first.as_ref()), json!(second.as_ref()));
    assert_eq!(store.hold_reads.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn controls_keep_error_precedence_and_retry_after_presentation_failure() {
    let store = fixture(1, vec![]);
    let svc = service(store.clone());
    store.fail_holds.store(true, Ordering::SeqCst);
    store.fail_materials.store(true, Ordering::SeqCst);
    assert!(matches!(
        svc.queue_action_controls().await,
        Err(ProductionMapError::StoreFailed)
    ));
    assert!(matches!(
        verification_stage_work::scoped_queue_controls(&svc, MACHINE).await,
        Err(ProductionMapError::StoreFailed)
    ));
    store.fail_materials.store(false, Ordering::SeqCst);
    assert!(matches!(
        svc.queue_action_controls().await,
        Err(ProductionMapError::PrintPreflightNotReady)
    ));
    assert!(matches!(
        verification_stage_work::scoped_queue_controls(&svc, MACHINE).await,
        Err(ProductionMapError::PrintPreflightNotReady)
    ));
    assert_eq!(store.hold_reads.load(Ordering::SeqCst), 4);
    store.fail_holds.store(false, Ordering::SeqCst);
    store.fail_materials.store(true, Ordering::SeqCst);
    assert!(matches!(
        svc.live_snapshot().await,
        Err(ProductionMapError::StoreFailed)
    ));
    store.fail_materials.store(false, Ordering::SeqCst);
    svc.live_snapshot().await.unwrap();
    assert_eq!(store.hold_reads.load(Ordering::SeqCst), 6);
}
