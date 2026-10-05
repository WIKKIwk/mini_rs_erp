use super::*;

#[cfg(test)]
#[path = "service_worker_snapshot_tests.rs"]
mod tests;

const MAX_WORKER_SNAPSHOTS: usize = 64;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct WorkerSnapshotScope {
    pub apparatus: Vec<String>,
    history: Vec<String>,
}

pub(super) struct WorkerSnapshotInputs {
    pub maps: Vec<ProductionMapDefinition>,
    pub sequences: (BTreeMap<String, Vec<String>>, BTreeMap<String, i64>),
    pub queue_states: ApparatusQueueStateMap,
    pub order_controls: BTreeMap<String, OrderControlRecord>,
    pub order_ids: BTreeSet<String>,
    pub apparatus_ids: BTreeSet<String>,
}

fn normalized(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

impl ProductionMapService {
    /// Same authoritative revision as the full view, built only for worker
    /// orders and the queue context needed to preserve their route controls.
    /// Cached results are usable only at the exact current revision; no TTL.
    pub async fn worker_snapshot_shared_with_revision(
        &self,
        apparatus: &[String],
        history_order_ids: &[String],
    ) -> Result<(std::sync::Arc<ProductionMapLiveSnapshot>, u64), ProductionMapError> {
        let scope = WorkerSnapshotScope {
            apparatus: normalized(apparatus),
            history: normalized(history_order_ids),
        };
        if scope.apparatus.is_empty()
            || scope
                .apparatus
                .iter()
                .any(|id| !queue_state::is_canonical_apparatus_id(id))
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let entry = {
            let mut entries = self.snapshot_cache.worker_snapshots.lock().await;
            if let Some(entry) = entries.get(&scope) {
                entry.clone()
            } else {
                if entries.len() >= MAX_WORKER_SNAPSHOTS {
                    let removable = entries
                        .iter()
                        .find(|(_, entry)| std::sync::Arc::strong_count(entry) == 1)
                        .map(|(key, _)| key.clone());
                    if let Some(key) = removable {
                        entries.remove(&key);
                    }
                }
                let entry = std::sync::Arc::new(Mutex::new(None::<CachedProductionSnapshot>));
                if entries.len() < MAX_WORKER_SNAPSHOTS {
                    entries.insert(scope.clone(), entry.clone());
                }
                entry
            }
        };
        let mut cached = entry.lock().await;
        loop {
            let revision = self.snapshot_revision();
            if let Some(entry) = cached.as_ref().filter(|entry| entry.revision == revision) {
                return Ok((entry.snapshot.clone(), revision));
            }
            // An administrator may already have built this exact revision.
            // Reuse that authority rather than repeating even a scoped build.
            if let Some(global) = self
                .snapshot_cache
                .snapshot
                .read()
                .await
                .as_ref()
                .filter(|entry| entry.revision == revision)
            {
                return Ok((global.snapshot.clone(), revision));
            }
            let snapshot = self
                .build_production_snapshot_in_scope(
                    Some(&scope),
                    cached.as_ref().map(|entry| entry.snapshot.as_ref()),
                )
                .await?;
            if self.snapshot_revision() != revision {
                continue;
            }
            let snapshot = std::sync::Arc::new(snapshot);
            *cached = Some(CachedProductionSnapshot {
                revision,
                snapshot: snapshot.clone(),
            });
            return Ok((snapshot, revision));
        }
    }

    pub(super) async fn worker_snapshot_inputs(
        &self,
        scope: &WorkerSnapshotScope,
    ) -> Result<WorkerSnapshotInputs, ProductionMapError> {
        // These are small shared queue/control records, not every map's graph
        // or WIP. Other orders' active states must still block a busy machine.
        let (sequences, queue_states, order_controls) = tokio::try_join!(
            self.store.apparatus_sequences_with_revisions(),
            self.store.apparatus_queue_states(),
            self.store.order_control_states(),
        )?;
        let mut extra = scope.history.iter().cloned().collect::<BTreeSet<_>>();
        for apparatus in &scope.apparatus {
            extra.extend(sequences.0.get(apparatus).into_iter().flatten().cloned());
        }
        // Legacy frozen records identify their apparatus through the last
        // freeze log. Include those few orders until the common builder resolves it.
        extra.extend(
            order_controls
                .iter()
                .filter(|(_, control)| control.state == OrderControlState::Frozen)
                .map(|(id, _)| id.clone()),
        );
        let candidates = self
            .store
            .maps_for_snapshot_scope(&scope.apparatus, &extra.iter().cloned().collect::<Vec<_>>())
            .await?;
        let canonical = self.snapshot_canonical_apparatuses().await;
        let mut queue_orders = apparatus::queue_order_ids_by_apparatus(&candidates);
        apparatus::filter_unselected_print_orders(&candidates, &canonical, &mut queue_orders);
        let mut order_ids = extra;
        for id in &scope.apparatus {
            order_ids.extend(queue_orders.get(id).into_iter().flatten().cloned());
        }
        let mut apparatus_ids = scope.apparatus.iter().cloned().collect::<BTreeSet<_>>();
        for map in candidates.iter().filter(|map| order_ids.contains(&map.id)) {
            apparatus_ids.extend(
                chain::linear_work_stages(map)
                    .into_iter()
                    .filter_map(|stage| stage.apparatus_id),
            );
        }
        // Retain competing queue orders for these stations. Strict queue
        // priority and roll-detached/re-entry blockers use the existing rules.
        // Their expensive controls/stage status and programs are not built.
        let maps = self
            .store
            .maps_for_snapshot_scope(
                &apparatus_ids.iter().cloned().collect::<Vec<_>>(),
                &order_ids.iter().cloned().collect::<Vec<_>>(),
            )
            .await?;
        Ok(WorkerSnapshotInputs {
            maps,
            sequences,
            queue_states,
            order_controls,
            order_ids,
            apparatus_ids,
        })
    }
}
