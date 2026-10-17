#![cfg(feature = "verification")]

use mini_rs_erp::core::{apparatus_standard::*, production_map::*};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

const PRESS: &str = "apparatus:default:bosma_7";
const CUT: &str = "apparatus:default:asset-010";
const LAM: &str = "apparatus:default:asset-007";
const ORDER: &str = "zakaz-rezka-wip-completion-waste";

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
            .find(|item| &item.runtime.apparatus_id == id)
            .cloned())
    }

    async fn list(&self) -> Result<Vec<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        Ok(self.0.clone())
    }
}

fn resolver() -> Arc<Resolver> {
    Arc::new(Resolver(
        [PRESS, CUT, LAM]
            .into_iter()
            .map(|id| {
                let mut revision = mini_rs_apparatus_contract::isa95::test_support::revision_with(
                    id,
                    &format!("physical-asset:{}", id.rsplit(':').next().unwrap()),
                    "Fixture",
                );
                if id != PRESS {
                    let (operation, technology, capability) = if id == CUT {
                        (
                            ExecutionOperation::Cut,
                            ProcessTechnology::Slitting,
                            EquipmentCapabilityCode::Cut,
                        )
                    } else {
                        (
                            ExecutionOperation::Laminate,
                            ProcessTechnology::AdhesiveLamination,
                            EquipmentCapabilityCode::Laminate,
                        )
                    };
                    revision.execution_profile.operation = operation;
                    revision.execution_profile.technology = technology;
                    revision.execution_profile.color_station_count = None;
                    revision.capabilities = vec![EquipmentCapability {
                        code: capability,
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

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "cut-worker".into(),
        display_name: "Cut worker".into(),
    }
}

async fn fixture(
    intermediate: bool,
    input_count: usize,
    upstream_closed: bool,
) -> (ProductionMapService, Arc<MemoryProductionMapStore>) {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new(store.clone(), resolver());
    let mut nodes = vec![
        json!({"id":"start", "kind":"start", "title":"Start"}),
        json!({"id":"press", "kind":"apparatus", "title":"Bosma", "apparatus_id":PRESS}),
        json!({"id":"cut", "kind":"apparatus", "title":"Rezka", "apparatus_id":CUT, "rezka_kadr_count":1}),
    ];
    let mut edges = vec![
        json!({"from":"start", "to":"press"}),
        json!({"from":"press", "to":"cut"}),
    ];
    if intermediate {
        nodes.extend([
            json!({"id":"lam", "kind":"apparatus", "title":"Laminatsiya", "apparatus_id":LAM}),
            json!({"id":"cut-final", "kind":"apparatus", "title":"Rezka", "apparatus_id":CUT, "rezka_kadr_count":1}),
        ]);
        edges.extend([
            json!({"from":"cut", "to":"lam"}),
            json!({"from":"lam", "to":"cut-final"}),
            json!({"from":"cut-final", "to":"end"}),
        ]);
    } else {
        edges.push(json!({"from":"cut", "to":"end"}));
    }
    nodes.push(json!({"id":"end", "kind":"end", "title":"End"}));
    service
        .upsert_map(
            serde_json::from_value(json!({
                "id":ORDER, "product_code":"CUT-WASTE", "title":"Rezka WIP completion",
                "nodes":nodes, "edges":edges,
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    store.put_order_run_session(serde_json::from_value(json!({
        "session_id":"press-run", "apparatus":PRESS, "order_id":ORDER, "stage_node_id":"press",
        "status":if upstream_closed {"completed"} else {"active"},
        "worker_role":"aparatchi", "worker_ref":"printer", "worker_display_name":"Printer",
        "started_at_unix":1, "updated_at_unix":2,
        "payload_json":if upstream_closed {json!({"stage_work_protocol":1, "stage_work_report":{
            "report_id":"press-report", "sequence":1, "submitted_at_unix":2,
            "worker_ref":"printer", "worker_role":"aparatchi", "worker_display_name":"Printer",
        }})} else {json!({"stage_work_protocol":1})},
    })).unwrap()).await.unwrap();
    store
        .put_apparatus_queue_states(
            PRESS,
            BTreeMap::from([(
                ORDER.into(),
                if upstream_closed {
                    "completed"
                } else {
                    "in_progress"
                }
                .into(),
            )]),
        )
        .await
        .unwrap();
    for index in 0..input_count {
        store.put_order_progress_batch(serde_json::from_value(json!({
            "batch_id":format!("input-{index}"), "session_id":"press-run", "apparatus":PRESS,
            "order_id":ORDER, "started_at_unix":1, "completed_at_unix":2,
            "action":"complete", "status":"completed", "produced_qty":100.0, "uom":"m",
            "qr_payload":format!("input-qr-{index}"), "wip_status":"waiting", "next_apparatus":CUT,
                "label_item_code":"CUT-WASTE", "label_item_name":"Film", "executor_name":"Printer",
                "worker_role":"aparatchi", "worker_ref":"printer", "worker_display_name":"Printer",
            "finished_goods_meter":100.0, "finished_goods_kg":10.0,
            "payload_json":{"stage_node_id":"press", "next_stage_node_id":"cut"},
        })).unwrap()).await.unwrap();
    }
    (service, store)
}

async fn start(service: &ProductionMapService, index: usize) {
    service
        .apply_apparatus_queue_action_with_progress(
            CUT,
            ORDER,
            queue_state::ApparatusQueueAction::Start,
            &[CUT.into()],
            actor(),
            QueueProgressInput {
                qr_payload: format!("input-qr-{index}"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
}

fn output() -> QueueProgressInput {
    QueueProgressInput {
        produced_qty: Some(100.0),
        gross_qty: Some(10.0),
        diameter: Some(45.0),
        bobina_kg: Some(1.0),
        uom: "m".into(),
        ..Default::default()
    }
}

async fn check_completion_cycle(intermediate: bool) {
    let (service, store) = fixture(intermediate, 2, true).await;
    start(&service, 0).await;
    let controls = service.queue_action_controls().await.unwrap();
    let control = &controls[CUT][ORDER];
    assert_eq!(control.stage_node_id, "cut");
    assert!(!control.complete_requires_full_report);
    assert!(!control.complete_requires_rezka_total_waste_only);

    // Output quantities remain required even when waste accounting is deferred.
    assert_eq!(
        service
            .apply_apparatus_queue_action_with_progress(
                CUT,
                ORDER,
                queue_state::ApparatusQueueAction::Complete,
                &[CUT.into()],
                actor(),
                QueueProgressInput {
                    diameter: None,
                    ..output()
                },
            )
            .await,
        Err(ProductionMapError::RezkaProgressMetricsRequired)
    );

    let result = service
        .apply_apparatus_queue_action_with_progress(
            CUT,
            ORDER,
            queue_state::ApparatusQueueAction::Complete,
            &[CUT.into()],
            actor(),
            output(),
        )
        .await
        .expect("another input remains: complete this WIP without waste");
    assert_eq!(result.states[ORDER], "pending");
    let batches = store.progress_batches_for_order(ORDER).await.unwrap();
    assert_eq!(
        batches
            .iter()
            .find(|b| b.batch_id == "input-0")
            .unwrap()
            .wip_status,
        OrderProgressBatchWipStatus::Processed
    );
    assert_eq!(
        batches
            .iter()
            .find(|b| b.batch_id == "input-1")
            .unwrap()
            .wip_status,
        OrderProgressBatchWipStatus::Waiting
    );
    let produced = batches.iter().find(|b| b.apparatus == CUT).unwrap();
    assert_eq!(produced.total_waste, None);
    assert_eq!(produced.next_apparatus, if intermediate { LAM } else { "" });

    start(&service, 1).await;
    let controls = service.queue_action_controls().await.unwrap();
    let control = &controls[CUT][ORDER];
    assert_eq!(
        control.stage_node_id, "cut",
        "use the current occurrence of a repeated Rezka"
    );
    assert_eq!(control.complete_requires_full_report, !intermediate);
    assert_eq!(
        control.complete_requires_rezka_total_waste_only,
        intermediate
    );

    // A caller cannot bypass final accounting by setting the partial flag itself.
    assert_eq!(
        service
            .apply_apparatus_queue_action_with_progress(
                CUT,
                ORDER,
                queue_state::ApparatusQueueAction::Complete,
                &[CUT.into()],
                actor(),
                QueueProgressInput {
                    allow_partial_station_completion: true,
                    ..output()
                },
            )
            .await,
        Err(ProductionMapError::RezkaProgressMetricsRequired)
    );
    service
        .apply_apparatus_queue_action_with_progress(
            CUT,
            ORDER,
            queue_state::ApparatusQueueAction::Complete,
            &[CUT.into()],
            actor(),
            QueueProgressInput {
                total_waste: Some(0.5),
                ..output()
            },
        )
        .await
        .expect("last WIP accepts its waste report");
}

#[tokio::test]
async fn intermediate_rezka_defers_waste_until_its_last_wip() {
    check_completion_cycle(true).await;
}

#[tokio::test]
async fn final_rezka_preserves_its_last_wip_waste_report() {
    check_completion_cycle(false).await;
}

#[tokio::test]
async fn intermediate_rezka_defers_waste_while_upstream_is_open() {
    let (service, _) = fixture(true, 1, false).await;
    start(&service, 0).await;
    let controls = service.queue_action_controls().await.unwrap();
    assert!(!controls[CUT][ORDER].complete_requires_rezka_total_waste_only);
    let result = service
        .apply_apparatus_queue_action_with_progress(
            CUT,
            ORDER,
            queue_state::ApparatusQueueAction::Complete,
            &[CUT.into()],
            actor(),
            output(),
        )
        .await
        .expect("upstream can still produce WIP: defer accounting");
    assert_eq!(result.states[ORDER], "pending");
}
