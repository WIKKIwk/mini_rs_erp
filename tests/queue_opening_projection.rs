#![cfg(feature = "verification")]
//! Baseline fingerprints include every serialized queue control and read count.
//! Captured from f4286c7's unmodified evaluators, using the same library fixture.
use mini_rs_erp::core::{apparatus_standard::*, production_map::*};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Instant,
};

type Result<T> = std::result::Result<T, ProductionMapError>;
type States = BTreeMap<String, BTreeMap<String, String>>;

#[derive(Default)]
struct FixtureStore {
    maps: Vec<ProductionMapDefinition>,
    sessions: BTreeMap<String, Vec<OrderRunSession>>,
    opening: Vec<OpeningWipRecord>,
    states: States,
    calls: Mutex<BTreeMap<String, usize>>,
}
impl FixtureStore {
    fn count(&self, operation: &str, rows: usize) {
        let mut calls = self.calls.lock().unwrap();
        *calls.entry(operation.into()).or_default() += 1;
        *calls.entry(format!("{operation}_rows")).or_default() += rows;
    }
    fn data(&self) -> Value {
        json!({"maps":self.maps,"sessions":self.sessions,"opening":self.opening,"states":self.states})
    }
}
#[async_trait::async_trait]
impl ProductionMapStorePort for FixtureStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>> {
        self.count("maps", self.maps.len());
        Ok(self.maps.clone())
    }
    async fn put_map(&self, _: ProductionMapDefinition) -> Result<()> {
        panic!("projection wrote map")
    }
    async fn put_maps_batch(&self, _: &[ProductionMapDefinition]) -> Result<()> {
        panic!("projection wrote maps")
    }
    async fn delete_map(&self, _: &str) -> Result<()> {
        panic!("projection deleted map")
    }
    async fn apparatus_sequences(&self) -> Result<BTreeMap<String, Vec<String>>> {
        self.count("sequences", 0);
        Ok(BTreeMap::new())
    }
    async fn put_apparatus_sequence(&self, _: &str, _: Vec<String>) -> Result<()> {
        panic!("projection wrote sequence")
    }
    async fn apparatus_queue_states(&self) -> Result<States> {
        self.count("states", self.states.len());
        Ok(self.states.clone())
    }
    async fn put_apparatus_queue_states(&self, _: &str, _: BTreeMap<String, String>) -> Result<()> {
        panic!("projection wrote states")
    }
    async fn raw_material_assignments(&self) -> Result<Vec<RawMaterialAssignment>> {
        self.count("materials", 0);
        Ok(vec![])
    }
    async fn put_raw_material_assignment(&self, _: RawMaterialAssignment) -> Result<()> {
        panic!("projection wrote material")
    }
    async fn delete_raw_material_assignment(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Option<RawMaterialAssignment>> {
        panic!("projection deleted material")
    }
    async fn order_control_states(&self) -> Result<BTreeMap<String, OrderControlRecord>> {
        self.count("controls", 0);
        Ok(BTreeMap::new())
    }
    async fn order_run_sessions_for_orders(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<OrderRunSession>>> {
        self.count("sessions", self.sessions.values().map(Vec::len).sum());
        self.calls
            .lock()
            .unwrap()
            .insert("session_requested_order_ids".into(), order_ids.len());
        Ok(self.sessions.clone())
    }
    async fn progress_batches_for_orders(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<OrderProgressBatch>>> {
        self.count("batches", 0);
        self.calls
            .lock()
            .unwrap()
            .insert("batch_requested_order_ids".into(), order_ids.len());
        Ok(BTreeMap::new())
    }
    async fn opening_wip_records(&self, query: OpeningWipQuery) -> Result<Vec<OpeningWipRecord>> {
        assert_eq!(
            query,
            OpeningWipQuery {
                order_id: String::new(),
                wip_status: None,
                limit: 100_000
            }
        );
        self.count("opening", self.opening.len());
        Ok(self.opening.clone())
    }
    async fn active_print_preflight_holds(&self) -> Result<Vec<PrintPreflightHold>> {
        self.count("holds", 0);
        Ok(vec![])
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
fn resolver() -> Arc<Resolver> {
    Arc::new(Resolver(
        (0..6)
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
    ))
}
fn map(id: &str, operations: usize) -> ProductionMapDefinition {
    let mut nodes = vec![json!({"id":"start","kind":"start","title":"Start"})];
    let mut edges = vec![];
    let mut previous = "start".to_string();
    for i in 0..operations {
        let node = format!("node-{i}");
        nodes.push(json!({"id":node,"kind":"apparatus","title":format!("Machine {i}"),"apparatus_id":format!("apparatus:test:machine-{i}")}));
        edges.push(json!({"from":previous,"to":node}));
        previous = node;
    }
    nodes.push(json!({"id":"end","kind":"end","title":"End"}));
    edges.push(json!({"from":previous,"to":"end"}));
    serde_json::from_value(
        json!({"id":id,"title":"Order","product_code":"P","nodes":nodes,"edges":edges}),
    )
    .unwrap()
}
fn opening(order: &str, index: usize, operations: usize) -> OpeningWipRecord {
    let stage = index % operations.max(1);
    let apparatus = format!("apparatus:test:machine-{stage}");
    let status = ["waiting", "in_use", "processed", "void"][index % 4];
    serde_json::from_value(json!({"intake":{
        "intake_id":format!("intake-{index}"),"idempotency_key":format!("key-{index}"),"request_fingerprint":"fixture",
        "order_id":order,"entry_apparatus":apparatus,"source_operation":"fixture","source_apparatus":match index % 2 { 0 => "", _ => &apparatus },
        "current_location":"Fixture","resume_apparatus":apparatus,"resume_stage_node_id":format!("node-{stage}"),
        "history_status":"legacy","status":match index % 7 { 0 => "cancelled", _ => "confirmed" },
        "actor":{"role":"admin","ref_":"fixture","display_name":"Fixture"},"created_at_unix":index,"updated_at_unix":index},
        "batches":[{"batch_id":format!("batch-{index}"),"intake_id":format!("intake-{index}"),"order_id":order,"sequence_no":1,
        "qr_payload":format!("qr-{index}"),"quantity_basis":"unknown","wip_status":status,
        "label_item_code":"P","label_item_name":"Fixture","created_at_unix":index,"updated_at_unix":index}]})).unwrap()
}
fn fixture(
    maps: usize,
    operations: usize,
    history: usize,
    records: usize,
    raw: bool,
) -> FixtureStore {
    let mut store = FixtureStore::default();
    for i in 0..maps {
        let id = if raw {
            ["order", " order ", "", " ", "order"][i % 5].to_string()
        } else {
            format!("order-{i:04}")
        };
        let map = map(&id, operations);
        let sessions = (0..history).map(|s| OrderRunSession {
            session_id:format!("{id}-session-{s}"),apparatus:format!("apparatus:test:machine-{}",s%operations.max(1)),order_id:id.clone(),stage_node_id:format!("node-{}",s%operations.max(1)),
            status:OrderRunStatus::Completed,worker_role:"aparatchi".into(),worker_ref:"fixture".into(),worker_display_name:"Fixture".into(),started_at_unix:s as i64,updated_at_unix:s as i64,
            payload_json:json!({"stage_work_protocol":1,"stage_work_report":{"report_id":format!("report-{s}"),"sequence":s,"submitted_at_unix":s,"worker_ref":"fixture","worker_display_name":"Fixture"}}),
        }).collect();
        store.sessions.insert(id.clone(), sessions);
        for a in 0..operations {
            store
                .states
                .entry(format!("apparatus:test:machine-{a}"))
                .or_default()
                .insert(id.clone(), "completed".into());
        }
        store.maps.push(map);
    }
    for i in 0..records {
        let order = if raw {
            ["order", " order ", "", " ", "absent"][i % 5].to_string()
        } else if i % 4 == 0 && maps > 0 {
            format!("order-{:04}", i % maps)
        } else {
            format!("unrelated-{i}")
        };
        store.opening.push(opening(&order, i, operations));
    }
    store
}
fn scenarios() -> Vec<(String, usize, usize, usize, usize, bool)> {
    let mut rows = vec![];
    for maps in [0, 1, 285] {
        for records in [0, 100, 1000, 10000] {
            rows.push((
                format!("maps-{maps}-records-{records}"),
                maps,
                1,
                0,
                records,
                false,
            ));
        }
    }
    for operations in [1, 3, 6] {
        for history in [0, 1, 20, 100] {
            rows.push((
                format!("operations-{operations}-history-{history}"),
                20,
                operations,
                history,
                100,
                false,
            ));
        }
    }
    rows.push((
        "raw-ids-duplicates-source-order".into(),
        5,
        3,
        20,
        100,
        true,
    ));
    rows.push(("scoped-empty-10000".into(), 285, 1, 0, 10000, false));
    rows.push(("scoped-one-10000".into(), 285, 1, 0, 10000, false));
    rows
}
fn fingerprint(value: &Value) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(value).unwrap()))
}

async fn run_case(args: &(String, usize, usize, usize, usize, bool), timed: bool) -> Value {
    let (name, maps, operations, history, records, raw) = args;
    let mut fixture = fixture(*maps, *operations, *history, *records, *raw);
    if name.starts_with("scoped-") {
        for (index, map) in fixture.maps.iter_mut().enumerate() {
            if name.starts_with("scoped-empty") || index > 0 {
                map.nodes[1].apparatus_id = "apparatus:test:machine-1".into();
            }
        }
    }
    let store = Arc::new(fixture);
    let service = ProductionMapService::new(store.clone(), resolver());
    let before = store.data();
    let start = Instant::now();
    let result = if name.starts_with("scoped-") {
        verification_stage_work::scoped_queue_controls(&service, "apparatus:test:machine-0")
            .await
            .unwrap()
    } else {
        serde_json::to_value(service.queue_action_controls().await.unwrap()).unwrap()
    };
    let elapsed = start.elapsed().as_micros();
    assert_eq!(
        before,
        store.data(),
        "read projection changed ERP fixture data"
    );
    let result = json!({"controls":result,"calls":*store.calls.lock().unwrap()});
    if timed {
        println!("queue-elapsed name={name} elapsed_us={elapsed}");
    }
    result
}

#[tokio::test]
async fn complete_queue_controls_and_store_reads_match_original_library() {
    let expected: BTreeMap<String, String> =
        serde_json::from_str(include_str!("support/queue_projection_baseline.json")).unwrap();
    for args in scenarios() {
        assert_eq!(
            fingerprint(&run_case(&args, false).await),
            expected[&args.0],
            "case {}",
            args.0
        );
    }
}

#[tokio::test]
#[ignore = "baseline capture/timing harness; run against the baseline source explicitly"]
async fn capture_projection_baseline() {
    for args in scenarios() {
        let value = run_case(&args, true).await;
        println!("queue-baseline {} {}", args.0, fingerprint(&value));
    }
}
