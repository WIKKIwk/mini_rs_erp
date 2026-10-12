#![cfg(feature = "verification")]

use mini_rs_erp::core::{apparatus_standard::*, production_map::*};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

const PRESS: &str = "apparatus:default:bosma_9";
const LAM: &str = "apparatus:default:asset-007";
const ORDER: &str = "zakaz-layer-remainder-order";

fn roll(id: &str, origin: &str) -> OrderProgressBatch {
    serde_json::from_value(json!({
        "batch_id":id,"session_id":"press-session","started_at_unix":1,"completed_at_unix":2,
        "apparatus":PRESS,"order_id":ORDER,"action":"complete","status":"completed",
        "produced_qty":if origin.is_empty() {1500.0} else {600.0},"uom":"m","qr_payload":format!("qr-{id}"),
        "label_item_code":"FILM","label_item_name":"Film","executor_name":"Printer",
        "worker_role":"aparatchi","worker_ref":"printer","worker_display_name":"Printer",
        "wip_status":if origin.is_empty() {"processed"} else {"waiting"},
        "processed_by_apparatus":if origin.is_empty() {LAM} else {""},
        "processed_by_session_id":if origin.is_empty() {"lam-session"} else {""},
        "next_apparatus":LAM,"current_apparatus":LAM,"current_location":"Laminatsiya",
        "parent_batch_id":if origin.is_empty() {""} else {"original"},
        "finished_goods_kg":if origin.is_empty() {50.0} else {20.0},
        "finished_goods_meter":if origin.is_empty() {1500.0} else {600.0},
        "payload_json":{"origin":origin,"stage_node_id":"press","next_stage_node_id":"lam",
            "layer_remainder_operation_id":"return-1","source_input_links":[]}
    })).unwrap()
}

fn session(apparatus: &str, node: &str, id: &str) -> OrderRunSession {
    serde_json::from_value(json!({
        "session_id":id,"apparatus":apparatus,"order_id":ORDER,"stage_node_id":node,
        "status":"completed","worker_role":"aparatchi","worker_ref":if apparatus==PRESS {"printer"} else {"laminator"},
        "worker_display_name":"Worker","started_at_unix":1,"updated_at_unix":2,
        "payload_json":{"input_progress_batch_id":"","input_lineage":[],"stage_work_protocol":1,
            "stage_work_report":{"report_id":format!("report-{id}"),"sequence":1,
                "submitted_at_unix":2,"worker_ref":"worker","worker_display_name":"Worker"}}
    })).unwrap()
}

struct Resolver(Vec<Arc<RuntimeApparatusConfiguration>>);

#[async_trait::async_trait]
impl CanonicalApparatusResolver for Resolver {
    async fn resolve(
        &self,
        id: &ApparatusId,
    ) -> Result<Option<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        Ok(self
            .0
            .iter()
            .find(|c| &c.runtime.apparatus_id == id)
            .cloned())
    }
    async fn list(&self) -> Result<Vec<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        Ok(self.0.clone())
    }
}

fn resolver() -> Arc<Resolver> {
    Arc::new(Resolver(
        [PRESS, LAM]
            .into_iter()
            .map(|id| {
                let mut revision = mini_rs_apparatus_contract::isa95::test_support::revision_with(
                    id,
                    &format!(
                        "physical-asset:{}",
                        if id == PRESS { "press" } else { "lam" }
                    ),
                    "Fixture",
                );
                if id == LAM {
                    revision.execution_profile.operation = ExecutionOperation::Laminate;
                    revision.execution_profile.technology = ProcessTechnology::AdhesiveLamination;
                    revision.execution_profile.color_station_count = None;
                    revision.capabilities = vec![EquipmentCapability {
                        code: EquipmentCapabilityCode::Laminate,
                        level: 1,
                    }];
                    revision.training.enabled = false;
                    revision.training.queue_enabled = false;
                    revision.training.material_tracking_enabled = false;
                }
                revision.policies.queue = QueueDiscipline::FreePick;
                revision.policies.material = MaterialExecutionPolicy::NotRequired {
                    item_group_ids: vec![],
                };
                revision.policies.tooling = ToolingExecutionPolicy::NotRequired;
                revision.capacity.availability = CapacityAvailability::Always;
                revision.validate().unwrap();
                Arc::new(
                    project_apparatus_revision(&revision, AasxSha256::digest(id.as_bytes())).into(),
                )
            })
            .collect(),
    ))
}

async fn fixture() -> (ProductionMapService, Arc<MemoryProductionMapStore>) {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new(store.clone(), resolver());
    service.upsert_map(serde_json::from_value(json!({
        "id":ORDER,"product_code":"FILM","title":"Film order",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"press","kind":"apparatus","title":"Bosma","apparatus_id":PRESS},
            {"id":"lam","kind":"apparatus","title":"Laminatsiya","apparatus_id":LAM},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"press"},{"from":"press","to":"lam"},{"from":"lam","to":"end"}]
    })).unwrap()).await.unwrap();
    store
        .put_order_run_session(session(PRESS, "press", "press-session"))
        .await
        .unwrap();
    store
        .put_order_run_session(session(LAM, "lam", "lam-session"))
        .await
        .unwrap();
    for apparatus in [PRESS, LAM] {
        store
            .put_apparatus_queue_states(
                apparatus,
                BTreeMap::from([(ORDER.into(), "completed".into())]),
            )
            .await
            .unwrap();
    }
    store
        .put_order_progress_batch(roll("original", ""))
        .await
        .unwrap();
    store
        .put_order_progress_batch(roll("remainder", "layer_remainder"))
        .await
        .unwrap();
    (service, store)
}

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "laminator".into(),
        display_name: "Laminator".into(),
    }
}

#[tokio::test]
async fn remainder_is_inventory_without_duplicate_production_or_reopened_stage() {
    let (service, store) = fixture().await;
    let history = service
        .progress_batches_for_worker(&["printer".into()], "", 500)
        .await
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].batch_id, "original");
    assert_eq!(history[0].finished_goods_kg, Some(50.0));
    let before = service.order_status_detail(ORDER).await.unwrap();
    assert_eq!(
        before.lifecycle_status,
        ProductionOrderLifecycleStatus::ProductionCompleted
    );
    let controls = service.queue_action_controls().await.unwrap();
    let control = &controls[LAM][ORDER];
    assert!(control.stage_work.as_ref().unwrap().completed);
    assert!(
        control
            .allowed_actions
            .contains(&queue_state::ApparatusQueueAction::Start)
    );
    assert_eq!(
        control.state,
        queue_state::ApparatusQueueOrderState::Pending
    );
    let scanned = service
        .start_input_for_qr(LAM, ORDER, "remainder", "qr-remainder")
        .await
        .unwrap();
    assert!(scanned.is_layer_remainder());
    let audit = service.audit_production_workflow().await.unwrap();
    assert!(!audit.violations.iter().any(|violation| {
        violation.subject == "remainder"
            && matches!(
                violation.code.as_str(),
                "progress_batch_parent_apparatus_mismatch" | "layer_remainder_producer_mismatch"
            )
    }));
    service
        .apply_apparatus_queue_action_with_progress(
            LAM,
            ORDER,
            queue_state::ApparatusQueueAction::Start,
            &[LAM.into()],
            actor(),
            QueueProgressInput {
                progress_batch_id: "remainder".into(),
                qr_payload: "qr-remainder".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let child = store.progress_batch("remainder").await.unwrap().unwrap();
    assert_eq!(child.wip_status, OrderProgressBatchWipStatus::InUse);
    let original = store.progress_batch("original").await.unwrap().unwrap();
    assert_eq!(original.wip_status, OrderProgressBatchWipStatus::Processed);
    assert_eq!(original.finished_goods_kg, Some(50.0));
}

#[tokio::test]
async fn remainder_correction_is_locked_and_qr_lineage_points_to_physical_source() {
    let (service, store) = fixture().await;
    let error = service.correct_progress_batch(serde_json::from_value(json!({
        "batch_id":"remainder","expected_revision":1,"produced_qty":800.0,"uom":"m","reason":"change"
    })).unwrap(),&QueueActionActor { ref_:"printer".into(),..actor() }).await.unwrap_err();
    assert!(matches!(
        error,
        ProductionMapError::ProgressBatchCorrectionLocked
    ));
    let mut child = store.progress_batch("remainder").await.unwrap().unwrap();
    child.payload_json["source_input_links"] = json!([{"input_batch_id":"unrelated", "input_qr_payload":"qr-unrelated",
        "source_apparatus":PRESS,"source_kind":"progress_batch","sequence_no":1}]);
    store.put_order_progress_batch(child).await.unwrap();
    let report = service
        .progress_qr_report("remainder", "qr-remainder")
        .await
        .unwrap();
    assert_eq!(report.scanned_batch.parent_batch_id, "original");
    assert!(
        report
            .progress_batches
            .iter()
            .any(|b| b.batch_id == "original")
    );
}

#[tokio::test]
async fn paused_session_rescans_returned_input_without_reclaiming_original() {
    let (service, store) = fixture().await;
    let mut paused = session(LAM, "lam", "lam-session");
    paused.status = OrderRunStatus::Paused;
    paused.payload_json["layer_remainder_detached_input_batch_id"] = json!("original");
    paused.payload_json["input_lineage"] = json!([{
        "input_batch_id":"original","input_qr_payload":"qr-original","source_apparatus":PRESS,
        "source_kind":"progress_batch","stage_node_id":"lam","sequence_no":1,
        "status":"processed","linked_at_unix":1,"processed_at_unix":2
    }]);
    store.put_order_run_session(paused).await.unwrap();
    store
        .put_apparatus_queue_states(LAM, BTreeMap::from([(ORDER.into(), "paused".into())]))
        .await
        .unwrap();
    for progress in [
        QueueProgressInput::default(),
        QueueProgressInput {
            progress_batch_id: "original".into(),
            qr_payload: "qr-original".into(),
            ..Default::default()
        },
    ] {
        let error = service
            .apply_apparatus_queue_action_with_progress(
                LAM,
                ORDER,
                queue_state::ApparatusQueueAction::Resume,
                &[LAM.into()],
                actor(),
                progress,
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ProductionMapError::ProgressBatchNotResumable
        ));
    }
    service
        .apply_apparatus_queue_action_with_progress(
            LAM,
            ORDER,
            queue_state::ApparatusQueueAction::Resume,
            &[LAM.into()],
            actor(),
            QueueProgressInput {
                progress_batch_id: "remainder".into(),
                qr_payload: "qr-remainder".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let resumed = store
        .order_run_session("lam-session")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resumed.status, OrderRunStatus::Active);
    assert_eq!(resumed.payload_json["input_progress_batch_id"], "remainder");
    assert!(
        resumed
            .payload_json
            .get("layer_remainder_detached_input_batch_id")
            .is_none()
    );
    assert_eq!(
        store
            .progress_batch("remainder")
            .await
            .unwrap()
            .unwrap()
            .wip_status,
        OrderProgressBatchWipStatus::InUse
    );
    assert_eq!(
        store
            .progress_batch("original")
            .await
            .unwrap()
            .unwrap()
            .wip_status,
        OrderProgressBatchWipStatus::Processed
    );
}
