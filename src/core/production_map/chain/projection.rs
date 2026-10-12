//! Read projection caches belong to one immutable map, never to a thread or
//! global revision. The borrow prevents edits until the projection is dropped.
use super::*;
use crate::core::production_map::{OrderProgressBatch, ProductionMapError, wip_route};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const MAX_ROUTE_CLASSES: usize = 256;
const MAX_ROUTE_KEY_BYTES: usize = 4096;
type RouteResult = Result<wip_route::WipInputRoute, ProductionMapError>;
type AdjacentCache = HashMap<(String, bool), Vec<ChainStage>>;

pub(crate) struct ProjectionMap<'a> {
    map: &'a ProductionMapDefinition,
    cached: bool,
    stages: OnceLock<Vec<ChainStage>>,
    adjacent: Mutex<AdjacentCache>,
    fingerprint: OnceLock<String>,
    routes: Mutex<HashMap<Vec<u8>, RouteResult>>,
}

impl<'a> ProjectionMap<'a> {
    pub(crate) fn new(map: &'a ProductionMapDefinition) -> Self {
        Self {
            map,
            cached: true,
            stages: OnceLock::new(),
            adjacent: Mutex::new(HashMap::new()),
            fingerprint: OnceLock::new(),
            routes: Mutex::new(HashMap::new()),
        }
    }

    /// Pure reference path used by write validation and differential tests.
    pub(crate) fn reference(map: &'a ProductionMapDefinition) -> Self {
        Self {
            cached: false,
            ..Self::new(map)
        }
    }

    pub(crate) fn map(&self) -> &'a ProductionMapDefinition {
        self.map
    }

    pub(crate) fn is_cached(&self) -> bool {
        self.cached
    }

    pub(crate) fn stages(&self) -> Vec<ChainStage> {
        if !self.cached {
            return linear_work_stages(self.map);
        }
        self.stages
            .get_or_init(|| linear_work_stages(self.map))
            .clone()
    }

    pub(crate) fn work_stage(&self, station: &str, node: &str) -> Option<ChainStage> {
        work_stage_for_station_with_stages(self.map, station, node, self.stages())
    }

    fn adjacent_stages(&self, node: &str, previous: bool) -> Vec<ChainStage> {
        let key = (node.trim().to_string(), previous);
        if self.cached {
            if let Some(value) = self.adjacent.lock().unwrap().get(&key) {
                return value.clone();
            }
        }
        let value =
            adjacent_physical_stages_for_node_with_stages(self.map, node, previous, self.stages());
        // Only graph node identities are memoized; arbitrary/stale input IDs
        // cannot grow this cache beyond two entries per immutable map node.
        if self.cached && self.map.nodes.iter().any(|n| n.id.trim() == node.trim()) {
            self.adjacent.lock().unwrap().insert(key, value.clone());
        }
        value
    }

    pub(crate) fn previous_stages(&self, node: &str) -> Vec<ChainStage> {
        self.adjacent_stages(node, true)
    }

    pub(crate) fn next_stages(&self, node: &str) -> Vec<ChainStage> {
        self.adjacent_stages(node, false)
    }

    pub(crate) fn previous_stations(&self, station: &str) -> Vec<String> {
        previous_work_stage_stations_with_stages(self.map, station, self.stages())
    }

    #[cfg(test)]
    pub(crate) fn next_stations(&self, station: &str) -> Vec<String> {
        next_work_stage_stations_with_stages(self.map, station, self.stages())
    }

    pub(crate) fn previous_unavailable(&self, station: &str) -> bool {
        previous_stage_resolution_is_unavailable_with_stages(self.map, station, self.stages())
    }

    pub(crate) fn ready_for_stage(
        &self,
        order_id: &str,
        node: &str,
        states: &BTreeMap<String, BTreeMap<String, String>>,
    ) -> bool {
        self.previous_stages(node)
            .into_iter()
            .next()
            .and_then(|s| s.apparatus_id)
            .is_none_or(|id| {
                queue_state_for_station(&id, order_id, states)
                    == ApparatusQueueOrderState::Completed
            })
    }

    pub(crate) fn fingerprint(&self) -> String {
        if !self.cached {
            return wip_route::production_map_fingerprint(self.map);
        }
        self.fingerprint
            .get_or_init(|| wip_route::production_map_fingerprint(self.map))
            .clone()
    }

    pub(crate) fn route(&self, batch: &OrderProgressBatch) -> RouteResult {
        if !self.cached {
            return wip_route::resolve_wip_input_route_projected(self, batch);
        }
        let Some(key) = route_class(batch) else {
            return wip_route::resolve_wip_input_route_projected(self, batch);
        };
        if let Some(value) = self.routes.lock().unwrap().get(&key) {
            return value.clone();
        }
        let value = wip_route::resolve_wip_input_route_projected(self, batch);
        let mut routes = self.routes.lock().unwrap();
        if routes.len() < MAX_ROUTE_CLASSES {
            routes.insert(key, value.clone());
        }
        value
    }
}

struct RouteKeyWriter(Vec<u8>);

impl std::io::Write for RouteKeyWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_ROUTE_KEY_BYTES - self.0.len() {
            return Err(std::io::Error::other("route identity exceeds cache limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn route_class(batch: &OrderProgressBatch) -> Option<Vec<u8>> {
    let field = |key| {
        batch
            .payload_json
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
    };
    // Every field read by the resolver/actionable_wip participates. Session IDs
    // are emptiness witnesses; quantity, QR, worker and timestamps are historical.
    let mut writer = RouteKeyWriter(Vec::new());
    // Serialize borrowed fields directly, stopping before an oversized binding
    // could allocate an unbounded cache key or clone its arbitrary JSON payload.
    serde_json::to_writer(
        &mut writer,
        &(
            batch.order_id.trim(),
            batch.action.records_progress_output(),
            &batch.wip_status,
            batch.apparatus.trim(),
            batch.next_apparatus.trim(),
            batch.current_apparatus.trim(),
            batch.used_by_apparatus.trim(),
            !batch.used_by_session_id.trim().is_empty(),
            batch.processed_by_apparatus.trim(),
            !batch.processed_by_session_id.trim().is_empty(),
            field("stage_node_id"),
            field("next_stage_node_id"),
            batch.payload_json.get("wip_route_binding").is_some(),
            batch.payload_json.get("wip_route_binding"),
        ),
    )
    .ok()?;
    Some(writer.0)
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
