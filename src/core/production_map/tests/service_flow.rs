use std::collections::BTreeMap;
use std::sync::Arc;

use crate::core::apparatus_standard::test_support::{TestApparatusSpec, canonical_draft};
use crate::core::apparatus_standard::{
    ApparatusId, ApparatusOperationalPolicies, CanonicalApparatusPatch, CanonicalApparatusService,
    CanonicalCommandMetadata, ExecutionOperation, MaterialExecutionPolicy, MaterialRequirementSet,
    ProcessTechnology, QueueDiscipline,
};
use crate::core::production_map::*;
use crate::core::qolip::{QolipOrderStartPreparation, QolipProductSpec};

use super::fixtures::{apparatus_stage_map, canonical_apparatus_stage_map, sample_map};

const FLOW_REZKA_ID: &str = "apparatus:test:flow-rezka";
const FLOW_PECHAT_ID: &str = "apparatus:test:flow-pechat";
const FLOW_LAMINATION_ID: &str = "apparatus:test:flow-lamination";
const FLOW_ALT_PECHAT_ID: &str = "apparatus:test:flow-alt-pechat";
const PECHAT_7_ID: &str = "apparatus:default:bosma_7";
const PECHAT_8_ID: &str = "apparatus:default:bosma_8";
const PECHAT_9_ID: &str = "apparatus:default:bosma_9";
const FLEXO_ID: &str = "apparatus:default:asset-005";
const LAMINATION_1_ID: &str = "apparatus:default:asset-007";
const LAMINATION_2_ID: &str = "apparatus:default:asset-008";
const REZKA_ID: &str = "apparatus:default:asset-010";

async fn service_with_apparatus_store(
    store: Arc<MemoryProductionMapStore>,
    apparatus: &[(&str, &str)],
) -> (ProductionMapService, CanonicalApparatusService) {
    let apparatus_service = apparatus_service_for(apparatus).await;
    let service = ProductionMapService::new(
        store,
        Arc::new(CanonicalServiceApparatusResolver::new(
            apparatus_service.clone(),
        )),
    );
    (service, apparatus_service)
}

async fn service_with_apparatus(
    apparatus: &[(&str, &str)],
) -> (ProductionMapService, CanonicalApparatusService) {
    service_with_apparatus_store(Arc::new(MemoryProductionMapStore::new()), apparatus).await
}

async fn default_service_with_store(store: Arc<MemoryProductionMapStore>) -> ProductionMapService {
    service_with_apparatus_store(store, &[(FLOW_PECHAT_ID, "Flow pechat test")])
        .await
        .0
}

async fn apparatus_service_for(apparatus: &[(&str, &str)]) -> CanonicalApparatusService {
    let service = CanonicalApparatusService::memory();
    let supplied_ids = apparatus
        .iter()
        .map(|(id, _)| *id)
        .collect::<std::collections::BTreeSet<_>>();
    for (id, name) in [
        (PECHAT_7_ID, "7 ta rangli bosma aparat"),
        (PECHAT_8_ID, "8 ta rangli bosma aparat"),
        (PECHAT_9_ID, "9 ta rangli bosma aparat"),
        ("apparatus:default:asset-004", "Extruder laminatsiya"),
        (FLEXO_ID, "Flexo pechat"),
        ("apparatus:default:holodniy_kley", "Holodniy kley aparat"),
        (LAMINATION_1_ID, "Laminatsiya 1"),
        (LAMINATION_2_ID, "Laminatsiya 2"),
        ("apparatus:default:paket", "Paket aparat"),
        (REZKA_ID, "Rezka"),
    ] {
        if supplied_ids.contains(id) {
            continue;
        }
        seed_apparatus(&service, apparatus_spec(id, name))
            .await
            .expect("seed default canonical apparatus");
    }
    for (id, name) in apparatus {
        seed_apparatus(&service, apparatus_spec(id, name))
            .await
            .expect("seed canonical apparatus");
    }
    service
}

async fn seed_apparatus(
    service: &CanonicalApparatusService,
    spec: TestApparatusSpec<'_>,
) -> Result<(), crate::core::apparatus_standard::CanonicalApparatusError> {
    let apparatus_id = ApparatusId::new(spec.apparatus_id).expect("canonical test apparatus id");
    service
        .seed_for_test(apparatus_id, canonical_draft(&spec))
        .await?;
    Ok(())
}

fn apparatus_spec<'a>(id: &'a str, name: &'a str) -> TestApparatusSpec<'a> {
    match id {
        PECHAT_7_ID => TestApparatusSpec::print(id, name, ProcessTechnology::Rotogravure, Some(7)),
        PECHAT_8_ID => TestApparatusSpec::print(id, name, ProcessTechnology::Rotogravure, Some(8)),
        PECHAT_9_ID => TestApparatusSpec::print(id, name, ProcessTechnology::Rotogravure, Some(9)),
        FLEXO_ID => TestApparatusSpec::print(id, name, ProcessTechnology::Flexographic, None),
        FLOW_PECHAT_ID | FLOW_ALT_PECHAT_ID => {
            TestApparatusSpec::print(id, name, ProcessTechnology::Rotogravure, Some(7))
        }
        LAMINATION_1_ID | LAMINATION_2_ID | FLOW_LAMINATION_ID => {
            TestApparatusSpec::laminate(id, name)
        }
        FLOW_REZKA_ID | REZKA_ID => TestApparatusSpec::cut(id, name),
        "apparatus:default:asset-004" => TestApparatusSpec::operation(
            id,
            name,
            ExecutionOperation::Laminate,
            ProcessTechnology::ExtrusionLamination,
        ),
        "apparatus:default:holodniy_kley" => TestApparatusSpec::operation(
            id,
            name,
            ExecutionOperation::Glue,
            ProcessTechnology::ColdGlue,
        ),
        "apparatus:default:paket" => TestApparatusSpec::package(id, name),
        _ => panic!("test apparatus requires an explicit ISA-95 profile: {id}"),
    }
}

async fn set_test_queue_policy(
    service: &CanonicalApparatusService,
    apparatus_id: &ApparatusId,
    policy: ApparatusQueuePolicy,
) {
    let current = service
        .current_configuration(apparatus_id)
        .await
        .expect("current canonical configuration")
        .expect("seeded canonical configuration");
    let policies = ApparatusOperationalPolicies {
        queue: match policy {
            ApparatusQueuePolicy::StrictSequence => QueueDiscipline::StrictSequence,
            ApparatusQueuePolicy::FreePick => QueueDiscipline::FreePick,
        },
        material: current.material.policy.clone(),
        tooling: current.material.tooling.clone(),
    };
    service
        .patch(
            apparatus_id.clone(),
            current.runtime.source_revision,
            CanonicalApparatusPatch {
                policies: Some(policies),
                ..CanonicalApparatusPatch::default()
            },
            CanonicalCommandMetadata::new(
                "user:test",
                format!(
                    "command:test-queue:{apparatus_id}:{}",
                    current.runtime.source_revision
                ),
            ),
        )
        .await
        .expect("canonical queue policy patch");
}

async fn set_test_material_rule(
    service: &CanonicalApparatusService,
    input: ApparatusMaterialRuleUpsert,
) -> Option<ApparatusMaterialRule> {
    let apparatus_id = ApparatusId::new(input.apparatus).expect("canonical apparatus id");
    let current = service
        .current_configuration(&apparatus_id)
        .await
        .expect("current canonical configuration")
        .expect("seeded canonical configuration");
    let material = if !input.requires_material {
        MaterialExecutionPolicy::NotRequired {
            item_group_ids: input.item_groups,
        }
    } else {
        match input.start_policy {
            RawMaterialStartPolicy::StateAll => MaterialExecutionPolicy::AllRequired {
                item_group_ids: input.item_groups,
            },
            RawMaterialStartPolicy::RequirementGroups => MaterialExecutionPolicy::RequirementSets {
                sets: input
                    .requirement_groups
                    .into_iter()
                    .map(|group| MaterialRequirementSet {
                        requirement_id: group.name,
                        item_group_ids: group.item_groups,
                        minimum_required_count: group.min_required_count,
                    })
                    .collect(),
            },
        }
    };
    service
        .patch(
            apparatus_id.clone(),
            current.runtime.source_revision,
            CanonicalApparatusPatch {
                policies: Some(ApparatusOperationalPolicies {
                    queue: current.queue.discipline,
                    material,
                    tooling: current.material.tooling.clone(),
                }),
                ..CanonicalApparatusPatch::default()
            },
            CanonicalCommandMetadata::new(
                "user:test",
                format!(
                    "command:test-material:{apparatus_id}:{}",
                    current.runtime.source_revision
                ),
            ),
        )
        .await
        .expect("canonical material policy patch");
    let updated = service
        .current_configuration(&apparatus_id)
        .await
        .expect("updated canonical configuration")
        .expect("updated projection");
    super::super::materials::live_material_rule(&updated)
}

#[tokio::test]
async fn maps_skips_legacy_invalid_map_without_failing_list() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let mut valid = sample_map();
    valid.id = "valid-map".to_string();
    let mut invalid =
        canonical_apparatus_stage_map("invalid-laminatsiya", LAMINATION_1_ID, "Laminatsiya");
    invalid
        .nodes
        .iter_mut()
        .find(|node| node.kind == ProductionMapNodeKind::Apparatus)
        .expect("invalid apparatus node")
        .apparatus_id = "Laminatsiya".to_string();

    store.put_map(valid).await.expect("valid insert");
    store.put_map(invalid).await.expect("invalid legacy insert");

    let service = default_service_with_store(store).await;
    let maps = service.maps().await.expect("list");
    assert_eq!(maps.len(), 1);
    assert_eq!(maps[0].map.id, "valid-map");
    assert_eq!(
        service.map("invalid-laminatsiya").await,
        Err(ProductionMapError::MissingId)
    );
}

#[tokio::test]
async fn first_stage_completion_keeps_order_available_for_raw_material_assignment() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let order_id = "zakaz-material-after-first-stage";
    service
        .upsert_map(two_stage_map(order_id, FLOW_PECHAT_ID, LAMINATION_1_ID))
        .await
        .expect("two-stage production map");
    assert_eq!(
        service
            .production_order_lifecycle(order_id)
            .await
            .expect("released lifecycle")
            .status,
        ProductionOrderLifecycleStatus::Released
    );
    store
        .put_apparatus_queue_states(
            FLOW_PECHAT_ID,
            BTreeMap::from([(order_id.to_string(), "completed".to_string())]),
        )
        .await
        .expect("first operation completed");
    assert_eq!(
        service
            .production_order_lifecycle(order_id)
            .await
            .expect("in-progress lifecycle")
            .status,
        ProductionOrderLifecycleStatus::InProgress
    );
    assert_eq!(
        service
            .order_status_detail(order_id)
            .await
            .expect("order status detail")
            .lifecycle_status,
        ProductionOrderLifecycleStatus::InProgress
    );

    let candidates = service
        .raw_material_assignment_orders()
        .await
        .expect("raw-material assignment candidates");

    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.map.id == order_id),
        "completing one operation must not complete the production-order header"
    );

    store
        .put_apparatus_queue_states(
            LAMINATION_1_ID,
            BTreeMap::from([(order_id.to_string(), "completed".to_string())]),
        )
        .await
        .expect("final operation completed");
    assert_eq!(
        service
            .production_order_lifecycle(order_id)
            .await
            .expect("completed lifecycle")
            .status,
        ProductionOrderLifecycleStatus::ProductionCompleted
    );
    assert!(
        !service
            .raw_material_assignment_orders()
            .await
            .expect("completed raw-material assignment candidates")
            .iter()
            .any(|candidate| candidate.map.id == order_id),
        "a production-completed order must leave material assignment candidates"
    );
}

#[tokio::test]
async fn live_snapshot_uses_persisted_operational_projection_instead_of_replaying_history() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let order_id = "zakaz-persisted-operational-status";
    service
        .upsert_map(apparatus_stage_map(order_id, FLOW_PECHAT_ID))
        .await
        .expect("production map");
    store
        .put_apparatus_queue_states(
            FLOW_PECHAT_ID,
            BTreeMap::from([(order_id.to_string(), "pending".to_string())]),
        )
        .await
        .expect("persist ready queue projection");

    store
        .put_order_run_session(OrderRunSession {
            session_id: "audit-only-session".to_string(),
            apparatus: FLOW_PECHAT_ID.to_string(),
            order_id: order_id.to_string(),
            status: OrderRunStatus::Active,
            worker_role: "audit".to_string(),
            worker_ref: "audit-only".to_string(),
            worker_display_name: "Audit only".to_string(),
            started_at_unix: 10,
            updated_at_unix: 10,
            payload_json: serde_json::json!({}),
        })
        .await
        .expect("append audit history without a state transition");

    let snapshot = service.live_snapshot().await.expect("live snapshot");
    let status = snapshot
        .order_statuses
        .get(order_id)
        .expect("persisted order status projection");

    assert_eq!(status.order_status, "ready");
    assert_eq!(status.work_status, "waiting");
    assert_eq!(status.flow_status, "ready");

    for (queue_state, expected_order_status) in [
        ("in_progress", "in_progress"),
        ("paused", "paused"),
        ("completed", "completed"),
    ] {
        store
            .put_apparatus_queue_states(
                FLOW_PECHAT_ID,
                BTreeMap::from([(order_id.to_string(), queue_state.to_string())]),
            )
            .await
            .expect("persist operational status transition");
        let snapshot_reader = default_service_with_store(store.clone()).await;
        let snapshot = snapshot_reader
            .live_snapshot()
            .await
            .expect("snapshot after operational transition");
        assert_eq!(
            snapshot
                .order_statuses
                .get(order_id)
                .expect("transitioned operational status")
                .order_status,
            expected_order_status
        );
    }
}

#[tokio::test]
async fn paused_order_can_transfer_between_compatible_pechat_apparatuses_atomically() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-transfer".to_string(),
        display_name: "Transfer Worker".to_string(),
    };
    let order_id = "zakaz-transfer-7-to-8";
    let from = "apparatus:default:bosma_7";
    let to = "apparatus:default:bosma_8";
    service
        .upsert_map(canonical_apparatus_stage_map(
            order_id,
            from,
            "7 ta rangli bosma aparat",
        ))
        .await
        .expect("map");
    service
        .schedule_apparatus_order(ApparatusScheduleRequest {
            order_id: order_id.to_string(),
            apparatus_id: from.to_string(),
            apparatus: "7 ta rangli bosma aparat".to_string(),
            earliest_start_unix: 1_700_000_000,
            latest_end_unix: None,
            duration_minutes: 20,
            priority: 0,
            source: "transfer-test".to_string(),
            reason: "capacity reservation".to_string(),
            idempotency_key: "transfer-capacity-reservation".to_string(),
            capability_requirements: Vec::new(),
            candidate_apparatuses: Vec::new(),
            actor: actor.clone(),
        })
        .await
        .expect("schedule");
    let batch = pause_first_stage_batch(&service, order_id, from, &actor, 42.5)
        .await
        .expect("pause");
    store
        .put_raw_material_assignment(RawMaterialAssignment {
            order_id: order_id.to_string(),
            apparatus_id: ApparatusId::new(from).expect("source apparatus id"),
            apparatus: "7 ta rangli bosma aparat".to_string(),
            barcode: "TRANSFER-RAW-1".to_string(),
            item_code: "TRANSFER-ITEM".to_string(),
            item_name: "Transfer material".to_string(),
            item_group: "Transfer group".to_string(),
            assigned_by_role: actor.role.clone(),
            assigned_by_ref: actor.ref_.clone(),
            assigned_by_display_name: actor.display_name.clone(),
            assigned_at: "now".to_string(),
        })
        .await
        .expect("raw material assignment");
    assert_eq!(
        service
            .apparatus_capacity_snapshot()
            .await
            .expect("paused reservation")
            .reservations[0]
            .status,
        ApparatusScheduleStatus::Paused
    );

    let result = service
        .transfer_apparatus_order(
            ProductionMapApparatusTransferRequest {
                order_id: order_id.to_string(),
                from_apparatus: from.to_string(),
                to_apparatus: to.to_string(),
                reason: "7 rangli aparat avariyasi".to_string(),
                idempotency_key: "transfer-test-7-to-8".to_string(),
            },
            actor.clone(),
        )
        .await
        .expect("transfer");
    assert_eq!(result.saved.map.id, order_id);
    assert_eq!(result.transfer.from_apparatus, from);
    assert_eq!(result.transfer.to_apparatus, to);
    assert_eq!(result.transfer.progress_batch_id, batch.batch_id);
    assert_eq!(result.transfer.session.apparatus, to);
    assert_eq!(result.transfer.progress_batch.apparatus, to);
    let reservation = service
        .apparatus_capacity_snapshot()
        .await
        .expect("transferred reservation")
        .reservations
        .into_iter()
        .next()
        .expect("reservation");
    assert_eq!(reservation.status, ApparatusScheduleStatus::Paused);
    assert_eq!(reservation.apparatus, to);
    assert_eq!(
        reservation.apparatus_id,
        ApparatusId::new(to).expect("canonical apparatus id")
    );

    let states = service.apparatus_queue_states().await.expect("states");
    assert_eq!(
        states.get(from).and_then(|states| states.get(order_id)),
        None
    );
    assert_eq!(
        states.get(to).and_then(|states| states.get(order_id)),
        Some(&"paused".to_string())
    );
    let session = store
        .order_run_session(&result.transfer.session_id)
        .await
        .expect("session lookup")
        .expect("session");
    assert_eq!(session.apparatus, to);
    let moved_batch = store
        .progress_batch(&result.transfer.progress_batch_id)
        .await
        .expect("batch lookup")
        .expect("batch");
    assert_eq!(moved_batch.apparatus, to);
    assert_eq!(moved_batch.produced_qty, 42.5);
    let moved_assignment = service
        .raw_material_assignments()
        .await
        .expect("moved raw material assignments")
        .into_iter()
        .find(|assignment| assignment.barcode == "TRANSFER-RAW-1")
        .expect("moved raw material assignment");
    assert_eq!(
        moved_assignment.apparatus_id,
        ApparatusId::new(to).expect("target apparatus id")
    );
    assert_eq!(moved_assignment.apparatus, "8 ta rangli bosma aparat");

    let resumed = service
        .apply_apparatus_queue_action_with_progress(
            to,
            order_id,
            queue_state::ApparatusQueueAction::Resume,
            &[to.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: result.transfer.progress_batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("resume on replacement apparatus");
    assert_eq!(
        resumed.states.get(order_id),
        Some(&"in_progress".to_string())
    );
    assert_eq!(
        resumed.progress_batch.expect("resumed batch").status,
        OrderProgressBatchStatus::Resumed
    );

    let replay = service
        .transfer_apparatus_order(
            ProductionMapApparatusTransferRequest {
                order_id: order_id.to_string(),
                from_apparatus: from.to_string(),
                to_apparatus: to.to_string(),
                reason: "different retry text".to_string(),
                idempotency_key: "transfer-test-7-to-8".to_string(),
            },
            actor,
        )
        .await
        .expect("idempotent replay");
    assert_eq!(replay.transfer.transfer_id, result.transfer.transfer_id);
    assert_eq!(replay.transfer.reason, "7 rangli aparat avariyasi");

    let conflicting_replay = service
        .transfer_apparatus_order(
            ProductionMapApparatusTransferRequest {
                order_id: order_id.to_string(),
                from_apparatus: from.to_string(),
                to_apparatus: "apparatus:default:bosma_9".to_string(),
                reason: "wrong target reuse".to_string(),
                idempotency_key: "transfer-test-7-to-8".to_string(),
            },
            QueueActionActor {
                role: "admin".to_string(),
                ref_: "retry".to_string(),
                display_name: "Retry".to_string(),
            },
        )
        .await;
    assert_eq!(
        conflicting_replay,
        Err(ProductionMapError::ApparatusTransferIdempotencyConflict)
    );
}

#[tokio::test]
async fn apparatus_transfer_cannot_bypass_frozen_or_completed_stage_state() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-transfer-state-guard".to_string(),
        display_name: "Transfer State Guard".to_string(),
    };
    let order_id = "zakaz-transfer-state-guard";
    let from = PECHAT_7_ID;
    let to = PECHAT_8_ID;
    service
        .upsert_map(canonical_apparatus_stage_map(
            order_id,
            from,
            "7 ta rangli pechat",
        ))
        .await
        .expect("map");
    pause_first_stage_batch(&service, order_id, from, &actor, 6.0)
        .await
        .expect("pause");

    let mut frozen = OrderControlRecord::active(order_id);
    frozen.state = OrderControlState::Frozen;
    store
        .put_order_control_state(frozen)
        .await
        .expect("frozen control");
    let frozen_result = service
        .transfer_apparatus_order(
            ProductionMapApparatusTransferRequest {
                order_id: order_id.to_string(),
                from_apparatus: from.to_string(),
                to_apparatus: to.to_string(),
                reason: "frozen state guard".to_string(),
                idempotency_key: "transfer-frozen-state-guard".to_string(),
            },
            actor.clone(),
        )
        .await;
    assert_eq!(frozen_result, Err(ProductionMapError::OrderFrozen));

    let mut freeze_requested = OrderControlRecord::active(order_id);
    freeze_requested.state = OrderControlState::FreezeRequested;
    store
        .put_order_control_state(freeze_requested)
        .await
        .expect("freeze requested control");
    let requested_result = service
        .transfer_apparatus_order(
            ProductionMapApparatusTransferRequest {
                order_id: order_id.to_string(),
                from_apparatus: from.to_string(),
                to_apparatus: to.to_string(),
                reason: "freeze request guard".to_string(),
                idempotency_key: "transfer-freeze-requested-guard".to_string(),
            },
            actor.clone(),
        )
        .await;
    assert_eq!(
        requested_result,
        Err(ProductionMapError::OrderFreezeRequested)
    );

    store
        .put_order_control_state(OrderControlRecord::active(order_id))
        .await
        .expect("active control");
    store
        .put_apparatus_queue_states(
            from,
            BTreeMap::from([(order_id.to_string(), "completed".to_string())]),
        )
        .await
        .expect("completed state");
    let completed_result = service
        .transfer_apparatus_order(
            ProductionMapApparatusTransferRequest {
                order_id: order_id.to_string(),
                from_apparatus: from.to_string(),
                to_apparatus: to.to_string(),
                reason: "completed state guard".to_string(),
                idempotency_key: "transfer-completed-state-guard".to_string(),
            },
            actor,
        )
        .await;
    assert_eq!(
        completed_result,
        Err(ProductionMapError::OrderAlreadyCompleted)
    );

    let states = service.apparatus_queue_states().await.expect("states");
    assert_eq!(
        states.get(from).and_then(|states| states.get(order_id)),
        Some(&"completed".to_string())
    );
    assert_eq!(states.get(to).and_then(|states| states.get(order_id)), None);
}

#[tokio::test]
async fn apparatus_transfer_updates_parent_wip_next_apparatus_lineage() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-transfer-lineage".to_string(),
        display_name: "Transfer Lineage Worker".to_string(),
    };
    let order_id = "zakaz-transfer-lineage";
    let from = PECHAT_7_ID;
    let to = PECHAT_8_ID;
    service
        .upsert_map(canonical_apparatus_stage_map(
            order_id,
            from,
            "7 ta rangli pechat",
        ))
        .await
        .expect("map");
    let child = pause_first_stage_batch(&service, order_id, from, &actor, 12.0)
        .await
        .expect("pause");

    let mut parent = child.clone();
    parent.batch_id = "parent-lineage-batch".to_string();
    parent.action = queue_state::ApparatusQueueAction::Complete;
    parent.status = OrderProgressBatchStatus::Completed;
    parent.parent_batch_id.clear();
    parent.next_apparatus = from.to_string();
    parent.qr_payload = "parent-lineage-qr".to_string();
    store
        .put_order_progress_batch(parent)
        .await
        .expect("parent batch");
    let mut child_with_parent = child.clone();
    child_with_parent.parent_batch_id = "parent-lineage-batch".to_string();
    store
        .put_order_progress_batch(child_with_parent)
        .await
        .expect("child lineage");

    let result = service
        .transfer_apparatus_order(
            ProductionMapApparatusTransferRequest {
                order_id: order_id.to_string(),
                from_apparatus: from.to_string(),
                to_apparatus: to.to_string(),
                reason: "7 rangli aparat avariyasi".to_string(),
                idempotency_key: "transfer-lineage-test".to_string(),
            },
            actor,
        )
        .await
        .expect("transfer");
    assert_eq!(result.transfer.progress_batch_updates.len(), 1);
    let updated_parent = store
        .progress_batch("parent-lineage-batch")
        .await
        .expect("parent lookup")
        .expect("parent");
    assert_eq!(updated_parent.next_apparatus, to);
}

#[tokio::test]
async fn flexo_transfer_does_not_cross_into_colour_pechat() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-flexo-transfer".to_string(),
        display_name: "Flexo Worker".to_string(),
    };
    let order_id = "zakaz-flexo-transfer";
    let from = FLEXO_ID;
    service
        .upsert_map(canonical_apparatus_stage_map(
            order_id,
            from,
            "Flexo pechat",
        ))
        .await
        .expect("map");
    pause_first_stage_batch(&service, order_id, from, &actor, 8.0)
        .await
        .expect("pause");

    let result = service
        .transfer_apparatus_order(
            ProductionMapApparatusTransferRequest {
                order_id: order_id.to_string(),
                from_apparatus: from.to_string(),
                to_apparatus: PECHAT_8_ID.to_string(),
                reason: "flexo apparat avariyasi".to_string(),
                idempotency_key: "transfer-flexo-cross-family".to_string(),
            },
            actor,
        )
        .await;
    assert_eq!(result, Err(ProductionMapError::MoveNotAllowed));
}

#[tokio::test]
async fn normal_move_rejects_started_order_and_requires_pause_transfer() {
    let service = default_service_with_store(Arc::new(MemoryProductionMapStore::new())).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-transfer-guard".to_string(),
        display_name: "Transfer Guard".to_string(),
    };
    let order_id = "zakaz-transfer-guard";
    let from = PECHAT_7_ID;
    service
        .upsert_map(canonical_apparatus_stage_map(
            order_id,
            from,
            "7 ta rangli pechat",
        ))
        .await
        .expect("map");
    start_first_stage(&service, order_id, from, actor)
        .await
        .expect("start");

    let result = service
        .move_apparatus(ProductionMapMoveRequest {
            map_id: order_id.to_string(),
            from_apparatus: from.to_string(),
            to_apparatus: PECHAT_8_ID.to_string(),
        })
        .await;
    assert_eq!(
        result,
        Err(ProductionMapError::StartedOrderMoveRequiresTransfer)
    );
}

#[tokio::test]
async fn free_pick_policy_allows_ready_order_outside_sequence_head() {
    let (service, apparatus_service) =
        service_with_apparatus(&[(FLOW_REZKA_ID, "Rezka apparat")]).await;
    let actor = QueueActionActor {
        role: "admin".to_string(),
        ref_: "admin".to_string(),
        display_name: "Admin".to_string(),
    };
    let first = canonical_apparatus_stage_map("zakaz-1", FLOW_REZKA_ID, "Rezka apparat");
    let second = canonical_apparatus_stage_map("zakaz-2", FLOW_REZKA_ID, "Rezka apparat");
    service.upsert_map(first).await.expect("first map");
    service.upsert_map(second).await.expect("second map");
    service
        .set_apparatus_sequence(
            FLOW_REZKA_ID,
            vec!["zakaz-1".to_string(), "zakaz-2".to_string()],
        )
        .await
        .expect("sequence");

    let strict_result = service
        .apply_apparatus_queue_action(
            FLOW_REZKA_ID,
            "zakaz-2",
            queue_state::ApparatusQueueAction::Start,
            &[FLOW_REZKA_ID.to_string()],
            actor.clone(),
        )
        .await;
    assert_eq!(
        strict_result,
        Err(ProductionMapError::QueueActionNotAllowed)
    );

    let apparatus_id = ApparatusId::new(FLOW_REZKA_ID).expect("canonical test apparatus id");
    set_test_queue_policy(
        &apparatus_service,
        &apparatus_id,
        ApparatusQueuePolicy::FreePick,
    )
    .await;
    let states = service
        .apply_apparatus_queue_action(
            FLOW_REZKA_ID,
            "zakaz-2",
            queue_state::ApparatusQueueAction::Start,
            &[FLOW_REZKA_ID.to_string()],
            actor.clone(),
        )
        .await
        .expect("start second");
    assert_eq!(states.get("zakaz-2"), Some(&"in_progress".to_string()));
}

#[tokio::test]
async fn apparatus_sequence_rejects_unknown_and_wrong_apparatus_orders() {
    let service = default_service_with_store(Arc::new(MemoryProductionMapStore::new())).await;
    service
        .upsert_map(canonical_apparatus_stage_map(
            "zakaz-sequence-valid",
            REZKA_ID,
            "Rezka apparat",
        ))
        .await
        .expect("map");

    assert_eq!(
        service
            .set_apparatus_sequence(REZKA_ID, vec!["zakaz-sequence-missing".to_string()],)
            .await,
        Err(ProductionMapError::QueueSequenceOrderNotFound(
            "zakaz-sequence-missing".to_string(),
        ))
    );
    assert_eq!(
        service
            .set_apparatus_sequence(LAMINATION_1_ID, vec!["zakaz-sequence-valid".to_string()],)
            .await,
        Err(ProductionMapError::QueueSequenceApparatusMismatch(
            "zakaz-sequence-valid".to_string(),
        ))
    );
    assert!(
        service
            .apparatus_sequences()
            .await
            .expect("sequences after rejection")
            .is_empty()
    );

    service
        .set_apparatus_sequence(REZKA_ID, vec!["zakaz-sequence-valid".to_string()])
        .await
        .expect("valid sequence");
}

#[tokio::test]
async fn pechat_queue_policy_is_an_explicit_canonical_policy() {
    let (_service, apparatus_service) =
        service_with_apparatus(&[("apparatus:default:bosma_7", "7 ta rangli bosma aparat")]).await;
    let apparatus_id =
        ApparatusId::new("apparatus:default:bosma_7").expect("canonical pechat apparatus id");
    set_test_queue_policy(
        &apparatus_service,
        &apparatus_id,
        ApparatusQueuePolicy::FreePick,
    )
    .await;
    let current = apparatus_service.current_configuration(&apparatus_id).await;
    assert_eq!(
        current.unwrap().unwrap().queue.discipline,
        QueueDiscipline::FreePick
    );
}

#[tokio::test]
async fn queue_controls_follow_canonical_material_policy_edits() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let (service, apparatus_service) =
        service_with_apparatus_store(store.clone(), &[(FLOW_PECHAT_ID, "7 ta rangli pechat - A")])
            .await;
    let order_id = "zakaz-canonical-material-controls";
    service
        .upsert_map(canonical_apparatus_stage_map(
            order_id,
            FLOW_PECHAT_ID,
            "7 ta rangli pechat - A",
        ))
        .await
        .expect("map");
    service
        .set_apparatus_sequence(FLOW_PECHAT_ID, vec![order_id.to_string()])
        .await
        .expect("sequence");

    set_test_material_rule(
        &apparatus_service,
        ApparatusMaterialRuleUpsert {
            apparatus: FLOW_PECHAT_ID.to_string(),
            requires_material: true,
            start_policy: RawMaterialStartPolicy::StateAll,
            item_groups: vec!["Kraska".to_string()],
            requirement_groups: Vec::new(),
        },
    )
    .await
    .expect("canonical material policy");
    let controls = service
        .queue_action_controls()
        .await
        .expect("controls after canonical edit");
    let control = controls
        .get(FLOW_PECHAT_ID)
        .and_then(|orders| orders.get(order_id))
        .expect("canonical material control");
    assert_eq!(
        control.interaction.start_materials_mode,
        ApparatusQueueStartMaterialsMode::ScanRequired
    );
    assert_eq!(
        control.interaction.blocking_reason_code,
        "raw_material_assignment_required"
    );

    assert!(
        set_test_material_rule(
            &apparatus_service,
            ApparatusMaterialRuleUpsert {
                apparatus: FLOW_PECHAT_ID.to_string(),
                requires_material: false,
                start_policy: RawMaterialStartPolicy::StateAll,
                item_groups: Vec::new(),
                requirement_groups: Vec::new(),
            },
        )
        .await
        .is_none()
    );
    let controls = service
        .queue_action_controls()
        .await
        .expect("controls after canonical reset");
    let control = controls
        .get(FLOW_PECHAT_ID)
        .and_then(|orders| orders.get(order_id))
        .expect("reset material control");
    assert_eq!(
        control.interaction.start_materials_mode,
        ApparatusQueueStartMaterialsMode::Hidden
    );
}

#[tokio::test]
async fn queue_policy_decisions_follow_the_canonical_projection() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let (service, apparatus_service) =
        service_with_apparatus_store(store.clone(), &[(FLOW_REZKA_ID, "Rezka apparat")]).await;
    let first = canonical_apparatus_stage_map("zakaz-canonical-policy-1", FLOW_REZKA_ID, "Rezka");
    let second = canonical_apparatus_stage_map("zakaz-canonical-policy-2", FLOW_REZKA_ID, "Rezka");
    service.upsert_map(first).await.expect("first map");
    service.upsert_map(second).await.expect("second map");
    service
        .set_apparatus_sequence(
            FLOW_REZKA_ID,
            vec![
                "zakaz-canonical-policy-1".to_string(),
                "zakaz-canonical-policy-2".to_string(),
            ],
        )
        .await
        .expect("sequence");

    let actor = QueueActionActor {
        role: "admin".to_string(),
        ref_: "canonical-policy-test".to_string(),
        display_name: "Canonical policy test".to_string(),
    };
    let apparatus_id = ApparatusId::new(FLOW_REZKA_ID).expect("canonical apparatus id");
    set_test_queue_policy(
        &apparatus_service,
        &apparatus_id,
        ApparatusQueuePolicy::FreePick,
    )
    .await;

    let states = service
        .apply_apparatus_queue_action(
            FLOW_REZKA_ID,
            "zakaz-canonical-policy-2",
            queue_state::ApparatusQueueAction::Start,
            &[FLOW_REZKA_ID.to_string()],
            actor,
        )
        .await
        .expect("canonical free-pick decision");
    assert_eq!(
        states.get("zakaz-canonical-policy-2"),
        Some(&"in_progress".to_string())
    );
}

#[tokio::test]
async fn queue_action_controls_are_backend_owned_for_each_order_state() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let apparatus = LAMINATION_1_ID;
    let order_id = "zakaz-action-controls";

    service
        .upsert_map(canonical_apparatus_stage_map(
            order_id,
            apparatus,
            "7 ta rangli pechat",
        ))
        .await
        .expect("map");
    service
        .set_apparatus_sequence(apparatus, vec![order_id.to_string()])
        .await
        .expect("sequence");

    store
        .put_apparatus_queue_states(
            apparatus,
            BTreeMap::from([(order_id.to_string(), "in_progress".to_string())]),
        )
        .await
        .expect("in-progress state");
    let controls = service.queue_action_controls().await.expect("controls");
    let in_progress = controls
        .get(apparatus)
        .and_then(|orders| orders.get(order_id))
        .expect("in-progress control");
    assert_eq!(
        in_progress.state,
        queue_state::ApparatusQueueOrderState::InProgress
    );
    assert!(
        in_progress
            .allowed_actions
            .contains(&queue_state::ApparatusQueueAction::Pause)
    );
    assert!(
        in_progress
            .allowed_actions
            .contains(&queue_state::ApparatusQueueAction::Complete)
    );
    assert!(
        in_progress
            .allowed_actions
            .contains(&queue_state::ApparatusQueueAction::Freeze)
    );
    assert!(
        !in_progress
            .allowed_actions
            .contains(&queue_state::ApparatusQueueAction::Resume)
    );
    assert!(in_progress.complete_requires_full_report);

    store
        .put_apparatus_queue_states(
            apparatus,
            BTreeMap::from([(order_id.to_string(), "paused".to_string())]),
        )
        .await
        .expect("paused state");
    let controls = service
        .queue_action_controls()
        .await
        .expect("paused controls");
    let paused = controls
        .get(apparatus)
        .and_then(|orders| orders.get(order_id))
        .expect("paused control");
    assert_eq!(
        paused.allowed_actions,
        vec![queue_state::ApparatusQueueAction::Resume]
    );
}

#[tokio::test]
async fn raw_material_state_policy_requires_only_staged_scan_before_start() {
    let (service, apparatus_service) = service_with_apparatus(&[
        (FLOW_PECHAT_ID, "7 ta rangli pechat - A"),
        (FLOW_REZKA_ID, "Rezka apparat"),
    ])
    .await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-1".to_string(),
        display_name: "Worker 1".to_string(),
    };
    service
        .upsert_map(canonical_apparatus_stage_map(
            "zakaz-raw-1",
            FLOW_PECHAT_ID,
            "7 ta rangli pechat - A",
        ))
        .await
        .expect("map");
    service
        .set_apparatus_sequence(FLOW_PECHAT_ID, vec!["zakaz-raw-1".to_string()])
        .await
        .expect("sequence");
    set_test_material_rule(
        &apparatus_service,
        ApparatusMaterialRuleUpsert {
            apparatus: FLOW_PECHAT_ID.to_string(),
            requires_material: true,
            start_policy: RawMaterialStartPolicy::StateAll,
            item_groups: vec!["Kraska".to_string(), "Kley".to_string()],
            requirement_groups: Vec::new(),
        },
    )
    .await
    .expect("material rule");
    let missing_assignment = service
        .apply_apparatus_queue_action_with_material_scan(
            FLOW_PECHAT_ID,
            "zakaz-raw-1",
            queue_state::ApparatusQueueAction::Start,
            &[FLOW_PECHAT_ID.to_string()],
            actor.clone(),
            "",
            &[],
        )
        .await;
    assert_eq!(
        missing_assignment,
        Err(ProductionMapError::RawMaterialAssignmentNotFound)
    );
    let assigned = service
        .assign_raw_material_to_order(
            RawMaterialAssignmentInput {
                order_id: "zakaz-raw-1".to_string(),
                barcode: "30AA".to_string(),
                item_code: "INK-BLACK".to_string(),
                item_name: "Black ink".to_string(),
                item_group: "Kraska".to_string(),
                item_group_path: Vec::new(),
                apparatus: String::new(),
            },
            &actor,
        )
        .await
        .expect("assign material");
    assert_eq!(assigned.apparatus, FLOW_PECHAT_ID);
    let second_assigned = service
        .assign_raw_material_to_order(
            RawMaterialAssignmentInput {
                order_id: "zakaz-raw-1".to_string(),
                barcode: "30CC".to_string(),
                item_code: "INK-WHITE".to_string(),
                item_name: "White ink".to_string(),
                item_group: "Kraska".to_string(),
                item_group_path: Vec::new(),
                apparatus: String::new(),
            },
            &actor,
        )
        .await
        .expect("assign second material");
    assert_eq!(second_assigned.apparatus, FLOW_PECHAT_ID);

    service
        .upsert_map(canonical_apparatus_stage_map(
            "zakaz-raw-2",
            FLOW_PECHAT_ID,
            "7 ta rangli pechat - A",
        ))
        .await
        .expect("second map");
    let duplicate = service
        .assign_raw_material_to_order(
            RawMaterialAssignmentInput {
                order_id: "zakaz-raw-2".to_string(),
                barcode: "30AA".to_string(),
                item_code: "INK-BLACK".to_string(),
                item_name: "Black ink".to_string(),
                item_group: "Kraska".to_string(),
                item_group_path: Vec::new(),
                apparatus: String::new(),
            },
            &actor,
        )
        .await;
    assert_eq!(
        duplicate,
        Err(ProductionMapError::RawMaterialAlreadyAssigned)
    );

    let not_assigned = service
        .apply_apparatus_queue_action_with_material_scan(
            FLOW_PECHAT_ID,
            "zakaz-raw-1",
            queue_state::ApparatusQueueAction::Start,
            &[FLOW_REZKA_ID.to_string()],
            actor.clone(),
            "",
            &[],
        )
        .await;
    assert_eq!(not_assigned, Err(ProductionMapError::ApparatusNotAssigned));

    let missing_scan = service
        .apply_apparatus_queue_action_with_material_scan(
            FLOW_PECHAT_ID,
            "zakaz-raw-1",
            queue_state::ApparatusQueueAction::Start,
            &[FLOW_PECHAT_ID.to_string()],
            actor.clone(),
            "",
            &["30AA".to_string()],
        )
        .await;
    assert_eq!(
        missing_scan,
        Err(ProductionMapError::RawMaterialScanRequired)
    );

    let wrong_scan = service
        .apply_apparatus_queue_action_with_material_scan(
            FLOW_PECHAT_ID,
            "zakaz-raw-1",
            queue_state::ApparatusQueueAction::Start,
            &[FLOW_PECHAT_ID.to_string()],
            actor.clone(),
            "30BB",
            &["30AA".to_string()],
        )
        .await;
    assert_eq!(wrong_scan, Err(ProductionMapError::RawMaterialMismatch));

    let partial_scan = service
        .apply_apparatus_queue_action_with_material_scan(
            FLOW_PECHAT_ID,
            "zakaz-raw-1",
            queue_state::ApparatusQueueAction::Start,
            &[FLOW_PECHAT_ID.to_string()],
            actor.clone(),
            "30AA",
            &["30AA".to_string(), "30CC".to_string()],
        )
        .await;
    assert_eq!(
        partial_scan,
        Err(ProductionMapError::RawMaterialScanIncomplete)
    );

    let requirements = service
        .raw_material_start_requirements(
            FLOW_PECHAT_ID,
            "zakaz-raw-1",
            &["30AA".to_string()],
            "30AA",
        )
        .await
        .expect("start requirements");
    assert_eq!(requirements.policy, RawMaterialStartPolicy::StateAll);
    assert_eq!(
        requirements.assigned_barcodes,
        vec!["30AA".to_string(), "30CC".to_string()]
    );
    assert_eq!(requirements.staged_barcodes, vec!["30AA".to_string()]);
    assert_eq!(requirements.eligible_barcodes, vec!["30AA".to_string()]);
    assert_eq!(requirements.required_scan_count, 1);
    assert_eq!(requirements.matched_scan_count, 1);
    assert!(requirements.assignments_satisfied);
    assert!(requirements.scan_satisfied);

    let states = service
        .apply_apparatus_queue_action_with_material_scan(
            FLOW_PECHAT_ID,
            "zakaz-raw-1",
            queue_state::ApparatusQueueAction::Start,
            &[FLOW_PECHAT_ID.to_string()],
            actor.clone(),
            "30AA",
            &["30AA".to_string()],
        )
        .await
        .expect("start with the staged material only");
    assert_eq!(states.get("zakaz-raw-1"), Some(&"in_progress".to_string()));
}

#[tokio::test]
async fn optional_material_assignment_requires_scan_before_start() {
    let (service, apparatus_service) =
        service_with_apparatus(&[(FLOW_PECHAT_ID, "7 ta rangli pechat - A")]).await;
    let order_id = "zakaz-raw-optional";
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-optional".to_string(),
        display_name: "Optional material worker".to_string(),
    };
    service
        .upsert_map(canonical_apparatus_stage_map(
            order_id,
            FLOW_PECHAT_ID,
            "7 ta rangli pechat - A",
        ))
        .await
        .expect("map");
    service
        .set_apparatus_sequence(FLOW_PECHAT_ID, vec![order_id.to_string()])
        .await
        .expect("sequence");
    set_test_material_rule(
        &apparatus_service,
        ApparatusMaterialRuleUpsert {
            apparatus: FLOW_PECHAT_ID.to_string(),
            requires_material: false,
            start_policy: RawMaterialStartPolicy::StateAll,
            item_groups: vec!["Kraska".to_string()],
            requirement_groups: Vec::new(),
        },
    )
    .await
    .expect("optional material rule");
    service
        .assign_raw_material_to_order(
            RawMaterialAssignmentInput {
                order_id: order_id.to_string(),
                barcode: "OPTIONAL-INK-1".to_string(),
                item_code: "INK-BLACK".to_string(),
                item_name: "Black ink".to_string(),
                item_group: "Kraska".to_string(),
                item_group_path: Vec::new(),
                apparatus: FLOW_PECHAT_ID.to_string(),
            },
            &actor,
        )
        .await
        .expect("optional material assignment");

    let controls = service
        .queue_action_controls()
        .await
        .expect("optional material controls");
    let control = controls
        .get(FLOW_PECHAT_ID)
        .and_then(|orders| orders.get(order_id))
        .expect("optional material control");
    assert_eq!(
        control.interaction.start_materials_mode,
        ApparatusQueueStartMaterialsMode::ScanRequired
    );
    assert!(control.interaction.material_scan_required);
    assert!(!control.interaction.assigned_materials_display_only);
    assert!(
        control
            .allowed_actions
            .contains(&queue_state::ApparatusQueueAction::Start)
    );

    let requirements = service
        .raw_material_start_requirements(
            FLOW_PECHAT_ID,
            order_id,
            &["OPTIONAL-INK-1".to_string()],
            "",
        )
        .await
        .expect("optional material start requirements");
    assert!(!requirements.requires_material);
    assert!(requirements.material_scan_required);
    assert!(requirements.assignments_satisfied);
    assert!(!requirements.scan_satisfied);
    assert_eq!(requirements.required_scan_count, 1);

    let error = service
        .apply_apparatus_queue_action_with_material_scan(
            FLOW_PECHAT_ID,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[FLOW_PECHAT_ID.to_string()],
            actor.clone(),
            "",
            &["OPTIONAL-INK-1".to_string()],
        )
        .await
        .expect_err("assigned material must be scanned before start");
    assert_eq!(error, ProductionMapError::RawMaterialScanRequired);

    let states = service
        .apply_apparatus_queue_action_with_material_scan(
            FLOW_PECHAT_ID,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[FLOW_PECHAT_ID.to_string()],
            actor,
            "OPTIONAL-INK-1",
            &["OPTIONAL-INK-1".to_string()],
        )
        .await
        .expect("assigned material scan starts the order");
    assert_eq!(states.get(order_id), Some(&"in_progress".to_string()));
}

#[tokio::test]
async fn additional_raw_material_is_only_received_by_assigned_worker_while_order_is_active() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let apparatus = FLOW_PECHAT_ID;
    let apparatus_service = apparatus_service_for(&[
        (FLOW_PECHAT_ID, "7 ta rangli pechat - A"),
        (FLOW_REZKA_ID, "Rezka apparat"),
    ])
    .await;
    let service = ProductionMapService::new(
        store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(
            apparatus_service.clone(),
        )),
    );
    let order_id = "zakaz-raw-intake";
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-1".to_string(),
        display_name: "Worker 1".to_string(),
    };
    service
        .upsert_map(canonical_apparatus_stage_map(
            order_id,
            apparatus,
            "7 ta rangli pechat - A",
        ))
        .await
        .expect("map");
    set_test_material_rule(
        &apparatus_service,
        ApparatusMaterialRuleUpsert {
            apparatus: apparatus.to_string(),
            requires_material: true,
            start_policy: RawMaterialStartPolicy::StateAll,
            item_groups: vec!["Kraska".to_string()],
            requirement_groups: Vec::new(),
        },
    )
    .await
    .expect("material rule");

    let input = |barcode: &str| RawMaterialAssignmentInput {
        order_id: order_id.to_string(),
        barcode: barcode.to_string(),
        item_code: "INK-BLACK".to_string(),
        item_name: "Black ink".to_string(),
        item_group: "Kraska".to_string(),
        item_group_path: Vec::new(),
        apparatus: apparatus.to_string(),
    };
    let supplier_actor = QueueActionActor {
        role: "material_taminotchi".to_string(),
        ref_: "supplier-1".to_string(),
        display_name: "Material Supplier".to_string(),
    };
    for barcode in ["ROLL-1000-A", "ROLL-1000-B", "ROLL-1000-C"] {
        service
            .assign_raw_material_to_order(input(barcode), &supplier_actor)
            .await
            .expect("supplier assignment");
    }
    store
        .put_apparatus_queue_states(
            apparatus,
            BTreeMap::from([(order_id.to_string(), "pending".to_string())]),
        )
        .await
        .expect("pending state");
    let before_start = service
        .receive_raw_material_for_active_order(
            input("ROLL-1000-A"),
            &[apparatus.to_string()],
            &actor,
        )
        .await;
    assert_eq!(
        before_start,
        Err(ProductionMapError::RawMaterialOrderNotActive)
    );

    store
        .put_apparatus_queue_states(
            apparatus,
            BTreeMap::from([(order_id.to_string(), "in_progress".to_string())]),
        )
        .await
        .expect("active state");
    let wrong_worker = service
        .receive_raw_material_for_active_order(
            input("ROLL-1000-A"),
            &[FLOW_REZKA_ID.to_string()],
            &actor,
        )
        .await;
    assert_eq!(wrong_worker, Err(ProductionMapError::ApparatusNotAssigned));

    set_test_material_rule(
        &apparatus_service,
        ApparatusMaterialRuleUpsert {
            apparatus: apparatus.to_string(),
            requires_material: true,
            start_policy: RawMaterialStartPolicy::StateAll,
            item_groups: vec!["Kley".to_string()],
            requirement_groups: Vec::new(),
        },
    )
    .await
    .expect("changed material rule");
    let disallowed_by_current_rule = service
        .receive_raw_material_for_active_order(
            input("ROLL-1000-A"),
            &[apparatus.to_string()],
            &actor,
        )
        .await;
    assert_eq!(
        disallowed_by_current_rule,
        Err(ProductionMapError::RawMaterialGroupNotAllowed)
    );
    set_test_material_rule(
        &apparatus_service,
        ApparatusMaterialRuleUpsert {
            apparatus: apparatus.to_string(),
            requires_material: true,
            start_policy: RawMaterialStartPolicy::StateAll,
            item_groups: vec!["Kraska".to_string()],
            requirement_groups: Vec::new(),
        },
    )
    .await
    .expect("restored material rule");

    let assignment_count = service
        .raw_material_assignments()
        .await
        .expect("assignments")
        .len();
    let unassigned = service
        .receive_raw_material_for_active_order(
            input("ROLL-UNASSIGNED"),
            &[apparatus.to_string()],
            &actor,
        )
        .await;
    assert_eq!(
        unassigned,
        Err(ProductionMapError::RawMaterialAssignmentNotFound)
    );
    assert_eq!(
        service
            .raw_material_assignments()
            .await
            .expect("assignments after rejected intake")
            .len(),
        assignment_count
    );

    let (first, warehouses) = service
        .receive_raw_material_for_active_order(
            input("ROLL-1000-A"),
            &[apparatus.to_string()],
            &actor,
        )
        .await
        .expect("receive while running");
    assert_eq!(first.barcode, "ROLL-1000-A");
    assert!(warehouses.is_empty());

    store
        .put_apparatus_queue_states(
            apparatus,
            BTreeMap::from([(order_id.to_string(), "paused".to_string())]),
        )
        .await
        .expect("paused state");
    service
        .receive_raw_material_for_active_order(
            input("ROLL-1000-B"),
            &[apparatus.to_string()],
            &actor,
        )
        .await
        .expect("receive while paused");
    assert_eq!(
        service
            .raw_material_assignments()
            .await
            .expect("assignments")
            .len(),
        3
    );

    let mut frozen_control = OrderControlRecord::active(order_id);
    frozen_control.state = OrderControlState::Frozen;
    store
        .put_order_control_state(frozen_control)
        .await
        .expect("frozen control");
    let while_frozen = service
        .receive_raw_material_for_active_order(
            input("ROLL-1000-C"),
            &[apparatus.to_string()],
            &actor,
        )
        .await;
    assert_eq!(while_frozen, Err(ProductionMapError::OrderFrozen));
    store
        .put_order_control_state(OrderControlRecord::active(order_id))
        .await
        .expect("active control");

    store
        .put_apparatus_queue_states(
            apparatus,
            BTreeMap::from([(order_id.to_string(), "completed".to_string())]),
        )
        .await
        .expect("completed state");
    let after_complete = service
        .receive_raw_material_for_active_order(
            input("ROLL-1000-C"),
            &[apparatus.to_string()],
            &actor,
        )
        .await;
    assert_eq!(
        after_complete,
        Err(ProductionMapError::RawMaterialOrderNotActive)
    );
}

#[tokio::test]
async fn raw_material_assignment_returns_choices_and_accepts_selected_apparatus() {
    let (service, apparatus_service) = service_with_apparatus(&[
        (FLOW_ALT_PECHAT_ID, "Pechat A"),
        (FLOW_LAMINATION_ID, "Flow laminatsiya 1"),
    ])
    .await;
    let actor = QueueActionActor {
        role: "admin".to_string(),
        ref_: "admin".to_string(),
        display_name: "Admin".to_string(),
    };
    let mut map = canonical_apparatus_stage_map("zakaz-raw-choice", FLOW_ALT_PECHAT_ID, "Pechat A");
    let mut second_apparatus = map.nodes[1].clone();
    second_apparatus.id = "apparatus-2".to_string();
    second_apparatus.title = "Laminatsiya 1".to_string();
    second_apparatus.apparatus_id = FLOW_LAMINATION_ID.to_string();
    map.nodes.insert(2, second_apparatus);
    map.edges = vec![
        ProductionMapEdge {
            from: "start".to_string(),
            to: "apparatus".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "apparatus".to_string(),
            to: "apparatus-2".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "apparatus-2".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
    ];
    service.upsert_map(map).await.expect("map");
    for apparatus in [FLOW_ALT_PECHAT_ID, FLOW_LAMINATION_ID] {
        set_test_material_rule(
            &apparatus_service,
            ApparatusMaterialRuleUpsert {
                apparatus: apparatus.to_string(),
                requires_material: true,
                start_policy: RawMaterialStartPolicy::StateAll,
                item_groups: vec!["Kraska".to_string()],
                requirement_groups: Vec::new(),
            },
        )
        .await
        .expect("material rule");
    }

    let ambiguous = service
        .assign_raw_material_to_order(
            RawMaterialAssignmentInput {
                order_id: "zakaz-raw-choice".to_string(),
                barcode: "30CHOICE".to_string(),
                item_code: "INK-CHOICE".to_string(),
                item_name: "Choice ink".to_string(),
                item_group: "Kraska".to_string(),
                item_group_path: Vec::new(),
                apparatus: String::new(),
            },
            &actor,
        )
        .await;
    assert_eq!(
        ambiguous,
        Err(ProductionMapError::RawMaterialGroupAmbiguous(vec![
            FLOW_ALT_PECHAT_ID.to_string(),
            FLOW_LAMINATION_ID.to_string(),
        ]))
    );

    let assigned = service
        .assign_raw_material_to_order(
            RawMaterialAssignmentInput {
                order_id: "zakaz-raw-choice".to_string(),
                barcode: "30CHOICE".to_string(),
                item_code: "INK-CHOICE".to_string(),
                item_name: "Choice ink".to_string(),
                item_group: "Kraska".to_string(),
                item_group_path: Vec::new(),
                apparatus: FLOW_LAMINATION_ID.to_string(),
            },
            &actor,
        )
        .await
        .expect("assign selected apparatus");
    assert_eq!(assigned.apparatus, FLOW_LAMINATION_ID);
}

#[tokio::test]
async fn raw_material_requirement_group_accepts_alternative_item_group() {
    let (service, apparatus_service) =
        service_with_apparatus(&[(FLOW_LAMINATION_ID, "Flow laminatsiya 1")]).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-alternative-material".to_string(),
        display_name: "Worker Alternative Material".to_string(),
    };
    service
        .upsert_map(canonical_apparatus_stage_map(
            "zakaz-raw-alt",
            FLOW_LAMINATION_ID,
            "Laminatsiya 1",
        ))
        .await
        .expect("map");
    let rule = set_test_material_rule(
        &apparatus_service,
        ApparatusMaterialRuleUpsert {
            apparatus: FLOW_LAMINATION_ID.to_string(),
            requires_material: true,
            start_policy: RawMaterialStartPolicy::RequirementGroups,
            item_groups: Vec::new(),
            requirement_groups: vec![ApparatusMaterialRequirementGroup {
                name: "Yopishtiruvchi".to_string(),
                item_groups: vec!["Kley".to_string(), "Kraska".to_string()],
                min_required_count: 1,
            }],
        },
    )
    .await
    .expect("material rule");
    assert_eq!(rule.requirement_groups.len(), 1);

    let assigned = service
        .assign_raw_material_to_order(
            RawMaterialAssignmentInput {
                order_id: "zakaz-raw-alt".to_string(),
                barcode: "30ALT".to_string(),
                item_code: "INK-ALT".to_string(),
                item_name: "Alternative kraska".to_string(),
                item_group: "Kraska".to_string(),
                item_group_path: Vec::new(),
                apparatus: String::new(),
            },
            &actor,
        )
        .await
        .expect("assign alternative material");
    assert_eq!(assigned.apparatus, FLOW_LAMINATION_ID);

    let states = service
        .apply_apparatus_queue_action_with_material_scan(
            FLOW_LAMINATION_ID,
            "zakaz-raw-alt",
            queue_state::ApparatusQueueAction::Start,
            &[FLOW_LAMINATION_ID.to_string()],
            actor,
            "30ALT",
            &[],
        )
        .await
        .expect("start with alternative material");
    assert_eq!(
        states.get("zakaz-raw-alt"),
        Some(&"in_progress".to_string())
    );
}

#[tokio::test]
async fn raw_material_requirement_groups_need_distinct_scanned_materials() {
    let (service, apparatus_service) =
        service_with_apparatus(&[(FLOW_ALT_PECHAT_ID, "Pechat A")]).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-distinct-material".to_string(),
        display_name: "Worker Distinct Material".to_string(),
    };
    service
        .upsert_map(canonical_apparatus_stage_map(
            "zakaz-raw-distinct",
            FLOW_ALT_PECHAT_ID,
            "Pechat A",
        ))
        .await
        .expect("map");
    set_test_material_rule(
        &apparatus_service,
        ApparatusMaterialRuleUpsert {
            apparatus: FLOW_ALT_PECHAT_ID.to_string(),
            requires_material: true,
            start_policy: RawMaterialStartPolicy::RequirementGroups,
            item_groups: Vec::new(),
            requirement_groups: vec![
                ApparatusMaterialRequirementGroup {
                    name: "Bo'yoq".to_string(),
                    item_groups: vec!["Kraska".to_string(), "Universal".to_string()],
                    min_required_count: 1,
                },
                ApparatusMaterialRequirementGroup {
                    name: "Yopishtiruvchi".to_string(),
                    item_groups: vec!["Kley".to_string(), "Universal".to_string()],
                    min_required_count: 1,
                },
            ],
        },
    )
    .await
    .expect("material rule");
    for (barcode, item_group) in [("30UNIVERSAL", "Universal"), ("30KLEY", "Kley")] {
        service
            .assign_raw_material_to_order(
                RawMaterialAssignmentInput {
                    order_id: "zakaz-raw-distinct".to_string(),
                    barcode: barcode.to_string(),
                    item_code: barcode.to_string(),
                    item_name: barcode.to_string(),
                    item_group: item_group.to_string(),
                    item_group_path: Vec::new(),
                    apparatus: String::new(),
                },
                &actor,
            )
            .await
            .expect("assign material");
    }

    let reused_for_two_groups = service
        .apply_apparatus_queue_action_with_material_scan(
            FLOW_ALT_PECHAT_ID,
            "zakaz-raw-distinct",
            queue_state::ApparatusQueueAction::Start,
            &[FLOW_ALT_PECHAT_ID.to_string()],
            actor.clone(),
            "30UNIVERSAL",
            &[],
        )
        .await;
    assert_eq!(
        reused_for_two_groups,
        Err(ProductionMapError::RawMaterialRequirementNotMet)
    );

    let states = service
        .apply_apparatus_queue_action_with_material_scan(
            FLOW_ALT_PECHAT_ID,
            "zakaz-raw-distinct",
            queue_state::ApparatusQueueAction::Start,
            &[FLOW_ALT_PECHAT_ID.to_string()],
            actor,
            "30UNIVERSAL,30KLEY",
            &[],
        )
        .await
        .expect("start with two distinct materials");
    assert_eq!(
        states.get("zakaz-raw-distinct"),
        Some(&"in_progress".to_string())
    );
}

#[tokio::test]
async fn paused_next_order_resumes_after_previous_order_completed() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-resume".to_string(),
        display_name: "Worker Resume".to_string(),
    };
    let apparatus = FLOW_PECHAT_ID;
    let completed_order_id = "zakaz-resume-completed";
    let paused_order_id = "zakaz-resume-paused";
    service
        .upsert_map(apparatus_stage_map(completed_order_id, apparatus))
        .await
        .expect("completed map");
    service
        .upsert_map(apparatus_stage_map(paused_order_id, apparatus))
        .await
        .expect("paused map");
    service
        .set_apparatus_sequence(
            apparatus,
            vec![completed_order_id.to_string(), paused_order_id.to_string()],
        )
        .await
        .expect("sequence");
    store
        .put_apparatus_queue_states(
            apparatus,
            BTreeMap::from([
                (completed_order_id.to_string(), "completed".to_string()),
                (paused_order_id.to_string(), "pending".to_string()),
            ]),
        )
        .await
        .expect("queue states");
    service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            paused_order_id,
            queue_state::ApparatusQueueAction::Start,
            &[apparatus.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("start next order");
    service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            paused_order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[apparatus.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(1.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("pause next order");

    let states = service
        .apply_apparatus_queue_action(
            apparatus,
            paused_order_id,
            queue_state::ApparatusQueueAction::Resume,
            &[apparatus.to_string()],
            actor,
        )
        .await
        .expect("resume paused next order");

    assert_eq!(
        states.get(paused_order_id),
        Some(&"in_progress".to_string())
    );
}

#[tokio::test]
async fn final_stage_pause_output_is_finished_goods_while_work_resumes() {
    let service = default_service_with_store(Arc::new(MemoryProductionMapStore::new())).await;
    let apparatus = FLOW_PECHAT_ID;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-progress-1".to_string(),
        display_name: "Worker Progress".to_string(),
    };
    service
        .upsert_map(apparatus_stage_map("zakaz-progress-1", apparatus))
        .await
        .expect("map");

    let started = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            "zakaz-progress-1",
            queue_state::ApparatusQueueAction::Start,
            &[apparatus.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("start");
    assert_eq!(
        started.states.get("zakaz-progress-1"),
        Some(&"in_progress".to_string())
    );
    assert!(started.session.is_some());
    assert!(started.progress_batch.is_none());

    let paused = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            "zakaz-progress-1",
            queue_state::ApparatusQueueAction::Pause,
            &[apparatus.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(42.5),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("pause");
    assert_eq!(
        paused.states.get("zakaz-progress-1"),
        Some(&"paused".to_string())
    );
    let batch = paused.progress_batch.expect("pause batch");
    assert_eq!(batch.status, OrderProgressBatchStatus::Paused);
    assert_eq!(batch.produced_qty, 42.5);
    assert_eq!(batch.qr_payload.len(), 24);
    assert!(batch.qr_payload.starts_with("4001"));
    assert!(batch.qr_payload.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert!(batch.label_item_name.contains("tayyor mahsulot"));
    assert!(batch.label_item_name.contains("chiqarildi"));
    assert!(!batch.label_item_name.contains("yarim tayyor mahsulot"));
    assert!(batch.next_apparatus.is_empty());
    assert_eq!(batch.status_detail.flow_status, "free_wip");
    assert_eq!(batch.executor_name, "Worker Progress");

    let paused_order_status = service
        .order_status_detail("zakaz-progress-1")
        .await
        .expect("paused order status");
    assert_eq!(paused_order_status.order_status, "paused");
    assert_eq!(paused_order_status.flow_status, "free_wip");

    let lookup = service
        .progress_batch_for_qr("", &batch.qr_payload)
        .await
        .expect("lookup");
    assert_eq!(lookup.batch_id, batch.batch_id);

    let resumed = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            "zakaz-progress-1",
            queue_state::ApparatusQueueAction::Resume,
            &[apparatus.to_string()],
            actor,
            QueueProgressInput {
                qr_payload: batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("resume");
    assert_eq!(
        resumed.states.get("zakaz-progress-1"),
        Some(&"in_progress".to_string())
    );
    assert_eq!(
        resumed
            .progress_batch
            .as_ref()
            .expect("resumed batch")
            .status,
        OrderProgressBatchStatus::Resumed
    );
    assert_eq!(
        resumed
            .progress_batch
            .as_ref()
            .expect("resumed batch")
            .wip_status,
        OrderProgressBatchWipStatus::Waiting
    );
    assert!(
        resumed
            .progress_batch
            .as_ref()
            .expect("resumed batch")
            .used_by_apparatus
            .is_empty()
    );
    assert_eq!(
        resumed
            .session
            .as_ref()
            .expect("resumed session")
            .payload_json["input_progress_batch_id"]
            .as_str()
            .unwrap_or_default(),
        ""
    );
    assert_eq!(
        resumed
            .progress_batch
            .as_ref()
            .expect("resumed batch")
            .status_detail
            .flow_status,
        "free_wip"
    );
}

#[tokio::test]
async fn worker_roll_detach_has_canonical_status_without_pausing_order() {
    let service = default_service_with_store(Arc::new(MemoryProductionMapStore::new())).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-roll-detach".to_string(),
        display_name: "Worker Roll Detach".to_string(),
    };
    let order_id = "zakaz-roll-detach";
    let apparatus = FLOW_PECHAT_ID;
    service
        .upsert_map(apparatus_stage_map(order_id, apparatus))
        .await
        .expect("map");

    service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[apparatus.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("start");

    let detached = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::DetachRoll,
            &[apparatus.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(42.5),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("detach roll");
    assert_eq!(
        detached.states.get(order_id),
        Some(&"paused".to_string()),
        "legacy queue scheduling state stays backward compatible"
    );
    assert_eq!(
        detached.session.as_ref().expect("detached session").status,
        OrderRunStatus::RollDetached
    );
    let batch = detached.progress_batch.expect("detached roll batch");
    assert_eq!(batch.action, queue_state::ApparatusQueueAction::DetachRoll);
    assert_eq!(batch.status, OrderProgressBatchStatus::RollDetached);
    assert!(batch.label_item_name.contains("rulon yechildi"));

    let order_status = service
        .order_status_detail(order_id)
        .await
        .expect("detached order status");
    assert_eq!(order_status.order_status, "in_progress");
    assert_eq!(order_status.roll_detached_session_count, 1);
    assert_eq!(order_status.paused_session_count, 0);

    let resumed = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Resume,
            &[apparatus.to_string()],
            actor,
            QueueProgressInput {
                qr_payload: batch.qr_payload,
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("resume detached roll");
    assert_eq!(
        resumed.states.get(order_id),
        Some(&"in_progress".to_string())
    );
    assert_eq!(
        resumed.progress_batch.expect("resumed batch").status,
        OrderProgressBatchStatus::Resumed
    );
}

#[tokio::test]
async fn first_stage_pause_resume_complete_keeps_both_wips_available_for_laminatsiya() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-first-stage-two-wips".to_string(),
        display_name: "First Stage Two WIPs".to_string(),
    };
    let order_id = "zakaz-first-stage-two-wips";
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");

    service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("bosma start");
    let first_output = service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(12.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("bosma pause")
        .progress_batch
        .expect("first bosma output");
    service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Resume,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("bosma resume");
    let second_output = service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Complete,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(13.0),
                uom: "kg".to_string(),
                return_ink_kg: Some(0.1),
                total_waste: Some(0.1),
                finished_goods_kg: Some(13.0),
                finished_goods_meter: Some(130.0),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("bosma complete")
        .progress_batch
        .expect("second bosma output");

    for output in [&first_output, &second_output] {
        let persisted = store
            .progress_batch(&output.batch_id)
            .await
            .expect("output lookup")
            .expect("persisted bosma output");
        assert_eq!(persisted.wip_status, OrderProgressBatchWipStatus::Waiting);
        assert!(persisted.used_by_apparatus.is_empty());
        assert!(persisted.processed_by_apparatus.is_empty());
    }

    service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                qr_payload: first_output.qr_payload,
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("laminatsiya starts first WIP");
    let controls = service
        .queue_action_controls()
        .await
        .expect("queue controls");
    let laminatsiya = controls
        .get(second)
        .and_then(|orders| orders.get(order_id))
        .expect("laminatsiya controls");
    assert!(!laminatsiya.complete_requires_full_report);
}

#[tokio::test]
async fn laminatsiya_worker_handoff_keeps_roll_in_apparatus_until_continue_or_remove() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "laminatsiyachi".to_string(),
        ref_: "worker-laminatsiya-handoff".to_string(),
        display_name: "Laminatsiya Handoff".to_string(),
    };
    let order_id = "zakaz-laminatsiya-handoff";
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");

    service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("bosma start");
    let bosma_pause = service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(18.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("bosma pause");
    let source_batch = bosma_pause.progress_batch.expect("bosma WIP");

    service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: source_batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("laminatsiya start");
    let handoff = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                lamination_print_leftover_rolls: Some(0.0),
                lamination_film_leftover_rolls: Some(0.0),
                total_waste: Some(0.0),
                worker_handoff: true,
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("worker handoff");
    assert_eq!(handoff.states.get(order_id), Some(&"paused".to_string()));
    assert!(handoff.progress_batch.is_none());
    assert_eq!(
        handoff
            .progress_event
            .as_ref()
            .expect("handoff event")
            .payload_json["event"],
        "worker_handoff"
    );

    let handed_off_source = store
        .progress_batch(&source_batch.batch_id)
        .await
        .expect("handoff source lookup")
        .expect("handoff source");
    assert_eq!(
        handed_off_source.wip_status,
        OrderProgressBatchWipStatus::InUse
    );
    assert_eq!(
        handed_off_source.payload_json["worker_handoff"],
        serde_json::Value::Bool(true)
    );

    let removed = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::DetachRoll,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                finished_goods_meter: Some(320.0),
                finished_goods_kg: Some(12.0),
                bobina_kg: Some(12.0),
                remove_roll_from_apparatus: true,
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("remove roll");
    assert_eq!(removed.states.get(order_id), Some(&"paused".to_string()));
    assert!(removed.progress_batch.is_none());
    let removed_source = store
        .progress_batch(&source_batch.batch_id)
        .await
        .expect("removed source lookup")
        .expect("removed source");
    assert_eq!(
        removed_source.wip_status,
        OrderProgressBatchWipStatus::Waiting
    );
    assert_eq!(
        removed_source.payload_json["roll_removed_from_apparatus"],
        serde_json::Value::Bool(true)
    );
    assert!(removed_source.used_by_apparatus.is_empty());

    let resumed = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Resume,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("resume removed roll");
    assert_eq!(
        resumed.states.get(order_id),
        Some(&"in_progress".to_string())
    );
    let resumed_source = store
        .progress_batch(&source_batch.batch_id)
        .await
        .expect("resumed source lookup")
        .expect("resumed source");
    assert_eq!(
        resumed_source.wip_status,
        OrderProgressBatchWipStatus::InUse
    );
    assert_eq!(
        resumed_source.payload_json["worker_handoff"],
        serde_json::Value::Bool(false)
    );

    let normal_pause = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                finished_goods_meter: Some(300.0),
                finished_goods_kg: Some(11.0),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("normal pause after resume");
    assert!(normal_pause.progress_batch.is_some());
    let active_source = store
        .progress_batch(&source_batch.batch_id)
        .await
        .expect("active source lookup")
        .expect("active source");
    assert_eq!(active_source.wip_status, OrderProgressBatchWipStatus::InUse);
}

#[tokio::test]
async fn downstream_start_requires_previous_stage_progress_qr() {
    let service = default_service_with_store(Arc::new(MemoryProductionMapStore::new())).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-downstream-1".to_string(),
        display_name: "Worker Downstream".to_string(),
    };
    let order_id = "zakaz-downstream-1";
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");

    service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("first start");

    let second_without_qr = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await;
    assert_eq!(
        second_without_qr,
        Err(ProductionMapError::ProgressQrRequired)
    );

    let paused = service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(18.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("first pause");
    let previous_batch = paused.progress_batch.expect("previous batch");

    let second_started = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: previous_batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second start with previous qr");
    assert_eq!(
        second_started.states.get(order_id),
        Some(&"in_progress".to_string())
    );
    let event = second_started.progress_event.expect("start event");
    assert_eq!(
        event.payload_json["input_progress_batch_id"],
        previous_batch.batch_id
    );
    assert_eq!(event.payload_json["input_progress_apparatus"], first);
}

#[tokio::test]
async fn laminatsiya_with_unresolvable_previous_stage_cannot_start() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "laminatsiyachi".to_string(),
        ref_: "worker-laminatsiya-missing-previous".to_string(),
        display_name: "Laminatsiya Missing Previous".to_string(),
    };
    let order_id = "zakaz-laminatsiya-missing-previous";

    let mut map = canonical_apparatus_stage_map(order_id, LAMINATION_1_ID, "Laminatsiya 1");
    let mut orphan_previous = map
        .nodes
        .iter()
        .find(|node| node.kind == ProductionMapNodeKind::Apparatus)
        .cloned()
        .expect("laminatsiya stage");
    orphan_previous.id = "orphan-bosma".to_string();
    orphan_previous.title = "7 ta rangli bosma aparat".to_string();
    orphan_previous.apparatus_id = FLOW_PECHAT_ID.to_string();
    map.nodes.push(orphan_previous);

    service.upsert_map(map).await.expect("map");
    store
        .put_apparatus_sequence(LAMINATION_1_ID, vec![order_id.to_string()])
        .await
        .expect("stale sequence");

    let controls = service.queue_action_controls().await.expect("controls");
    let control = controls
        .get(LAMINATION_1_ID)
        .and_then(|orders| orders.get(order_id))
        .expect("laminatsiya control");
    assert!(control.allowed_actions.is_empty());
    assert_eq!(
        control.interaction.blocking_reason_code,
        "previous_stage_not_configured"
    );

    let result = service
        .apply_apparatus_queue_action_with_progress(
            LAMINATION_1_ID,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[LAMINATION_1_ID.to_string()],
            actor,
            QueueProgressInput::default(),
        )
        .await;

    assert_eq!(result, Err(ProductionMapError::ProgressQrRequired));
}

#[tokio::test]
async fn laminatsiya_complete_requires_previous_stage_qr() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-laminatsiya-complete-without-input".to_string(),
        display_name: "Laminatsiya Complete Without Input".to_string(),
    };
    let order_id = "zakaz-laminatsiya-complete-without-input";
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");
    store
        .put_apparatus_queue_states(
            second,
            BTreeMap::from([(order_id.to_string(), "in_progress".to_string())]),
        )
        .await
        .expect("in-progress state");
    let missing_session = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Complete,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(10.0),
                uom: "kg".to_string(),
                lamination_print_leftover_rolls: Some(0.0),
                lamination_film_leftover_rolls: Some(0.0),
                total_waste: Some(0.0),
                finished_goods_kg: Some(10.0),
                finished_goods_meter: Some(100.0),
                ..QueueProgressInput::default()
            },
        )
        .await;
    assert_eq!(
        missing_session,
        Err(ProductionMapError::QueueActionNotAllowed)
    );
    store
        .put_order_run_session(OrderRunSession {
            session_id: "session-laminatsiya-complete-without-input".to_string(),
            apparatus: second.to_string(),
            order_id: order_id.to_string(),
            status: OrderRunStatus::Active,
            worker_role: actor.role.clone(),
            worker_ref: actor.ref_.clone(),
            worker_display_name: actor.display_name.clone(),
            started_at_unix: 100,
            updated_at_unix: 100,
            payload_json: serde_json::json!({}),
        })
        .await
        .expect("active session without previous-stage input");

    let result = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Complete,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                produced_qty: Some(10.0),
                uom: "kg".to_string(),
                lamination_print_leftover_rolls: Some(0.0),
                lamination_film_leftover_rolls: Some(0.0),
                total_waste: Some(0.0),
                finished_goods_kg: Some(10.0),
                finished_goods_meter: Some(100.0),
                ..QueueProgressInput::default()
            },
        )
        .await;

    assert_eq!(result, Err(ProductionMapError::ProgressQrRequired));
}

#[tokio::test]
async fn downstream_pause_resume_preserves_original_input_until_complete() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "laminatsiyachi".to_string(),
        ref_: "worker-laminatsiya-pause-resume".to_string(),
        display_name: "Laminatsiya Pause Resume".to_string(),
    };
    let order_id = "zakaz-laminatsiya-pause-resume";
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");
    let source_batch = pause_first_stage_batch(&service, order_id, first, &actor, 20.0)
        .await
        .expect("source WIP");
    service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: source_batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("laminatsiya start");

    let paused = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(10.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("laminatsiya pause");
    let pause_batch = paused.progress_batch.expect("pause WIP");
    assert_eq!(pause_batch.wip_status, OrderProgressBatchWipStatus::Waiting);
    assert_eq!(pause_batch.parent_batch_id, source_batch.batch_id);
    assert_eq!(
        store
            .progress_batch(&source_batch.batch_id)
            .await
            .expect("source lookup after pause")
            .expect("source after pause")
            .wip_status,
        OrderProgressBatchWipStatus::InUse
    );

    let resumed = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Resume,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("resume without QR");
    let resumed_batch = resumed.progress_batch.as_ref().expect("resumed WIP");
    let resumed_session = resumed.session.as_ref().expect("resumed session");
    assert_eq!(
        resumed.states.get(order_id),
        Some(&"in_progress".to_string())
    );
    assert_eq!(resumed_batch.batch_id, pause_batch.batch_id);
    assert_eq!(resumed_batch.status, OrderProgressBatchStatus::Resumed);
    assert_eq!(
        resumed_batch.wip_status,
        OrderProgressBatchWipStatus::Waiting
    );
    assert!(resumed_batch.used_by_session_id.is_empty());
    assert_eq!(
        resumed_session.payload_json["input_progress_batch_id"].as_str(),
        Some(source_batch.batch_id.as_str())
    );

    let completed = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Complete,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                produced_qty: Some(9.0),
                uom: "kg".to_string(),
                lamination_film_leftover_rolls: Some(1.0),
                total_waste: Some(0.5),
                finished_goods_kg: Some(9.0),
                finished_goods_meter: Some(90.0),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("complete after resume");
    assert_eq!(
        completed
            .progress_batch
            .as_ref()
            .expect("completed WIP")
            .parent_batch_id,
        source_batch.batch_id
    );
    assert_eq!(
        store
            .progress_batch(&source_batch.batch_id)
            .await
            .expect("source WIP lookup")
            .expect("persisted source WIP")
            .wip_status,
        OrderProgressBatchWipStatus::Processed
    );
    assert_eq!(
        store
            .progress_batch(&pause_batch.batch_id)
            .await
            .expect("pause output lookup")
            .expect("persisted pause output")
            .wip_status,
        OrderProgressBatchWipStatus::Waiting
    );
}

#[tokio::test]
async fn rezka_resume_reopens_all_frames_from_the_scanned_input_wip() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "rezkachi".to_string(),
        ref_: "worker-rezka-frame-resume".to_string(),
        display_name: "Rezka Frame Resume".to_string(),
    };
    let order_id = "zakaz-rezka-frame-resume";
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    let third = REZKA_ID;
    service
        .upsert_map(three_stage_map(order_id, first, second, third, 3))
        .await
        .expect("map");

    let source_batch = pause_first_stage_batch(&service, order_id, first, &actor, 20.0)
        .await
        .expect("bosma source WIP");
    let laminatsiya_pause = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: source_batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("laminatsiya start");
    let laminatsiya_source = laminatsiya_pause
        .session
        .as_ref()
        .expect("laminatsiya session");
    assert_eq!(
        laminatsiya_source.payload_json["input_progress_batch_id"].as_str(),
        Some(source_batch.batch_id.as_str())
    );
    let laminatsiya_pause = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(10.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("laminatsiya pause")
        .progress_batch
        .expect("laminatsiya WIP");

    service
        .apply_apparatus_queue_action_with_progress(
            third,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[third.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: laminatsiya_pause.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("rezka start from laminatsiya WIP");
    let paused = service
        .apply_apparatus_queue_action_with_progress(
            third,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[third.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(12.0),
                gross_qty: Some(15.0),
                diameter: Some(45.5),
                uom: "m".to_string(),
                finished_goods_kg: Some(15.0),
                finished_goods_meter: Some(12.0),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("rezka pause");
    assert_eq!(paused.progress_batches.len(), 3);
    assert!(paused.progress_batches.iter().all(|batch| {
        batch.status == OrderProgressBatchStatus::Paused
            && batch.wip_status == OrderProgressBatchWipStatus::Waiting
            && batch.parent_batch_id == laminatsiya_pause.batch_id
    }));

    let resumed = service
        .apply_apparatus_queue_action_with_progress(
            third,
            order_id,
            queue_state::ApparatusQueueAction::Resume,
            &[third.to_string()],
            actor,
            QueueProgressInput::default(),
        )
        .await
        .expect("rezka resume without a frame QR");
    assert_eq!(
        resumed.states.get(order_id),
        Some(&"in_progress".to_string())
    );
    assert_eq!(resumed.progress_batches.len(), 3);
    assert!(resumed.progress_batches.iter().all(|batch| {
        batch.status == OrderProgressBatchStatus::Resumed
            && batch.wip_status == OrderProgressBatchWipStatus::Waiting
    }));
    assert_eq!(
        store
            .progress_batch(&laminatsiya_pause.batch_id)
            .await
            .expect("laminatsiya source lookup")
            .expect("laminatsiya source")
            .wip_status,
        OrderProgressBatchWipStatus::InUse
    );
    for frame in &resumed.progress_batches {
        let persisted = store
            .progress_batch(&frame.batch_id)
            .await
            .expect("frame lookup")
            .expect("persisted frame");
        assert_eq!(persisted.status, OrderProgressBatchStatus::Resumed);
        assert_eq!(persisted.wip_status, OrderProgressBatchWipStatus::Waiting);
    }
}

#[tokio::test]
async fn complete_repairs_legacy_output_input_confusion() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "laminatsiyachi".to_string(),
        ref_: "worker-laminatsiya-legacy-resume".to_string(),
        display_name: "Laminatsiya Legacy Resume".to_string(),
    };
    let order_id = "zakaz-laminatsiya-legacy-resume";
    let apparatus = LAMINATION_1_ID;
    let previous = FLOW_PECHAT_ID;
    let session_id = "session-laminatsiya-legacy-resume";
    let source_batch_id = "source-bosma-batch";
    let misbound_output_id = "batch-laminatsiya-legacy-pause";
    service
        .upsert_map(two_stage_map(order_id, previous, apparatus))
        .await
        .expect("map");
    store
        .put_apparatus_queue_states(
            apparatus,
            BTreeMap::from([(order_id.to_string(), "in_progress".to_string())]),
        )
        .await
        .expect("legacy active queue state");
    store
        .put_order_run_session(OrderRunSession {
            session_id: session_id.to_string(),
            apparatus: apparatus.to_string(),
            order_id: order_id.to_string(),
            status: OrderRunStatus::Active,
            worker_role: actor.role.clone(),
            worker_ref: actor.ref_.clone(),
            worker_display_name: actor.display_name.clone(),
            started_at_unix: 100,
            updated_at_unix: 200,
            payload_json: serde_json::json!({
                "resumed_without_progress_qr": true,
                "input_progress_batch_id": misbound_output_id,
                "input_progress_qr_payload": "legacy-pause-qr",
                "input_progress_apparatus": apparatus,
            }),
        })
        .await
        .expect("legacy active session");
    let mut source_batch = test_progress_batch(
        source_batch_id,
        order_id,
        previous,
        "source-bosma-qr",
        OrderProgressBatchWipStatus::Processed,
        "",
    );
    source_batch.action = queue_state::ApparatusQueueAction::Pause;
    source_batch.status = OrderProgressBatchStatus::Resumed;
    source_batch.next_apparatus = apparatus.to_string();
    source_batch.current_apparatus = apparatus.to_string();
    source_batch.current_apparatus_key = queue_state::apparatus_search_key(apparatus);
    source_batch.current_location = apparatus.to_string();
    source_batch.used_by_session_id = session_id.to_string();
    source_batch.used_by_apparatus = apparatus.to_string();
    source_batch.processed_by_session_id = session_id.to_string();
    source_batch.processed_by_apparatus = apparatus.to_string();
    store
        .put_order_progress_batch(source_batch)
        .await
        .expect("legacy prematurely processed source WIP");
    let mut misbound_output = test_progress_batch(
        misbound_output_id,
        order_id,
        apparatus,
        "legacy-pause-qr",
        OrderProgressBatchWipStatus::InUse,
        source_batch_id,
    );
    misbound_output.session_id = session_id.to_string();
    misbound_output.action = queue_state::ApparatusQueueAction::Pause;
    misbound_output.status = OrderProgressBatchStatus::Resumed;
    misbound_output.current_apparatus = apparatus.to_string();
    misbound_output.current_apparatus_key = queue_state::apparatus_search_key(apparatus);
    misbound_output.current_location = apparatus.to_string();
    misbound_output.used_by_session_id = session_id.to_string();
    misbound_output.used_by_apparatus = apparatus.to_string();
    store
        .put_order_progress_batch(misbound_output)
        .await
        .expect("legacy misbound output WIP");

    let completed = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Complete,
            &[apparatus.to_string()],
            actor,
            QueueProgressInput {
                produced_qty: Some(9.0),
                uom: "kg".to_string(),
                finished_goods_kg: Some(9.0),
                finished_goods_meter: Some(90.0),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("complete repairs legacy input link");

    assert_eq!(
        completed
            .progress_batch
            .as_ref()
            .expect("completed WIP")
            .parent_batch_id,
        source_batch_id
    );
    assert_eq!(
        completed
            .session
            .as_ref()
            .expect("completed session")
            .payload_json["input_progress_batch_id"],
        source_batch_id
    );
    let processed_source = store
        .progress_batch(source_batch_id)
        .await
        .expect("source WIP lookup")
        .expect("source WIP");
    assert_eq!(
        processed_source.wip_status,
        OrderProgressBatchWipStatus::Processed
    );
    let repaired_output = store
        .progress_batch(misbound_output_id)
        .await
        .expect("output WIP lookup")
        .expect("output WIP");
    assert_eq!(
        repaired_output.wip_status,
        OrderProgressBatchWipStatus::Waiting
    );
    assert!(repaired_output.used_by_apparatus.is_empty());
    assert_eq!(
        repaired_output.payload_json["recovered_output_input_confusion"],
        true
    );
}

#[tokio::test]
async fn resume_without_resumable_wip_keeps_queue_paused() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "laminatsiyachi".to_string(),
        ref_: "worker-laminatsiya-ghost-resume".to_string(),
        display_name: "Laminatsiya Ghost Resume".to_string(),
    };
    let order_id = "zakaz-laminatsiya-ghost-resume";
    let apparatus = LAMINATION_1_ID;
    service
        .upsert_map(two_stage_map(order_id, FLOW_PECHAT_ID, apparatus))
        .await
        .expect("map");
    store
        .put_apparatus_queue_states(
            apparatus,
            BTreeMap::from([(order_id.to_string(), "paused".to_string())]),
        )
        .await
        .expect("paused queue state");
    store
        .put_order_run_session(OrderRunSession {
            session_id: "session-laminatsiya-ghost-resume".to_string(),
            apparatus: apparatus.to_string(),
            order_id: order_id.to_string(),
            status: OrderRunStatus::Paused,
            worker_role: actor.role.clone(),
            worker_ref: actor.ref_.clone(),
            worker_display_name: actor.display_name.clone(),
            started_at_unix: 100,
            updated_at_unix: 100,
            payload_json: serde_json::json!({}),
        })
        .await
        .expect("paused session");

    let result = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Resume,
            &[apparatus.to_string()],
            actor,
            QueueProgressInput::default(),
        )
        .await;

    assert_eq!(result, Err(ProductionMapError::ProgressBatchNotResumable));
    assert_eq!(
        service
            .apparatus_queue_states()
            .await
            .expect("queue states")
            .get(apparatus)
            .and_then(|states| states.get(order_id)),
        Some(&"paused".to_string())
    );
}

#[tokio::test]
async fn laminatsiya_astatka_uses_order_timeline_and_previous_report_anchor() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-laminatsiya-astatka".to_string(),
        display_name: "Laminatsiya Astatka Worker".to_string(),
    };
    let order_id = "zakaz-laminatsiya-astatka";
    let apparatus = LAMINATION_1_ID;
    service
        .upsert_map(two_stage_map(order_id, FLOW_PECHAT_ID, apparatus))
        .await
        .expect("map");
    store
        .put_order_run_session(OrderRunSession {
            session_id: "session-laminatsiya-astatka".to_string(),
            apparatus: apparatus.to_string(),
            order_id: order_id.to_string(),
            status: OrderRunStatus::Active,
            worker_role: actor.role.clone(),
            worker_ref: actor.ref_.clone(),
            worker_display_name: actor.display_name.clone(),
            started_at_unix: 100,
            updated_at_unix: 100,
            payload_json: serde_json::json!({}),
        })
        .await
        .expect("session");

    let first = service
        .record_laminatsiya_astatka(
            apparatus,
            order_id,
            actor.clone(),
            Some(1.0),
            Some(2.0),
            Some(0.5),
            None,
            None,
            None,
            "birinchi astatka",
        )
        .await
        .expect("first astatka");
    assert_eq!(first.from_at_unix, 100);
    assert_eq!(first.lamination_print_leftover_rolls, 1.0);

    let second = service
        .record_laminatsiya_astatka(
            apparatus,
            order_id,
            actor,
            Some(0.0),
            Some(0.0),
            Some(0.0),
            None,
            None,
            None,
            "ikkinchi astatka",
        )
        .await
        .expect("second astatka");
    assert_eq!(second.from_at_unix, first.to_at_unix);
    assert!(second.to_at_unix >= second.from_at_unix);
    assert_eq!(
        store
            .laminatsiya_astatka_reports_for_order(order_id)
            .await
            .expect("reports")
            .len(),
        2
    );
}

#[tokio::test]
async fn rezka_astatka_uses_order_timeline_and_accepts_zero_metrics() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-rezka-astatka".to_string(),
        display_name: "Rezka Astatka Worker".to_string(),
    };
    let order_id = "zakaz-rezka-astatka";
    let apparatus = REZKA_ID;
    service
        .upsert_map(two_stage_map(order_id, LAMINATION_1_ID, apparatus))
        .await
        .expect("map");
    store
        .put_order_run_session(OrderRunSession {
            session_id: "session-rezka-astatka".to_string(),
            apparatus: apparatus.to_string(),
            order_id: order_id.to_string(),
            status: OrderRunStatus::Active,
            worker_role: actor.role.clone(),
            worker_ref: actor.ref_.clone(),
            worker_display_name: actor.display_name.clone(),
            started_at_unix: 200,
            updated_at_unix: 200,
            payload_json: serde_json::json!({}),
        })
        .await
        .expect("session");

    let first = service
        .record_rezka_astatka(
            apparatus,
            order_id,
            actor.clone(),
            Some(1.0),
            Some(2.0),
            Some(3.0),
            Some(4.0),
            None,
            None,
            None,
            "birinchi astatka",
        )
        .await
        .expect("first astatka");
    assert_eq!(first.from_at_unix, 200);
    assert_eq!(first.rezka_edge_waste, 4.0);

    let second = service
        .record_rezka_astatka(
            apparatus,
            order_id,
            actor,
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            None,
            None,
            None,
            "ikkinchi astatka",
        )
        .await
        .expect("second astatka");
    assert_eq!(second.from_at_unix, first.to_at_unix);
    assert!(second.to_at_unix >= second.from_at_unix);
    assert_eq!(
        store
            .rezka_astatka_reports_for_order(order_id)
            .await
            .expect("reports")
            .len(),
        2
    );
}

#[tokio::test]
async fn downstream_start_accepts_previous_stage_output_after_producer_resumes() {
    let service = default_service_with_store(Arc::new(MemoryProductionMapStore::new())).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-downstream-resume".to_string(),
        display_name: "Worker Downstream Resume".to_string(),
    };
    let order_id = "zakaz-downstream-resume";
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");

    service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("first start");
    let paused = service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(12.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("first pause");
    let qr_payload = paused
        .progress_batch
        .as_ref()
        .expect("pause batch")
        .qr_payload
        .clone();
    service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Resume,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("first resume");

    let second_started = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                qr_payload: qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("downstream starts resumed producer output");
    assert_eq!(
        second_started.states.get(order_id),
        Some(&"in_progress".to_string())
    );
    let consumed = service
        .progress_batch_for_qr("", &qr_payload)
        .await
        .expect("consumed output lookup");
    assert_eq!(consumed.wip_status, OrderProgressBatchWipStatus::InUse);
    assert_eq!(consumed.used_by_apparatus, second);
}

#[tokio::test]
async fn downstream_start_with_previous_qr_can_skip_pending_sequence_head() {
    let service = default_service_with_store(Arc::new(MemoryProductionMapStore::new())).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-downstream-free".to_string(),
        display_name: "Worker Downstream Free".to_string(),
    };
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    let waiting_order = "zakaz-downstream-waiting";
    let ready_order = "zakaz-downstream-ready";
    service
        .upsert_map(two_stage_map(waiting_order, first, second))
        .await
        .expect("waiting map");
    service
        .upsert_map(two_stage_map(ready_order, first, second))
        .await
        .expect("ready map");
    service
        .set_apparatus_sequence(
            first,
            vec![ready_order.to_string(), waiting_order.to_string()],
        )
        .await
        .expect("first sequence");
    service
        .set_apparatus_sequence(
            second,
            vec![waiting_order.to_string(), ready_order.to_string()],
        )
        .await
        .expect("second sequence");

    service
        .apply_apparatus_queue_action_with_progress(
            first,
            ready_order,
            queue_state::ApparatusQueueAction::Start,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("first start ready order");
    let paused = service
        .apply_apparatus_queue_action_with_progress(
            first,
            ready_order,
            queue_state::ApparatusQueueAction::Pause,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(9.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("first pause ready order");
    let qr_payload = paused
        .progress_batch
        .as_ref()
        .expect("pause batch")
        .qr_payload
        .clone();

    let second_started = service
        .apply_apparatus_queue_action_with_progress(
            second,
            ready_order,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                qr_payload,
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second start skips waiting order with previous qr");
    assert_eq!(
        second_started.states.get(ready_order),
        Some(&"in_progress".to_string())
    );
    assert_ne!(
        second_started.states.get(waiting_order),
        Some(&"in_progress".to_string())
    );
}

#[tokio::test]
async fn downstream_start_marks_previous_stage_batch_in_use() {
    let service = default_service_with_store(Arc::new(MemoryProductionMapStore::new())).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-wip-in-use".to_string(),
        display_name: "Worker WIP In Use".to_string(),
    };
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    let order_id = "zakaz-wip-in-use";
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");
    let first_batch = pause_first_stage_batch(&service, order_id, first, &actor, 21.0)
        .await
        .expect("first batch");
    assert!(
        first_batch
            .label_item_name
            .contains("yarim tayyor mahsulot")
    );
    assert!(!first_batch.is_finished_goods_output());
    assert_eq!(first_batch.next_apparatus, second);
    assert_eq!(first_batch.status_detail.flow_status, "waiting_next_stage");
    let intermediate_receipt = service
        .receive_finished_goods(
            &first_batch.batch_id,
            &first_batch.qr_payload,
            "Tayyor mahsulot ombori",
            QueueActionActor {
                role: "werka".to_string(),
                ref_: "warehouse-intermediate-rejection".to_string(),
                display_name: "Warehouse Worker".to_string(),
            },
        )
        .await;
    assert_eq!(
        intermediate_receipt,
        Err(ProductionMapError::ProgressBatchNotAccepted)
    );

    service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                qr_payload: first_batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second start");

    let updated = service
        .progress_batch_for_qr("", &first_batch.qr_payload)
        .await
        .expect("updated first batch");
    assert_eq!(updated.payload_json["wip_status"], "in_use");
    assert_eq!(updated.payload_json["current_apparatus"], second);
    assert_eq!(updated.payload_json["used_by_order_id"], order_id);
}

#[tokio::test]
async fn legacy_self_consumed_pause_wip_is_available_to_the_next_stage() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "laminatsiyachi".to_string(),
        ref_: "worker-legacy-self-consumed".to_string(),
        display_name: "Legacy Self Consumed".to_string(),
    };
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    let order_id = "zakaz-legacy-self-consumed";
    let batch_id = "legacy-self-consumed-pause-wip";
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");
    let mut batch = test_progress_batch(
        batch_id,
        order_id,
        first,
        "legacy-self-consumed-qr",
        OrderProgressBatchWipStatus::Processed,
        "",
    );
    batch.action = queue_state::ApparatusQueueAction::Pause;
    batch.status = OrderProgressBatchStatus::Resumed;
    batch.next_apparatus = second.to_string();
    batch.used_by_session_id = batch.session_id.clone();
    batch.used_by_apparatus = first.to_string();
    batch.processed_by_session_id = batch.session_id.clone();
    batch.processed_by_apparatus = first.to_string();
    batch.current_apparatus = first.to_string();
    batch.current_apparatus_key = queue_state::apparatus_search_key(first);
    let producer_session_id = batch.session_id.clone();
    store
        .put_order_progress_batch(batch)
        .await
        .expect("legacy WIP");
    let mut sibling = test_progress_batch(
        "legacy-sibling-output",
        order_id,
        first,
        "legacy-sibling-qr",
        OrderProgressBatchWipStatus::Processed,
        batch_id,
    );
    sibling.session_id = producer_session_id;
    sibling.action = queue_state::ApparatusQueueAction::Complete;
    sibling.status = OrderProgressBatchStatus::Completed;
    sibling.next_apparatus = second.to_string();
    store
        .put_order_progress_batch(sibling)
        .await
        .expect("legacy sibling WIP");
    store
        .put_apparatus_queue_states(
            first,
            BTreeMap::from([(order_id.to_string(), "completed".to_string())]),
        )
        .await
        .expect("first stage completed");

    let available = service
        .wip_progress_batches(WipProgressBatchQuery::new(
            first,
            second,
            "",
            Some(OrderProgressBatchWipStatus::Waiting),
            false,
            order_id,
            10,
        ))
        .await
        .expect("available WIP list");
    assert_eq!(available.len(), 1);
    assert_eq!(
        available[0].wip_status,
        OrderProgressBatchWipStatus::Waiting
    );
    let report = service
        .progress_qr_report("", "legacy-self-consumed-qr")
        .await
        .expect("recovered QR report");
    assert!(!report.is_stale);
    assert_eq!(
        report
            .current_batch
            .expect("current recovered WIP")
            .batch_id,
        batch_id
    );

    service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                qr_payload: "legacy-self-consumed-qr".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("next stage claims recovered WIP");
    let claimed = store
        .progress_batch(batch_id)
        .await
        .expect("claimed WIP lookup")
        .expect("claimed WIP");
    assert_eq!(claimed.wip_status, OrderProgressBatchWipStatus::InUse);
    assert_eq!(claimed.used_by_apparatus, second);
    assert!(claimed.processed_by_apparatus.is_empty());
    assert!(
        store
            .progress_batch("legacy-sibling-output")
            .await
            .expect("sibling lookup")
            .expect("sibling")
            .parent_batch_id
            .is_empty()
    );
}

#[tokio::test]
async fn wip_listing_backfills_missing_current_and_next_apparatus_from_map() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-wip-next".to_string(),
        display_name: "Worker WIP Next".to_string(),
    };
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    let order_id = "zakaz-wip-next";
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");
    let mut batch = pause_first_stage_batch(&service, order_id, first, &actor, 21.0)
        .await
        .expect("first batch");
    batch.current_apparatus.clear();
    batch.current_apparatus_key.clear();
    batch.current_location.clear();
    batch.next_apparatus.clear();
    batch.payload_json["current_apparatus"] = serde_json::json!("");
    batch.payload_json["current_apparatus_key"] = serde_json::json!("");
    batch.payload_json["current_location"] = serde_json::json!("");
    batch.payload_json["next_apparatus"] = serde_json::json!("");
    store
        .put_order_progress_batch(batch)
        .await
        .expect("legacy batch update");

    let batches = service
        .wip_progress_batches(WipProgressBatchQuery::new(
            "",
            "",
            "",
            Some(OrderProgressBatchWipStatus::Waiting),
            false,
            order_id,
            10,
        ))
        .await
        .expect("wip batches");

    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].current_apparatus, first);
    assert_eq!(
        batches[0].current_apparatus_key,
        queue_state::apparatus_search_key(first)
    );
    assert_eq!(batches[0].next_apparatus, second);
    assert_eq!(batches[0].payload_json["current_apparatus"], first);
    assert_eq!(batches[0].payload_json["next_apparatus"], second);
}

#[tokio::test]
async fn unassigned_alternative_stage_is_claimed_by_first_started_candidate() {
    let service = default_service_with_store(Arc::new(MemoryProductionMapStore::new())).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-alt-claim".to_string(),
        display_name: "Worker Alt Claim".to_string(),
    };
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    let third = LAMINATION_2_ID;
    let order_id = "zakaz-alt-claim";
    service
        .upsert_map(unassigned_alternative_next_stage_map(
            order_id, first, second, third,
        ))
        .await
        .expect("map");
    let visible = service
        .visible_order_ids_by_apparatus()
        .await
        .expect("visible orders");
    assert_eq!(visible.get(second), Some(&vec![order_id.to_string()]));
    assert_eq!(visible.get(third), Some(&vec![order_id.to_string()]));

    let first_batch = pause_first_stage_batch(&service, order_id, first, &actor, 21.0)
        .await
        .expect("first batch");
    assert_eq!(first_batch.next_apparatus, second);
    let third_wips = service
        .wip_progress_batches(WipProgressBatchQuery::new(
            first,
            third,
            "",
            Some(OrderProgressBatchWipStatus::Waiting),
            false,
            order_id,
            10,
        ))
        .await
        .expect("third wips");
    assert_eq!(third_wips.len(), 1);
    assert_eq!(third_wips[0].batch_id, first_batch.batch_id);

    service
        .apply_apparatus_queue_action_with_progress(
            third,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[third.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: first_batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("third start");

    let saved = service
        .map(order_id)
        .await
        .expect("saved map")
        .expect("map exists");
    let assigned = saved
        .map
        .nodes
        .iter()
        .filter(|node| node.alternative_group_id == "alt_laminatsiya")
        .map(|node| node.alternative_assigned_title.as_str())
        .collect::<Vec<_>>();
    assert_eq!(assigned, vec!["Laminatsiya 2", "Laminatsiya 2"]);
    let visible = service
        .visible_order_ids_by_apparatus()
        .await
        .expect("visible after claim");
    assert_ne!(visible.get(second), Some(&vec![order_id.to_string()]));
    assert_eq!(visible.get(third), Some(&vec![order_id.to_string()]));
}

#[tokio::test]
async fn assigned_alternative_stage_rejects_unselected_candidate_even_with_stale_sequence() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-alt-assigned".to_string(),
        display_name: "Worker Alt Assigned".to_string(),
    };
    let first = FLOW_PECHAT_ID;
    let selected = LAMINATION_1_ID;
    let unselected = LAMINATION_2_ID;
    let order_id = "zakaz-alt-assigned";
    let mut map = unassigned_alternative_next_stage_map(order_id, first, selected, unselected);
    for node in &mut map.nodes {
        if node.alternative_group_id == "alt_laminatsiya" {
            node.alternative_assigned_apparatus_id = selected.to_string();
            node.alternative_assigned_title = "Laminatsiya 1".to_string();
        }
    }
    service.upsert_map(map).await.expect("map");
    store
        .put_apparatus_sequence(unselected, vec![order_id.to_string()])
        .await
        .expect("seed legacy stale sequence");
    let first_batch = pause_first_stage_batch(&service, order_id, first, &actor, 21.0)
        .await
        .expect("first batch");

    let result = service
        .apply_apparatus_queue_action_with_progress(
            unselected,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[unselected.to_string()],
            actor,
            QueueProgressInput {
                qr_payload: first_batch.qr_payload,
                ..QueueProgressInput::default()
            },
        )
        .await;

    assert_eq!(result, Err(ProductionMapError::QueueActionNotAllowed));
}

#[tokio::test]
async fn downstream_output_processes_input_batch_and_links_new_wip_batch() {
    let service = default_service_with_store(Arc::new(MemoryProductionMapStore::new())).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-wip-processed".to_string(),
        display_name: "Worker WIP Processed".to_string(),
    };
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    let order_id = "zakaz-wip-processed";
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");
    let first_batch = pause_first_stage_batch(&service, order_id, first, &actor, 21.0)
        .await
        .expect("first batch");
    service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: first_batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second start");

    let completed = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Complete,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                produced_qty: Some(18.0),
                uom: "kg".to_string(),
                lamination_film_leftover_rolls: Some(1.0),
                total_waste: Some(0.5),
                finished_goods_kg: Some(18.0),
                finished_goods_meter: Some(120.0),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second complete");

    let input = service
        .progress_batch_for_qr("", &first_batch.qr_payload)
        .await
        .expect("processed first batch");
    assert_eq!(input.payload_json["wip_status"], "processed");
    assert_eq!(input.payload_json["processed_by_apparatus"], second);
    assert_eq!(input.status_detail.flow_status, "consumed_by_next_stage");

    let output = completed.progress_batch.expect("second output batch");
    assert_eq!(output.payload_json["wip_status"], "waiting");
    assert_eq!(output.payload_json["parent_batch_id"], first_batch.batch_id);
    assert_eq!(output.payload_json["from_apparatus"], second);
    assert_eq!(output.status_detail.work_status, "completed");
    assert_eq!(output.status_detail.flow_status, "free_wip");
    assert!(output.status_detail.stock_status.is_empty());
}

#[tokio::test]
async fn downstream_complete_keeps_order_open_until_all_input_wips_processed() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-wip-partial-complete".to_string(),
        display_name: "Worker WIP Partial Complete".to_string(),
    };
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    let order_id = "zakaz-wip-partial-complete";
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");

    let first_pause = pause_first_stage_batch(&service, order_id, first, &actor, 11.0)
        .await
        .expect("first pause batch");
    let mut second_pause = test_progress_batch(
        "progress-batch-second-wip",
        order_id,
        first,
        "QR-SECOND-WIP",
        OrderProgressBatchWipStatus::Waiting,
        "",
    );
    second_pause.produced_qty = 12.0;
    second_pause.next_apparatus = second.to_string();
    second_pause.refresh_status_detail();
    store
        .put_order_progress_batch(second_pause.clone())
        .await
        .expect("second waiting wip");
    store
        .put_apparatus_queue_states(
            first,
            BTreeMap::from([(order_id.to_string(), "completed".to_string())]),
        )
        .await
        .expect("first stage completed after producing all wips");

    service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: first_pause.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second start first wip");
    let partial_complete = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Complete,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(11.0),
                uom: "kg".to_string(),
                finished_goods_kg: Some(11.0),
                finished_goods_meter: Some(110.0),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second complete first wip");
    let first_final_output = partial_complete
        .progress_batch
        .clone()
        .expect("first final output batch");

    assert_eq!(
        partial_complete.states.get(order_id),
        Some(&"pending".to_string())
    );
    let partial_history = service
        .completed_queue_orders_for_actor(&actor.ref_, 10)
        .await
        .expect("partial completed orders");
    assert_eq!(partial_history.len(), 1);
    assert_eq!(partial_history[0].order_id, order_id);
    assert_eq!(partial_history[0].apparatus, second);
    assert_eq!(
        partial_history[0].status,
        CompletedQueueOrderStatus::InProgress
    );
    let partial_order_status = service
        .order_status_detail(order_id)
        .await
        .expect("partial order status");
    assert_eq!(partial_order_status.order_status, "partially_completed");
    assert_eq!(partial_order_status.waiting_wip_count, 2);
    assert_eq!(partial_order_status.waiting_next_stage_count, 1);
    assert_eq!(partial_order_status.free_wip_count, 1);
    assert_eq!(partial_order_status.processed_wip_count, 1);
    assert_eq!(
        service
            .progress_batch_for_qr("", &first_pause.qr_payload)
            .await
            .expect("first wip processed")
            .wip_status,
        OrderProgressBatchWipStatus::Processed
    );
    let reused_processed = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: first_pause.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await;
    assert_eq!(
        reused_processed,
        Err(ProductionMapError::ProgressBatchNotAccepted)
    );
    assert_eq!(
        service
            .progress_batch_for_qr("", &second_pause.qr_payload)
            .await
            .expect("second wip still waiting")
            .status_detail
            .flow_status,
        "waiting_next_stage"
    );

    service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: second_pause.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second start final wip");
    let final_complete = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Complete,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(12.0),
                uom: "kg".to_string(),
                lamination_film_leftover_rolls: Some(1.0),
                total_waste: Some(0.5),
                finished_goods_kg: Some(12.0),
                finished_goods_meter: Some(120.0),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second complete final wip");

    assert_eq!(
        final_complete.states.get(order_id),
        Some(&"completed".to_string())
    );
    let final_order_status = service
        .order_status_detail(order_id)
        .await
        .expect("final order status");
    assert_eq!(final_order_status.order_status, "completed");
    assert_eq!(final_order_status.flow_status, "free_wip");
    assert!(final_order_status.stock_status.is_empty());
    assert_eq!(final_order_status.free_wip_count, 2);
    let final_output = final_complete
        .progress_batch
        .expect("final output batch has status detail");
    assert_eq!(final_output.status_detail.work_status, "completed");
    assert_eq!(final_output.status_detail.flow_status, "free_wip");
    assert!(final_output.status_detail.stock_status.is_empty());

    let aparatchi_receive = service
        .receive_finished_goods(
            &first_final_output.batch_id,
            &first_final_output.qr_payload,
            "Tayyor mahsulot ombori",
            actor.clone(),
        )
        .await;
    assert_eq!(
        aparatchi_receive,
        Err(ProductionMapError::QueueActionNotAllowed)
    );

    let warehouse_actor = QueueActionActor {
        role: "werka".to_string(),
        ref_: "warehouse-worker".to_string(),
        display_name: "Warehouse Worker".to_string(),
    };
    service
        .receive_finished_goods(
            &first_final_output.batch_id,
            &first_final_output.qr_payload,
            "Tayyor mahsulot ombori",
            warehouse_actor.clone(),
        )
        .await
        .expect("receive first final output");
    let received = service
        .receive_finished_goods(
            &final_output.batch_id,
            &final_output.qr_payload,
            "Tayyor mahsulot ombori",
            warehouse_actor,
        )
        .await
        .expect("receive second final output");
    assert_eq!(received.order_status.order_status, "completed");
    assert_eq!(received.order_status.flow_status, "accepted_to_stock");
    assert_eq!(received.order_status.stock_status, "accepted");
    assert_eq!(received.order_status.accepted_wip_count, 2);
}

#[tokio::test]
async fn downstream_start_rejects_mismatched_progress_batch_id_and_qr() {
    let service = default_service_with_store(Arc::new(MemoryProductionMapStore::new())).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-downstream-mismatch".to_string(),
        display_name: "Worker Downstream Mismatch".to_string(),
    };
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    let first_order = "zakaz-downstream-match";
    let second_order = "zakaz-downstream-other";
    service
        .upsert_map(two_stage_map(first_order, first, second))
        .await
        .expect("first map");
    service
        .upsert_map(two_stage_map(second_order, first, second))
        .await
        .expect("second map");
    service
        .set_apparatus_sequence(
            first,
            vec![first_order.to_string(), second_order.to_string()],
        )
        .await
        .expect("first sequence");

    let first_batch = pause_first_stage_batch(&service, first_order, first, &actor, 11.0)
        .await
        .expect("first batch");
    service
        .apply_apparatus_queue_action_with_progress(
            first,
            first_order,
            queue_state::ApparatusQueueAction::Resume,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: first_batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("first resume");
    service
        .apply_apparatus_queue_action_with_progress(
            first,
            first_order,
            queue_state::ApparatusQueueAction::Complete,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(11.0),
                uom: "kg".to_string(),
                return_ink_kg: Some(0.1),
                total_waste: Some(0.1),
                finished_goods_kg: Some(11.0),
                finished_goods_meter: Some(110.0),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("first complete");
    let second_batch = pause_first_stage_batch(&service, second_order, first, &actor, 12.0)
        .await
        .expect("second batch");

    let rejected = service
        .apply_apparatus_queue_action_with_progress(
            second,
            first_order,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                progress_batch_id: first_batch.batch_id,
                qr_payload: second_batch.qr_payload,
                ..QueueProgressInput::default()
            },
        )
        .await;
    assert_eq!(rejected, Err(ProductionMapError::ProgressBatchNotFound));
}

#[tokio::test]
async fn downstream_start_requires_qr_payload_not_only_progress_batch_id() {
    let service = default_service_with_store(Arc::new(MemoryProductionMapStore::new())).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-downstream-id-only".to_string(),
        display_name: "Worker Downstream Id Only".to_string(),
    };
    let first = FLOW_PECHAT_ID;
    let second = LAMINATION_1_ID;
    let order_id = "zakaz-downstream-id-only";
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");
    let batch = pause_first_stage_batch(&service, order_id, first, &actor, 8.0)
        .await
        .expect("batch");

    let rejected = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                progress_batch_id: batch.batch_id,
                ..QueueProgressInput::default()
            },
        )
        .await;
    assert_eq!(rejected, Err(ProductionMapError::ProgressQrRequired));
}

#[tokio::test]
async fn upsert_maps_batch_keeps_queue_state_and_sequence_cache() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    store
        .put_apparatus_sequence(
            REZKA_ID,
            vec!["zakaz-111".to_string(), "zakaz-222".to_string()],
        )
        .await
        .expect("seed sequence cache");
    service
        .store
        .put_apparatus_queue_states(
            REZKA_ID,
            BTreeMap::from([("zakaz-111".to_string(), "completed".to_string())]),
        )
        .await
        .expect("queue state");
    let mut first = sample_map();
    first.id = "zakaz-111".to_string();
    first.order_number = "111".to_string();
    first.code = "111".to_string();
    let mut second = sample_map();
    second.id = "zakaz-222".to_string();
    second.order_number = "222".to_string();
    second.code = "222".to_string();

    let saved = service
        .upsert_maps_batch(vec![first, second])
        .await
        .expect("batch upsert");

    assert_eq!(saved.len(), 2);
    assert_eq!(service.maps().await.expect("maps").len(), 2);
    assert_eq!(
        service
            .apparatus_sequences()
            .await
            .expect("sequences")
            .get(REZKA_ID),
        Some(&vec!["zakaz-111".to_string(), "zakaz-222".to_string()])
    );
    assert_eq!(
        service
            .apparatus_queue_states()
            .await
            .expect("states")
            .get(REZKA_ID)
            .and_then(|states| states.get("zakaz-111")),
        Some(&"completed".to_string())
    );
}

#[tokio::test]
async fn progress_qr_report_uses_child_batch_as_current_even_when_scanned_batch_sorts_first() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let order_id = "zakaz-qr-lineage";
    service
        .upsert_map(two_stage_map(order_id, FLOW_PECHAT_ID, LAMINATION_1_ID))
        .await
        .expect("map");

    let scanned = test_progress_batch(
        "progress-batch:999:flow-pechat:zakaz-qr-lineage:pause",
        order_id,
        FLOW_PECHAT_ID,
        "QR-OLD",
        OrderProgressBatchWipStatus::Processed,
        "",
    );
    let current = test_progress_batch(
        "progress-batch:100:flow-lamination:zakaz-qr-lineage:complete",
        order_id,
        LAMINATION_1_ID,
        "QR-NEW",
        OrderProgressBatchWipStatus::Waiting,
        &scanned.batch_id,
    );
    store
        .put_order_progress_batch(scanned.clone())
        .await
        .expect("scanned batch");
    store
        .put_order_progress_batch(current)
        .await
        .expect("current batch");

    let report = service
        .progress_qr_report("", &scanned.qr_payload)
        .await
        .expect("report");

    assert_eq!(
        report
            .current_batch
            .as_ref()
            .map(|batch| batch.qr_payload.as_str()),
        Some("QR-NEW")
    );
    assert!(report.is_stale);
    assert_eq!(report.stale_reason, "processed_by_next_stage");
}

#[tokio::test]
async fn progress_qr_report_keeps_lineage_when_order_has_more_than_500_batches() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let order_id = "zakaz-qr-long-lineage";
    service
        .upsert_map(two_stage_map(order_id, FLOW_PECHAT_ID, LAMINATION_1_ID))
        .await
        .expect("map");

    let scanned = test_progress_batch(
        "progress-batch:999:flow-pechat:zakaz-qr-long-lineage:pause",
        order_id,
        FLOW_PECHAT_ID,
        "QR-LONG-OLD",
        OrderProgressBatchWipStatus::Processed,
        "",
    );
    let current = test_progress_batch(
        "progress-batch:001:flow-lamination:zakaz-qr-long-lineage:complete",
        order_id,
        LAMINATION_1_ID,
        "QR-LONG-NEW",
        OrderProgressBatchWipStatus::Waiting,
        &scanned.batch_id,
    );
    store
        .put_order_progress_batch(scanned.clone())
        .await
        .expect("scanned batch");
    store
        .put_order_progress_batch(current)
        .await
        .expect("current batch");
    for index in 0..501 {
        store
            .put_order_progress_batch(test_progress_batch(
                &format!(
                    "progress-batch:{:03}:flow-pechat:zakaz-qr-long-lineage:filler",
                    index + 100
                ),
                order_id,
                FLOW_PECHAT_ID,
                &format!("QR-LONG-FILLER-{index}"),
                OrderProgressBatchWipStatus::Waiting,
                "",
            ))
            .await
            .expect("filler batch");
    }

    let report = service
        .progress_qr_report("", &scanned.qr_payload)
        .await
        .expect("report");

    assert_eq!(
        report
            .current_batch
            .as_ref()
            .map(|batch| batch.qr_payload.as_str()),
        Some("QR-LONG-NEW")
    );
    assert!(report.progress_batches.len() > 500);
}

#[tokio::test]
async fn roll_complete_final_output_can_be_received_as_finished_goods() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let worker = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-roll-complete-receipt".to_string(),
        display_name: "Roll Complete Worker".to_string(),
    };
    let order_id = "zakaz-roll-complete-receipt";
    let apparatus = REZKA_ID;
    let mut map = apparatus_stage_map(order_id, apparatus);
    map.nodes
        .iter_mut()
        .find(|node| node.kind == ProductionMapNodeKind::Apparatus)
        .expect("rezka node")
        .rezka_kadr_count = Some(1);
    service.upsert_map(map).await.expect("map");

    service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[apparatus.to_string()],
            worker.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("start rezka");
    let mut output = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::DetachRoll,
            &[apparatus.to_string()],
            worker.clone(),
            QueueProgressInput {
                produced_qty: Some(90.0),
                gross_qty: Some(11.0),
                diameter: Some(45.5),
                uom: "m".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("pause rezka")
        .progress_batch
        .expect("source batch");
    output.action = queue_state::ApparatusQueueAction::RollComplete;
    output.status = OrderProgressBatchStatus::Completed;
    output.wip_status = OrderProgressBatchWipStatus::Waiting;
    output.finished_goods_kg = Some(11.0);
    output.finished_goods_meter = Some(90.0);
    output.refresh_status_detail();
    store
        .put_order_progress_batch(output.clone())
        .await
        .expect("final roll output");

    let warehouse = QueueActionActor {
        role: "werka".to_string(),
        ref_: "warehouse-roll-complete-receipt".to_string(),
        display_name: "Warehouse Worker".to_string(),
    };
    let receipt = service
        .receive_finished_goods(
            &output.batch_id,
            &output.qr_payload,
            "Tayyor mahsulot ombori",
            warehouse,
        )
        .await
        .expect("receive roll-complete output");
    assert_eq!(
        receipt.batch.action,
        queue_state::ApparatusQueueAction::RollComplete
    );
    assert_eq!(
        receipt.batch.wip_status,
        OrderProgressBatchWipStatus::Processed
    );
    assert_eq!(receipt.batch.status_detail.flow_status, "accepted_to_stock");
}

#[tokio::test]
async fn pause_final_output_can_be_received_as_finished_goods() {
    let service = default_service_with_store(Arc::new(MemoryProductionMapStore::new())).await;
    let worker = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-pause-receipt".to_string(),
        display_name: "Pause Worker".to_string(),
    };
    let order_id = "zakaz-pause-receipt";
    let apparatus = REZKA_ID;
    let mut map = apparatus_stage_map(order_id, apparatus);
    map.nodes
        .iter_mut()
        .find(|node| node.kind == ProductionMapNodeKind::Apparatus)
        .expect("rezka node")
        .rezka_kadr_count = Some(1);
    service.upsert_map(map).await.expect("map");
    service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[apparatus.to_string()],
            worker.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("start final stage");
    let output = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[apparatus.to_string()],
            worker,
            QueueProgressInput {
                produced_qty: Some(11.0),
                gross_qty: Some(11.0),
                diameter: Some(45.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("take finished output")
        .progress_batch
        .expect("pause output");

    assert!(output.is_finished_goods_output());
    assert_eq!(output.status_detail.flow_status, "free_wip");
    assert_eq!(
        service
            .order_status_detail(order_id)
            .await
            .expect("paused order status")
            .order_status,
        "paused"
    );

    let warehouse = QueueActionActor {
        role: "werka".to_string(),
        ref_: "warehouse-pause-receipt".to_string(),
        display_name: "Warehouse Worker".to_string(),
    };
    let receipt = service
        .receive_finished_goods(
            &output.batch_id,
            &output.qr_payload,
            "Tayyor mahsulot ombori",
            warehouse,
        )
        .await
        .expect("receive pause output");
    assert_eq!(
        receipt.batch.wip_status,
        OrderProgressBatchWipStatus::Processed
    );
    assert_eq!(receipt.batch.status_detail.flow_status, "accepted_to_stock");
    assert_eq!(receipt.stock.qty, 11.0);
    assert_eq!(receipt.stock.uom, "kg");
    assert_eq!(receipt.order_status.order_status, "paused");
    assert_eq!(receipt.order_status.flow_status, "accepted_to_stock");
}

async fn pause_first_stage_batch(
    service: &ProductionMapService,
    order_id: &str,
    first: &str,
    actor: &QueueActionActor,
    qty: f64,
) -> Result<OrderProgressBatch, ProductionMapError> {
    let assigned_apparatus = [first.to_string()];
    service
        .apply_apparatus_queue_action_with_material_scan_and_progress(MaterialScanProgressAction {
            apparatus: first,
            order_id,
            action: queue_state::ApparatusQueueAction::Start,
            assigned_apparatus: &assigned_apparatus,
            actor: actor.clone(),
            material_barcode: "",
            state_material_barcodes: &[],
            progress: QueueProgressInput::default(),
            qolip_validation: ApparatusId::new(first.to_string()).ok().and_then(|id| {
                TrustedQolipStartValidation::from_preparations(
                    &id,
                    order_id,
                    &[QolipOrderStartPreparation {
                        spec: QolipProductSpec {
                            qolip_code: "QOLIP-FLOW-TEST".to_string(),
                            ..QolipProductSpec::default()
                        },
                        checkout: None,
                    }],
                )
            }),
        })
        .await?;
    let paused = service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(qty),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await?;
    paused
        .progress_batch
        .ok_or(ProductionMapError::ProgressBatchNotFound)
}

async fn start_first_stage(
    service: &ProductionMapService,
    order_id: &str,
    apparatus: &str,
    actor: QueueActionActor,
) -> Result<ApparatusQueueActionResult, ProductionMapError> {
    let assigned_apparatus = [apparatus.to_string()];
    let apparatus_id =
        ApparatusId::new(apparatus.to_string()).map_err(|_| ProductionMapError::MissingId)?;
    let qolip_validation = TrustedQolipStartValidation::from_preparations(
        &apparatus_id,
        order_id,
        &[QolipOrderStartPreparation {
            spec: QolipProductSpec {
                qolip_code: "QOLIP-FLOW-TEST".to_string(),
                ..QolipProductSpec::default()
            },
            checkout: None,
        }],
    );
    service
        .apply_apparatus_queue_action_with_material_scan_and_progress(MaterialScanProgressAction {
            apparatus,
            order_id,
            action: queue_state::ApparatusQueueAction::Start,
            assigned_apparatus: &assigned_apparatus,
            actor,
            material_barcode: "",
            state_material_barcodes: &[],
            progress: QueueProgressInput::default(),
            qolip_validation,
        })
        .await
}

fn test_progress_batch(
    batch_id: &str,
    order_id: &str,
    apparatus: &str,
    qr_payload: &str,
    wip_status: OrderProgressBatchWipStatus,
    parent_batch_id: &str,
) -> OrderProgressBatch {
    OrderProgressBatch {
        batch_id: batch_id.to_string(),
        revision: 1,
        session_id: format!("session-{batch_id}"),
        started_at_unix: 0,
        completed_at_unix: 0,
        apparatus: apparatus.to_string(),
        order_id: order_id.to_string(),
        action: queue_state::ApparatusQueueAction::Complete,
        status: OrderProgressBatchStatus::Completed,
        produced_qty: 1.0,
        uom: "kg".to_string(),
        qr_payload: qr_payload.to_string(),
        label_item_code: order_id.to_string(),
        label_item_name: order_id.to_string(),
        executor_name: "Worker".to_string(),
        worker_role: "aparatchi".to_string(),
        worker_ref: "worker".to_string(),
        worker_display_name: "Worker".to_string(),
        wip_status,
        status_detail: OrderProgressBatchStatusDetail::default(),
        current_apparatus: apparatus.to_string(),
        current_apparatus_key: queue_state::apparatus_search_key(apparatus),
        current_location: apparatus.to_string(),
        next_apparatus: String::new(),
        parent_batch_id: parent_batch_id.to_string(),
        used_by_session_id: String::new(),
        used_by_apparatus: String::new(),
        processed_by_session_id: String::new(),
        processed_by_apparatus: String::new(),
        return_ink_kg: None,
        lamination_print_leftover_rolls: None,
        lamination_film_leftover_rolls: None,
        rezka_bosma_waste: None,
        rezka_lamination_waste: None,
        rezka_edge_waste: None,
        total_waste: None,
        finished_goods_kg: None,
        bobina_kg: None,
        finished_goods_meter: None,
        diameter: None,
        description: String::new(),
        payload_json: serde_json::json!({}),
    }
}

#[tokio::test]
async fn progress_batch_correction_updates_owned_waiting_batch_with_revision() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-correction".to_string(),
        display_name: "Correction Worker".to_string(),
    };
    let mut waiting = test_progress_batch(
        "batch-correction",
        "order-correction",
        REZKA_ID,
        "qr-correction",
        OrderProgressBatchWipStatus::Waiting,
        "",
    );
    waiting.worker_ref = actor.ref_.clone();
    waiting.produced_qty = 10.0;
    waiting.uom = "m".to_string();
    store
        .put_order_progress_batch(waiting)
        .await
        .expect("seed waiting batch");

    let corrected = service
        .correct_progress_batch(
            ProgressBatchCorrectionInput {
                batch_id: "batch-correction".to_string(),
                expected_revision: 1,
                produced_qty: 12.5,
                uom: "m".to_string(),
                return_ink_kg: None,
                lamination_print_leftover_rolls: None,
                lamination_film_leftover_rolls: None,
                rezka_bosma_waste: Some(0.5),
                rezka_lamination_waste: None,
                rezka_edge_waste: None,
                total_waste: None,
                finished_goods_kg: Some(8.0),
                bobina_kg: Some(1.0),
                finished_goods_meter: Some(12.5),
                diameter: Some(200.0),
                description: "to'g'rilandi".to_string(),
                reason: "O'lchov noto'g'ri kiritilgan".to_string(),
            },
            &actor,
        )
        .await
        .expect("correct waiting batch");
    assert_eq!(corrected.revision, 2);
    assert_eq!(corrected.produced_qty, 12.5);
    assert_eq!(corrected.finished_goods_kg, Some(8.0));
    let corrections = store.progress_batch_correction_records().await;
    assert_eq!(corrections.len(), 1);
    assert_eq!(corrections[0].previous_revision, 1);
    assert_eq!(corrections[0].new_revision, 2);
    assert_eq!(corrections[0].old_values["produced_qty"], 10.0);
    assert_eq!(corrections[0].new_values["produced_qty"], 12.5);

    let stale = service
        .correct_progress_batch(
            ProgressBatchCorrectionInput {
                batch_id: "batch-correction".to_string(),
                expected_revision: 1,
                produced_qty: 13.0,
                uom: "m".to_string(),
                return_ink_kg: None,
                lamination_print_leftover_rolls: None,
                lamination_film_leftover_rolls: None,
                rezka_bosma_waste: Some(0.5),
                rezka_lamination_waste: None,
                rezka_edge_waste: None,
                total_waste: None,
                finished_goods_kg: Some(8.0),
                bobina_kg: Some(1.0),
                finished_goods_meter: Some(12.5),
                diameter: Some(200.0),
                description: "to'g'rilandi".to_string(),
                reason: "Yana o'lchandi".to_string(),
            },
            &actor,
        )
        .await;
    assert_eq!(
        stale,
        Err(ProductionMapError::ProgressBatchCorrectionConflict)
    );
}

#[tokio::test]
async fn progress_batch_correction_rejects_in_use_or_other_workers_batch() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let owner = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-owner".to_string(),
        display_name: "Owner".to_string(),
    };
    let mut in_use = test_progress_batch(
        "batch-in-use-correction",
        "order-correction",
        LAMINATION_1_ID,
        "qr-in-use-correction",
        OrderProgressBatchWipStatus::InUse,
        "",
    );
    in_use.worker_ref = owner.ref_.clone();
    store
        .put_order_progress_batch(in_use)
        .await
        .expect("seed in-use batch");
    let input = ProgressBatchCorrectionInput {
        batch_id: "batch-in-use-correction".to_string(),
        expected_revision: 1,
        produced_qty: 2.0,
        uom: "kg".to_string(),
        return_ink_kg: None,
        lamination_print_leftover_rolls: None,
        lamination_film_leftover_rolls: None,
        rezka_bosma_waste: None,
        rezka_lamination_waste: None,
        rezka_edge_waste: None,
        total_waste: None,
        finished_goods_kg: None,
        bobina_kg: None,
        finished_goods_meter: None,
        diameter: None,
        description: String::new(),
        reason: "Test correction".to_string(),
    };
    assert_eq!(
        service.correct_progress_batch(input.clone(), &owner).await,
        Err(ProductionMapError::ProgressBatchCorrectionLocked)
    );
    let other = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-other".to_string(),
        display_name: "Other".to_string(),
    };
    assert_eq!(
        service.correct_progress_batch(input, &other).await,
        Err(ProductionMapError::ProgressBatchCorrectionForbidden)
    );
}

fn two_stage_map(id: &str, first: &str, second: &str) -> ProductionMapDefinition {
    let mut map = apparatus_stage_map(id, first);
    map.nodes.insert(
        2,
        ProductionMapNode {
            id: "second".to_string(),
            kind: ProductionMapNodeKind::Apparatus,
            title: second.to_string(),
            apparatus_id: second.to_string(),
            formula: None,
            role_code: String::new(),
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: String::new(),
            alternative_group_label: String::new(),
            alternative_assigned_title: String::new(),
            alternative_assigned_apparatus_id: String::new(),
            rezka_kadr_count: None,
            rezka_label_length: None,
            x: 0.0,
            y: 264.0,
        },
    );
    if let Some(end) = map.nodes.iter_mut().find(|node| node.id == "end") {
        end.y = 396.0;
    }
    map.edges = vec![
        ProductionMapEdge {
            from: "start".to_string(),
            to: "apparatus".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "apparatus".to_string(),
            to: "second".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "second".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
    ];
    map
}

fn three_stage_map(
    id: &str,
    first: &str,
    second: &str,
    third: &str,
    rezka_kadr_count: i64,
) -> ProductionMapDefinition {
    let mut map = two_stage_map(id, first, second);
    map.nodes.insert(
        3,
        ProductionMapNode {
            id: "third".to_string(),
            kind: ProductionMapNodeKind::Apparatus,
            title: third.to_string(),
            apparatus_id: third.to_string(),
            formula: None,
            role_code: String::new(),
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: String::new(),
            alternative_group_label: String::new(),
            alternative_assigned_title: String::new(),
            alternative_assigned_apparatus_id: String::new(),
            rezka_kadr_count: Some(rezka_kadr_count),
            rezka_label_length: None,
            x: 0.0,
            y: 396.0,
        },
    );
    if let Some(end) = map.nodes.iter_mut().find(|node| node.id == "end") {
        end.y = 528.0;
    }
    map.edges = vec![
        ProductionMapEdge {
            from: "start".to_string(),
            to: "apparatus".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "apparatus".to_string(),
            to: "second".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "second".to_string(),
            to: "third".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "third".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
    ];
    map
}

fn unassigned_alternative_next_stage_map(
    id: &str,
    first: &str,
    second: &str,
    third: &str,
) -> ProductionMapDefinition {
    let mut map = apparatus_stage_map(id, first);
    map.nodes.insert(
        2,
        ProductionMapNode {
            id: "second".to_string(),
            kind: ProductionMapNodeKind::Apparatus,
            title: if second == LAMINATION_1_ID {
                "Laminatsiya 1".to_string()
            } else {
                second.to_string()
            },
            apparatus_id: second.to_string(),
            formula: None,
            role_code: String::new(),
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: "alt_laminatsiya".to_string(),
            alternative_group_label: "Laminatsiya".to_string(),
            alternative_assigned_title: String::new(),
            alternative_assigned_apparatus_id: String::new(),
            rezka_kadr_count: None,
            rezka_label_length: None,
            x: 0.0,
            y: 264.0,
        },
    );
    map.nodes.insert(
        3,
        ProductionMapNode {
            id: "third".to_string(),
            kind: ProductionMapNodeKind::Apparatus,
            title: if third == LAMINATION_2_ID {
                "Laminatsiya 2".to_string()
            } else {
                third.to_string()
            },
            apparatus_id: third.to_string(),
            formula: None,
            role_code: String::new(),
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: "alt_laminatsiya".to_string(),
            alternative_group_label: "Laminatsiya".to_string(),
            alternative_assigned_title: String::new(),
            alternative_assigned_apparatus_id: String::new(),
            rezka_kadr_count: None,
            rezka_label_length: None,
            x: 180.0,
            y: 264.0,
        },
    );
    if let Some(end) = map.nodes.iter_mut().find(|node| node.id == "end") {
        end.y = 396.0;
    }
    map.edges = vec![
        ProductionMapEdge {
            from: "start".to_string(),
            to: "apparatus".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "apparatus".to_string(),
            to: "second".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "apparatus".to_string(),
            to: "third".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "second".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "third".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
    ];
    map
}
