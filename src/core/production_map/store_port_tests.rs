use async_trait::async_trait;

use super::*;

#[test]
fn snapshot_logs_preserve_latest_stage_states_and_freezes_in_tie_order() {
    let entry = |id: &str, stage: &str, action: &str, state: &str| {
        serde_json::from_value::<ProductionOrderLogEntry>(serde_json::json!({
            "event_id":id,"apparatus":"apparatus:default:bosma_7","order_id":"order",
            "stage_node_id":stage,"action":action,"from_state":"in_progress","to_state":state,
            "actor_role":"aparatchi","actor_ref":id,"actor_display_name":id,
            "created_at_unix":100,"completed_with_issue":false,"issue_note":format!("note-{id}")
        })).unwrap()
    };
    let history = vec![
        entry("start", "a", "start", "in_progress"),
        entry("old-freeze", "a", "freeze", "frozen"),
        entry("resume", "a", "resume", "in_progress"),
        entry("new-freeze", "a", "freeze", "frozen"),
        entry("latest-a", "a", "resume", "in_progress"),
        entry("freeze-b", "b", "freeze", "frozen"),
        entry("latest-b", "b", "pause", "paused"),
        entry("global-freeze", "", "freeze", "frozen"),
        entry("latest-legacy", "", "resume", "in_progress"),
    ];
    let compact = queue_snapshot_logs_from_history(BTreeMap::from([("order".into(),history.clone())]));
    assert_eq!(compact["order"], history[3..]);
    for stage in ["a", "b", ""] {
        let latest = |logs: &[ProductionOrderLogEntry]| logs.iter().rev()
            .find(|log| log.stage_node_id == stage).cloned();
        let freeze = |logs: &[ProductionOrderLogEntry]| logs.iter().rev()
            .find(|log| log.stage_node_id == stage && log.action == super::super::queue_state::ApparatusQueueAction::Freeze
                && log.to_state == super::super::queue_state::ApparatusQueueOrderState::Frozen).cloned();
        assert_eq!(latest(&compact["order"]), latest(&history));
        assert_eq!(freeze(&compact["order"]), freeze(&history));
    }
    assert_eq!(compact["order"][0].issue_note,"note-new-freeze");
    assert_eq!(compact["order"][0].actor_ref,"new-freeze");
}

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
