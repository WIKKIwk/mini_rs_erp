use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::core::production_map::{
    ApparatusMaterialRule, ApparatusQueueActionEvent, ApparatusQueuePolicy,
    ProductionMapDefinition, ProductionMapError, ProductionMapStorePort, QueueActionActor,
    RawMaterialAssignment,
};

pub(super) struct UnavailableProductionMapStore;

#[async_trait]
impl ProductionMapStorePort for UnavailableProductionMapStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_map(&self, _map: ProductionMapDefinition) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_maps_batch(
        &self,
        _maps: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn delete_map(&self, _map_id: &str) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_apparatus_sequence(
        &self,
        _apparatus: &str,
        _order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_apparatus_queue_states(
        &self,
        _apparatus: &str,
        _states: BTreeMap<String, String>,
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn apparatus_queue_policies(
        &self,
    ) -> Result<BTreeMap<String, ApparatusQueuePolicy>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_apparatus_queue_policy(
        &self,
        _apparatus: &str,
        _policy: ApparatusQueuePolicy,
        _actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn append_apparatus_queue_action_event(
        &self,
        _event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn apparatus_material_rules(
        &self,
    ) -> Result<Vec<ApparatusMaterialRule>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_apparatus_material_rule(
        &self,
        _rule: ApparatusMaterialRule,
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_raw_material_assignment(
        &self,
        _assignment: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn delete_raw_material_assignment(
        &self,
        _order_id: &str,
        _barcode: &str,
    ) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }
}
