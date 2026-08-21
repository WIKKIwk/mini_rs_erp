
#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;

    struct DefaultDowntimeStore;

    #[async_trait]
    impl ProductionMapStorePort for DefaultDowntimeStore {
        async fn maps(&self) -> StoreResult<Vec<ProductionMapDefinition>> {
            unimplemented!()
        }

        async fn put_map(&self, _map: ProductionMapDefinition) -> StoreResult<()> {
            unimplemented!()
        }

        async fn put_maps_batch(&self, _maps: &[ProductionMapDefinition]) -> StoreResult<()> {
            unimplemented!()
        }

        async fn delete_map(&self, _map_id: &str) -> StoreResult<()> {
            unimplemented!()
        }

        async fn apparatus_sequences(&self) -> StoreResult<ApparatusSequenceMap> {
            unimplemented!()
        }

        async fn put_apparatus_sequence(
            &self,
            _apparatus: &str,
            _order_ids: Vec<String>,
        ) -> StoreResult<()> {
            unimplemented!()
        }

        async fn apparatus_queue_states(&self) -> StoreResult<ApparatusQueueStateMap> {
            unimplemented!()
        }

        async fn put_apparatus_queue_states(
            &self,
            _apparatus: &str,
            _states: QueueStateMap,
        ) -> StoreResult<()> {
            unimplemented!()
        }

        async fn raw_material_assignments(&self) -> StoreResult<Vec<RawMaterialAssignment>> {
            unimplemented!()
        }

        async fn put_raw_material_assignment(
            &self,
            _assignment: RawMaterialAssignment,
        ) -> StoreResult<()> {
            unimplemented!()
        }

        async fn delete_raw_material_assignment(
            &self,
            _order_id: &str,
            _barcode: &str,
        ) -> StoreResult<Option<RawMaterialAssignment>> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn default_downtime_write_fails_closed() {
        let downtime = ApparatusDowntime {
            id: "downtime-1".to_string(),
            apparatus_id: ApparatusId::new("apparatus:catalog:flexo-001").unwrap(),
            apparatus: "Flexo pechat".to_string(),
            starts_at_unix: 1,
            ends_at_unix: 2,
            reason: "maintenance".to_string(),
            active: true,
            actor: QueueActionActor::default(),
            created_at_unix: 1,
        };

        assert_eq!(
            DefaultDowntimeStore.put_apparatus_downtime(downtime).await,
            Err(ProductionMapError::StoreFailed)
        );
    }
}
