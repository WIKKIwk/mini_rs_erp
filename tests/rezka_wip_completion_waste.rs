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
    let service = fixture_with_store(intermediate, input_count, upstream_closed, store.clone()).await;
    (service, store)
}

async fn fixture_with_store(
    intermediate: bool,
    input_count: usize,
    upstream_closed: bool,
    store: Arc<dyn ProductionMapStorePort>,
) -> ProductionMapService {
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
    service
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

async fn seed_past_roll(store: &MemoryProductionMapStore, start: i64) {
    let mut session = store
        .active_order_run_session(CUT, ORDER)
        .await
        .unwrap()
        .unwrap();
    session.started_at_unix = 10;
    session.payload_json["wip_started_at_unix"] = json!(start);
    store.put_order_run_session(session).await.unwrap();
}

async fn timed_action(
    service: &ProductionMapService,
    action: queue_state::ApparatusQueueAction,
    input: QueueProgressInput,
) -> ApparatusQueueActionResult {
    service
        .apply_apparatus_queue_action_with_progress(
            CUT,
            ORDER,
            action,
            &[CUT.into()],
            actor(),
            input,
        )
        .await
        .unwrap_or_else(|error| panic!("{action:?}: {error:?}"))
}

#[tokio::test]
async fn wip_times_preserve_partial_roll_through_freeze_and_new_roll_starts_on_resume() {
    use queue_state::ApparatusQueueAction as Action;
    let (service, store) = fixture(false, 1, true).await;
    start(&service, 0).await;
    seed_past_roll(&store, 100).await;
    timed_action(
        &service,
        Action::Freeze,
        QueueProgressInput {
            description: "Timing test freeze".into(),
            ..Default::default()
        },
    )
    .await;
    service.unfreeze_order(ORDER, actor()).await.unwrap();
    timed_action(&service, Action::Resume, QueueProgressInput::default()).await;
    let first = timed_action(&service, Action::DetachRoll, output())
        .await
        .progress_batch
        .unwrap();
    assert_eq!(
        first.started_at_unix, 100,
        "physical partial roll start survives freeze"
    );
    let resumed = timed_action(&service, Action::Resume, QueueProgressInput::default())
        .await
        .session
        .unwrap();
    let second = timed_action(&service, Action::DetachRoll, output())
        .await
        .progress_batch
        .unwrap();
    assert_eq!(second.started_at_unix, resumed.updated_at_unix);
    assert!(second.started_at_unix > first.started_at_unix);
    let mut corrected = first.clone();
    corrected.description = "Later measurement correction".into();
    corrected.started_at_unix = 1;
    corrected.completed_at_unix = second.completed_at_unix + 86_400;
    store.put_order_progress_batch(corrected).await.unwrap();
    let saved = store
        .progress_batch(&first.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (saved.started_at_unix, saved.completed_at_unix),
        (first.started_at_unix, first.completed_at_unix)
    );
    assert_eq!(saved.description, "Later measurement correction");
}

#[tokio::test]
async fn wip_times_advance_only_at_continuous_roll_boundary() {
    use queue_state::ApparatusQueueAction as Action;
    let (service, store) = fixture(false, 2, true).await;
    start(&service, 0).await;
    seed_past_roll(&store, 100).await;
    // Replacing an exhausted input does not restart an unfinished output roll.
    timed_action(
        &service,
        Action::Merge,
        QueueProgressInput {
            qr_payload: "input-qr-1".into(),
            ..Default::default()
        },
    )
    .await;
    let first = timed_action(&service, Action::RollComplete, output())
        .await
        .progress_batch
        .unwrap();
    assert_eq!(first.started_at_unix, 100);
    let second = timed_action(&service, Action::RollComplete, output())
        .await
        .progress_batch
        .unwrap();
    assert_eq!(second.started_at_unix, first.completed_at_unix);
    let saved = store
        .progress_batch_by_qr(&first.qr_payload)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (saved.started_at_unix, saved.completed_at_unix),
        (first.started_at_unix, first.completed_at_unix)
    );
}

#[tokio::test]
async fn wip_times_seed_legacy_resume_clock_before_freeze_discards_checkpoint() {
    use queue_state::ApparatusQueueAction as Action;
    let (service, store) = fixture(false, 1, true).await;
    start(&service, 0).await;
    let mut session = store
        .active_order_run_session(CUT, ORDER)
        .await
        .unwrap()
        .unwrap();
    session.started_at_unix = 10;
    session.updated_at_unix = 100;
    session
        .payload_json
        .as_object_mut()
        .unwrap()
        .remove("wip_started_at_unix");
    session.payload_json["_resume_pause"] = json!({"legacy_checkpoint":true});
    store.put_order_run_session(session).await.unwrap();
    timed_action(
        &service,
        Action::Freeze,
        QueueProgressInput {
            description: "Freeze a pre-migration resume".into(),
            ..Default::default()
        },
    )
    .await;
    service.unfreeze_order(ORDER, actor()).await.unwrap();
    timed_action(&service, Action::Resume, QueueProgressInput::default()).await;
    let output = timed_action(&service, Action::DetachRoll, output())
        .await
        .progress_batch
        .unwrap();
    assert_eq!(output.started_at_unix, 100);
}

#[tokio::test]
async fn wip_times_freeze_at_individual_frame_recording_even_when_cycle_closes_later() {
    use queue_state::ApparatusQueueAction as Action;
    let (service, store) = fixture(false, 1, true).await;
    start(&service, 0).await;
    seed_past_roll(&store, 100).await;
    let cycle = store
        .active_order_run_session(CUT, ORDER)
        .await
        .unwrap()
        .unwrap()
        .session_id;
    let frame: RezkaFrameProgressInput = serde_json::from_value(json!({
        "produced_qty":100.0,"gross_qty":10.0,"diameter":45.0,"bobina_kg":1.0
    }))
    .unwrap();
    let record = QueueProgressInput {
        rezka_record_frame_index: Some(1),
        rezka_output_cycle: cycle.clone(),
        rezka_frames: vec![frame.clone()],
        ..Default::default()
    };
    let printed = timed_action(&service, Action::RollComplete, record.clone())
        .await
        .progress_batch
        .unwrap();
    assert_eq!(printed.started_at_unix, 100);
    timed_action(&service, Action::RollComplete, record).await;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let closed = timed_action(
        &service,
        Action::RollComplete,
        QueueProgressInput {
            rezka_output_cycle: cycle,
            rezka_frames: vec![frame],
            total_waste: Some(2.5),
            ..Default::default()
        },
    )
    .await
    .session
    .unwrap();
    assert!(closed.updated_at_unix > printed.completed_at_unix);
    let saved = store
        .progress_batch(&printed.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (saved.started_at_unix, saved.completed_at_unix),
        (printed.started_at_unix, printed.completed_at_unix)
    );
    assert_eq!(saved.total_waste, Some(2.5));
    let next = timed_action(&service, Action::RollComplete, output())
        .await
        .progress_batch
        .unwrap();
    assert_eq!(next.started_at_unix, closed.updated_at_unix);
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

#[tokio::test]
async fn unsaved_issue_outputs_preserve_waste_in_durable_history() {
    for intermediate in [false, true] {
        let (service, store) = fixture(intermediate, 1, true).await;
        start(&service, 0).await;
        let result = service
            .apply_apparatus_queue_action_with_progress(
                CUT, ORDER, queue_state::ApparatusQueueAction::Complete,
                &[CUT.into()], actor(), QueueProgressInput {
                    rezka_frames: vec![RezkaFrameProgressInput {
                        issue_note: "All output damaged".into(), ..Default::default()
                    }],
                    total_waste: Some(2.5),
                    rezka_bosma_waste: (!intermediate).then_some(1.0),
                    rezka_lamination_waste: (!intermediate).then_some(1.0),
                    rezka_edge_waste: (!intermediate).then_some(0.5),
                    ..Default::default()
                },
            ).await.unwrap();
        let event = result.progress_event.unwrap();
        assert_eq!(event.total_waste, Some(2.5));
        assert_eq!(event.payload_json["total_waste"], 2.5);
        assert_eq!(event.rezka_bosma_waste, (!intermediate).then_some(1.0));
        assert!(store.progress_batches_for_order(ORDER).await.unwrap()
            .iter().all(|batch| batch.apparatus != CUT), "No invented output batch");
        let logs = service.queue_action_logs_for_order(ORDER).await.unwrap();
        let reports: Vec<_> = logs.iter().filter_map(|log| log.zero_output_waste.as_ref()).collect();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0]["total_waste"], 2.5);
    }
}

#[tokio::test]
async fn next_roll_does_not_erase_or_repeat_issue_cycle_waste() {
    let (service, _) = fixture(false, 1, true).await;
    start(&service, 0).await;
    service.apply_apparatus_queue_action_with_progress(
        CUT, ORDER, queue_state::ApparatusQueueAction::RollComplete,
        &[CUT.into()], actor(), QueueProgressInput {
            rezka_frames: vec![RezkaFrameProgressInput {
                issue_note: "Damaged roll".into(), ..Default::default()
            }], total_waste: Some(2.5), ..Default::default()
        },
    ).await.unwrap();
    let next = service.apply_apparatus_queue_action_with_progress(
        CUT, ORDER, queue_state::ApparatusQueueAction::RollComplete,
        &[CUT.into()], actor(), output(),
    ).await.unwrap();
    assert_eq!(next.progress_event.unwrap().total_waste, None);
    let logs = service.queue_action_logs_for_order(ORDER).await.unwrap();
    let waste: Vec<_> = logs.iter().filter_map(|log| log.zero_output_waste.as_ref()).collect();
    assert_eq!(waste.len(), 1, "Later roll must neither erase nor repeat waste");
    assert_eq!(waste[0]["total_waste"], 2.5);
}

#[tokio::test]
#[ignore = "requires WASTE_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn postgres_unsaved_issue_waste_survives_order_and_closed_history_reads() {
    use mini_rs_erp::db::{postgres, postgres_production_map::PostgresProductionMapStore};
    let url = std::env::var("WASTE_TEST_DATABASE_URL").expect("disposable database URL required");
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    for intermediate in [false, true] {
        let name = format!("issue_waste_test_{:032x}", rand::random::<u128>());
        sqlx::query(&format!("CREATE DATABASE {name}")).execute(&admin).await.unwrap();
        let options = url.parse::<sqlx::postgres::PgConnectOptions>().unwrap().database(&name);
        let pool = sqlx::PgPool::connect_with(options.clone()).await.unwrap();
        postgres::apply_postgres_migrations_through_version(&pool, "0121").await.unwrap();
        postgres::canonical_apparatus_service(pool.clone()).bootstrap_factory_defaults().await.unwrap();
        postgres::apply_foundation_migration(&pool).await.unwrap();
        let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
        let service = fixture_with_store(intermediate, 1, true, store.clone()).await;
        start(&service, 0).await;
        let result = service.apply_apparatus_queue_action_with_progress(
            CUT, ORDER, queue_state::ApparatusQueueAction::Complete,
            &[CUT.into()], actor(), QueueProgressInput {
                rezka_frames: vec![RezkaFrameProgressInput { issue_note: "Every output damaged".into(), ..Default::default() }],
                total_waste: Some(2.5), rezka_bosma_waste: (!intermediate).then_some(1.0),
                rezka_lamination_waste: (!intermediate).then_some(1.0),
                rezka_edge_waste: (!intermediate).then_some(0.5), ..Default::default()
            },
        ).await.unwrap();
        assert!(result.progress_batches.is_empty());
        let persisted: serde_json::Value = sqlx::query_scalar(
            "SELECT payload_json->'zero_output_waste' FROM mini_queue_action_events WHERE order_id=$1 AND action='complete'")
            .bind(ORDER).fetch_one(&pool).await.unwrap();
        assert_eq!(persisted["total_waste"], 2.5);
        assert_eq!(persisted["rezka_bosma_waste"], if intermediate { json!(null) } else { json!(1.0) });
        assert!(store.progress_batches_for_order(ORDER).await.unwrap().iter().all(|b| b.apparatus != CUT));
        pool.close().await;
        // Both SQL projections and the closed-order serializer read the durable
        // event after reconnect, without relying on a session/cache payload.
        let pool = sqlx::PgPool::connect_with(options).await.unwrap();
        let service = ProductionMapService::new(Arc::new(PostgresProductionMapStore::new(pool.clone())), resolver());
        let logs = service.queue_action_logs_for_order(ORDER).await.unwrap();
        let order_reports: Vec<_> = logs.iter().filter_map(|log| log.zero_output_waste.as_ref()).collect();
        assert_eq!(order_reports, vec![&persisted]);
        let logs = service.queue_action_logs_for_worker(&[actor().ref_], "Worker", 100).await.unwrap();
        let worker_reports: Vec<_> = logs.iter().filter_map(|log| log.zero_output_waste.as_ref()).collect();
        assert_eq!(worker_reports, vec![&persisted]);
        assert!(serde_json::to_value(&logs).unwrap().as_array().unwrap().iter()
            .any(|log| log.get("zero_output_waste") == Some(&persisted)));
        let closed = service.fully_completed_orders(100).await.unwrap();
        if intermediate {
            assert!(closed.iter().all(|order| order.order_id != ORDER));
        } else {
            let order = closed.iter().find(|order| order.order_id == ORDER).expect("closed final-cut order");
            let closed_json = serde_json::to_value(order).unwrap();
            let closed_reports: Vec<_> = closed_json["logs"].as_array().unwrap().iter()
                .filter_map(|log| log.get("zero_output_waste")).collect();
            assert_eq!(closed_reports, vec![&persisted]);
        }
        pool.close().await;
        sqlx::query(&format!("DROP DATABASE {name}")).execute(&admin).await.unwrap();
    }
    admin.close().await;
}
