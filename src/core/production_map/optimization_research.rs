//! Research only: memoization over a single immutable synchronous evaluation.
//! Pointer keys are accepted only for input objects borrowed for the scope's
//! complete lifetime. No cached result survives a scope or crosses an await.
//! A shipping implementation should pass an explicit projection context.
use super::chain::ChainStage;
use super::wip_route::WipInputRoute;
use super::{OrderProgressBatch, ProductionMapDefinition, ProductionMapError};
use serde::Serialize;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    marker::PhantomData,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::ThreadId,
};

static MODE: AtomicUsize = AtomicUsize::new(0);
static RECORDING: AtomicBool = AtomicBool::new(false);
static COMPLETED: Mutex<Vec<Summary>> = Mutex::new(Vec::new());
thread_local! { static ACTIVE: RefCell<Option<State>> = const { RefCell::new(None) }; }

#[derive(Default, Serialize)]
pub struct Counter {
    calls: u64,
    hits: u64,
}
#[derive(Default, Serialize)]
pub struct Summary {
    scope: String,
    mode: usize,
    linear: Counter,
    adjacent: Counter,
    route: Counter,
    fingerprint: Counter,
    cached_maps: usize,
    cached_adjacencies: usize,
    cached_routes: usize,
    cached_fingerprints: usize,
    input_route_classes: usize,
    helper: Counter,
    estimated_owned_cache_bytes: usize,
}

struct State {
    mode: usize,
    allowed_maps: HashSet<usize>,
    allowed_batches: HashSet<usize>,
    stages: HashMap<usize, Vec<ChainStage>>,
    adjacent: HashMap<(usize, String, bool), Vec<ChainStage>>,
    routes: HashMap<(usize, usize), Result<WipInputRoute, ProductionMapError>>,
    fingerprints: HashMap<usize, String>,
    batch_classes: HashMap<usize, usize>,
    stage_lookup: HashMap<(usize, String, String), Option<ChainStage>>,
    station_neighbors: HashMap<(usize, String, bool), Vec<String>>,
    unavailable: HashMap<(usize, String), bool>,
    summary: Summary,
}

pub fn set_mode(mode: usize) {
    assert!(mode <= 7);
    MODE.store(mode, Ordering::Relaxed);
    RECORDING.store(true, Ordering::Release);
}
pub(crate) fn current_mode() -> usize {
    MODE.load(Ordering::Relaxed)
}
pub(crate) fn skip_terminal_input_route() -> bool {
    ACTIVE.with(|active| {
        active
            .borrow()
            .as_ref()
            .is_some_and(|state| state.mode >= 3)
    })
}
pub fn take_summaries() -> Vec<Summary> {
    std::mem::take(&mut *COMPLETED.lock().unwrap())
}

pub struct Scope<'a> {
    active: bool,
    owner: ThreadId,
    _inputs: PhantomData<(&'a [ProductionMapDefinition], &'a OrderProgressBatch)>,
}

pub fn begin<'a>(
    maps: &'a [ProductionMapDefinition],
    batches: &'a BTreeMap<String, Vec<OrderProgressBatch>>,
) -> Scope<'a> {
    begin_labeled(maps, batches, "queue_controls")
}
pub(crate) fn begin_labeled<'a>(
    maps: &'a [ProductionMapDefinition],
    batches: &'a BTreeMap<String, Vec<OrderProgressBatch>>,
    label: &str,
) -> Scope<'a> {
    if !RECORDING.load(Ordering::Acquire) {
        return Scope {
            active: false,
            owner: std::thread::current().id(),
            _inputs: PhantomData,
        };
    }
    let mode = MODE.load(Ordering::Relaxed);
    let mut classes = HashMap::<Vec<u8>, usize>::new();
    let mut batch_classes = HashMap::new();
    if mode >= 4 {
        for b in batches.values().flatten() {
            // Include every batch field read by resolve_wip_input_route and
            // actionable_wip. IDs used only as emptiness witnesses may share a class.
            let field = |key| {
                b.payload_json
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .trim()
            };
            let key = serde_json::to_vec(&serde_json::json!([
                b.order_id.trim(),
                b.action.records_progress_output(),
                b.wip_status,
                b.apparatus.trim(),
                b.next_apparatus.trim(),
                b.current_apparatus.trim(),
                b.used_by_apparatus.trim(),
                !b.used_by_session_id.trim().is_empty(),
                b.processed_by_apparatus.trim(),
                !b.processed_by_session_id.trim().is_empty(),
                field("stage_node_id"),
                field("next_stage_node_id"),
                b.payload_json.get("wip_route_binding").is_some(),
                b.payload_json.get("wip_route_binding")
            ]))
            .expect("serializable route class");
            let next = classes.len();
            let class = *classes.entry(key).or_insert(next);
            batch_classes.insert(b as *const _ as usize, class);
        }
    }
    let input_route_classes = classes.len();
    drop(classes);
    let state = State {
        mode,
        allowed_maps: maps.iter().map(|m| m as *const _ as usize).collect(),
        allowed_batches: batches
            .values()
            .flatten()
            .map(|b| b as *const _ as usize)
            .collect(),
        stages: HashMap::new(),
        adjacent: HashMap::new(),
        routes: HashMap::new(),
        fingerprints: HashMap::new(),
        batch_classes,
        stage_lookup: HashMap::new(),
        station_neighbors: HashMap::new(),
        unavailable: HashMap::new(),
        summary: Summary {
            mode,
            scope: label.to_string(),
            input_route_classes,
            ..Default::default()
        },
    };
    ACTIVE.with(|active| {
        assert!(active.borrow().is_none());
        *active.borrow_mut() = Some(state);
    });
    Scope {
        active: true,
        owner: std::thread::current().id(),
        _inputs: PhantomData,
    }
}

fn stage_bytes(stages: &[ChainStage]) -> usize {
    stages.len() * std::mem::size_of::<ChainStage>()
        + stages
            .iter()
            .map(|s| {
                s.node_id.capacity()
                    + s.station_title.capacity()
                    + s.apparatus_id.as_ref().map_or(0, String::capacity)
            })
            .sum::<usize>()
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        assert_eq!(
            self.owner,
            std::thread::current().id(),
            "research scope crossed threads"
        );
        let mut state = ACTIVE
            .with(|active| active.borrow_mut().take())
            .expect("active scope");
        state.summary.cached_maps = state.stages.len();
        state.summary.cached_adjacencies = state.adjacent.len();
        state.summary.cached_routes = state.routes.len();
        state.summary.cached_fingerprints = state.fingerprints.len();
        // Payload and bucket estimate; the harness separately measures live Rust allocations.
        state.summary.estimated_owned_cache_bytes = state.stage_lookup.capacity()
            * std::mem::size_of::<((usize, String, String), Option<ChainStage>)>()
            + state
                .stage_lookup
                .iter()
                .map(|((_, station, node), stage)| {
                    station.capacity()
                        + node.capacity()
                        + stage
                            .as_ref()
                            .map_or(0, |value| stage_bytes(std::slice::from_ref(value)))
                })
                .sum::<usize>()
            + state.station_neighbors.capacity()
                * std::mem::size_of::<((usize, String, bool), Vec<String>)>()
            + state
                .station_neighbors
                .iter()
                .map(|((_, station, _), values)| {
                    station.capacity()
                        + values.capacity() * std::mem::size_of::<String>()
                        + values.iter().map(String::capacity).sum::<usize>()
                })
                .sum::<usize>()
            + state.unavailable.capacity() * std::mem::size_of::<((usize, String), bool)>()
            + state
                .unavailable
                .keys()
                .map(|(_, station)| station.capacity())
                .sum::<usize>()
            + state.allowed_maps.capacity() * std::mem::size_of::<usize>()
            + state.allowed_batches.capacity() * std::mem::size_of::<usize>()
            + state.batch_classes.capacity() * std::mem::size_of::<(usize, usize)>()
            + state.stages.capacity() * std::mem::size_of::<(usize, Vec<ChainStage>)>()
            + state.stages.values().map(|v| stage_bytes(v)).sum::<usize>()
            + state.adjacent.capacity()
                * std::mem::size_of::<((usize, String, bool), Vec<ChainStage>)>()
            + state
                .adjacent
                .iter()
                .map(|((_, id, _), stages)| id.capacity() + stage_bytes(stages))
                .sum::<usize>()
            + state.routes.capacity()
                * std::mem::size_of::<((usize, usize), Result<WipInputRoute, ProductionMapError>)>(
                )
            + state
                .routes
                .values()
                .filter_map(|r| r.as_ref().ok())
                .map(|r| {
                    r.source_stage_node_id.capacity()
                        + r.stage_node_id.capacity()
                        + r.map_fingerprint.capacity()
                        + r.consumer_apparatus_ids.capacity() * std::mem::size_of::<String>()
                        + r.consumer_apparatus_ids
                            .iter()
                            .map(String::capacity)
                            .sum::<usize>()
                })
                .sum::<usize>()
            + state.fingerprints.capacity() * std::mem::size_of::<(usize, String)>()
            + state
                .fingerprints
                .values()
                .map(String::capacity)
                .sum::<usize>();
        COMPLETED.lock().unwrap().push(state.summary);
    }
}

pub fn get_stages(map: &ProductionMapDefinition) -> Option<Vec<ChainStage>> {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let s = active.as_mut()?;
        s.summary.linear.calls += 1;
        if s.mode == 0 {
            return None;
        }
        let value = s.stages.get(&(map as *const _ as usize)).cloned();
        if value.is_some() {
            s.summary.linear.hits += 1;
        }
        value
    })
}
pub fn put_stages(map: &ProductionMapDefinition, value: &[ChainStage]) {
    ACTIVE.with(|active| {
        if let Some(s) = active.borrow_mut().as_mut() {
            let key = map as *const _ as usize;
            if s.mode > 0 && s.allowed_maps.contains(&key) {
                s.stages.insert(key, value.to_vec());
            }
        }
    });
}

pub fn get_adjacent(
    map: &ProductionMapDefinition,
    id: &str,
    previous: bool,
) -> Option<Vec<ChainStage>> {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let s = active.as_mut()?;
        s.summary.adjacent.calls += 1;
        if s.mode == 0 {
            return None;
        }
        let value = s
            .adjacent
            .get(&(map as *const _ as usize, id.to_string(), previous))
            .cloned();
        if value.is_some() {
            s.summary.adjacent.hits += 1;
        }
        value
    })
}
pub fn put_adjacent(map: &ProductionMapDefinition, id: &str, previous: bool, value: &[ChainStage]) {
    ACTIVE.with(|active| {
        if let Some(s) = active.borrow_mut().as_mut() {
            let key = map as *const _ as usize;
            if s.mode > 0 && s.allowed_maps.contains(&key) {
                s.adjacent
                    .insert((key, id.to_string(), previous), value.to_vec());
            }
        }
    });
}

pub fn get_route(
    map: &ProductionMapDefinition,
    batch: &OrderProgressBatch,
) -> Option<Result<WipInputRoute, ProductionMapError>> {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let s = active.as_mut()?;
        s.summary.route.calls += 1;
        if s.mode < 2 {
            return None;
        }
        let b = batch as *const _ as usize;
        let key = if s.mode >= 4 {
            *s.batch_classes.get(&b)?
        } else {
            b
        };
        let value = s.routes.get(&(map as *const _ as usize, key)).cloned();
        if value.is_some() {
            s.summary.route.hits += 1;
        }
        value
    })
}
pub fn put_route(
    map: &ProductionMapDefinition,
    batch: &OrderProgressBatch,
    value: &Result<WipInputRoute, ProductionMapError>,
) {
    ACTIVE.with(|active| {
        if let Some(s) = active.borrow_mut().as_mut() {
            let m = map as *const _ as usize;
            let b = batch as *const _ as usize;
            if s.mode >= 2 && s.allowed_maps.contains(&m) && s.allowed_batches.contains(&b) {
                let key = if s.mode >= 4 {
                    *s.batch_classes.get(&b).expect("allowed batch class")
                } else {
                    b
                };
                s.routes.insert((m, key), value.clone());
            }
        }
    });
}

pub fn get_fingerprint(map: &ProductionMapDefinition) -> Option<String> {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let s = active.as_mut()?;
        s.summary.fingerprint.calls += 1;
        if s.mode == 0 {
            return None;
        }
        let value = s.fingerprints.get(&(map as *const _ as usize)).cloned();
        if value.is_some() {
            s.summary.fingerprint.hits += 1;
        }
        value
    })
}
pub fn put_fingerprint(map: &ProductionMapDefinition, value: &str) {
    ACTIVE.with(|active| {
        if let Some(s) = active.borrow_mut().as_mut() {
            let key = map as *const _ as usize;
            if s.mode > 0 && s.allowed_maps.contains(&key) {
                s.fingerprints.insert(key, value.to_string());
            }
        }
    });
}

pub fn get_stage_lookup(
    map: &ProductionMapDefinition,
    station: &str,
    node: &str,
) -> Option<Option<ChainStage>> {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let s = active.as_mut()?;
        s.summary.helper.calls += 1;
        if !matches!(s.mode, 5 | 6) {
            return None;
        }
        let value = s
            .stage_lookup
            .get(&(
                map as *const _ as usize,
                station.to_string(),
                node.to_string(),
            ))
            .cloned();
        if value.is_some() {
            s.summary.helper.hits += 1;
        }
        value
    })
}
pub fn put_stage_lookup(
    map: &ProductionMapDefinition,
    station: &str,
    node: &str,
    value: &Option<ChainStage>,
) {
    ACTIVE.with(|active| {
        if let Some(s) = active.borrow_mut().as_mut() {
            let m = map as *const _ as usize;
            if matches!(s.mode, 5 | 6) && s.allowed_maps.contains(&m) {
                s.stage_lookup
                    .insert((m, station.to_string(), node.to_string()), value.clone());
            }
        }
    });
}
pub fn get_station_neighbors(
    map: &ProductionMapDefinition,
    station: &str,
    previous: bool,
) -> Option<Vec<String>> {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let s = active.as_mut()?;
        s.summary.helper.calls += 1;
        if !matches!(s.mode, 5 | 6) {
            return None;
        }
        let value = s
            .station_neighbors
            .get(&(map as *const _ as usize, station.to_string(), previous))
            .cloned();
        if value.is_some() {
            s.summary.helper.hits += 1;
        }
        value
    })
}
pub fn put_station_neighbors(
    map: &ProductionMapDefinition,
    station: &str,
    previous: bool,
    value: &[String],
) {
    ACTIVE.with(|active| {
        if let Some(s) = active.borrow_mut().as_mut() {
            let m = map as *const _ as usize;
            if matches!(s.mode, 5 | 6) && s.allowed_maps.contains(&m) {
                s.station_neighbors
                    .insert((m, station.to_string(), previous), value.to_vec());
            }
        }
    });
}
pub fn get_unavailable(map: &ProductionMapDefinition, station: &str) -> Option<bool> {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let s = active.as_mut()?;
        s.summary.helper.calls += 1;
        if !matches!(s.mode, 5 | 6) {
            return None;
        }
        let value = s
            .unavailable
            .get(&(map as *const _ as usize, station.to_string()))
            .copied();
        if value.is_some() {
            s.summary.helper.hits += 1;
        }
        value
    })
}
pub fn put_unavailable(map: &ProductionMapDefinition, station: &str, value: bool) {
    ACTIVE.with(|active| {
        if let Some(s) = active.borrow_mut().as_mut() {
            let m = map as *const _ as usize;
            if matches!(s.mode, 5 | 6) && s.allowed_maps.contains(&m) {
                s.unavailable.insert((m, station.to_string()), value);
            }
        }
    });
}
