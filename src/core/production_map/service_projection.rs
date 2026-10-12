use super::*;

impl ProductionMapService {
    pub fn new(
        store: std::sync::Arc<dyn ProductionMapStorePort>,
        apparatus_resolver: std::sync::Arc<dyn CanonicalApparatusResolver>,
    ) -> Self {
        let (live_notify, _) = broadcast::channel(LIVE_NOTIFY_CAPACITY);
        Self {
            store,
            apparatus_resolver,
            live_notify,
            queue_action_locks: std::sync::Arc::new(ProductionMutationLocks::default()),
            snapshot_cache: std::sync::Arc::new(ProductionSnapshotCache::default()),
            #[cfg(any(test, feature = "projection-research"))]
            reference_projection: false,
        }
    }

    /// Benchmark/test-only pure reference projection. Select before sharing a
    /// service; each service owns an independent snapshot cache.
    #[cfg(any(test, feature = "projection-research"))]
    pub fn with_reference_projection(mut self) -> Self {
        self.reference_projection = true;
        self
    }

    pub(crate) fn projection_map<'a>(
        &self,
        map: &'a ProductionMapDefinition,
    ) -> chain::ProjectionMap<'a> {
        #[cfg(any(test, feature = "projection-research"))]
        if self.reference_projection {
            return chain::ProjectionMap::reference(map);
        }
        chain::ProjectionMap::new(map)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(store: std::sync::Arc<dyn ProductionMapStorePort>) -> Self {
        Self::new(
            store,
            std::sync::Arc::new(super::TestCanonicalApparatusResolver::standard()),
        )
    }
}
