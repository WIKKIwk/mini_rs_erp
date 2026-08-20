use std::collections::BTreeMap;
use std::sync::Arc;

use crate::core::apparatus_groups::{
    ApparatusGroupService, ApparatusMasterData, ApparatusUpsert, MemoryApparatusGroupStore,
};
use crate::core::apparatus_standard::{
    ApparatusClassification, ApparatusDisplayMetadata, ApparatusFamily, ApparatusId,
    ApparatusIdentity, ApparatusKind, CanonicalApparatus, CapabilityCode, CapabilityProfile,
    CapacityConfiguration, CatalogSource, OperationalPolicies, QueuePolicy, ToolingPolicy,
    TrainingReference, Versioning, aas_package_metadata_for_apparatus,
};
use crate::core::production_map::*;
use crate::core::qolip::{QolipOrderStartPreparation, QolipProductSpec};

use super::fixtures::apparatus_stage_map;

const FLEXO_ID: &str = "apparatus:catalog:asset-005";
const RESERVE_ID: &str = "apparatus:catalog:asset-006";
const REZKA_ID: &str = "apparatus:catalog:asset-010";
const FLEXO_NAME: &str = "Flexo capacity test";
const RESERVE_NAME: &str = "Flexo reserve capacity test";
const REZKA_NAME: &str = "Rezka capacity test";
const START: i64 = 1_700_000_040;

fn apparatus_id(value: &str) -> ApparatusId {
    ApparatusId::new(value).expect("canonical apparatus id")
}

fn canonical_apparatus(
    id: &str,
    display_name: &str,
    setup_minutes: u32,
    cleanup_minutes: u32,
    finite_capacity: bool,
) -> Arc<CanonicalApparatus> {
    let is_rezka = id == REZKA_ID;
    let id = apparatus_id(id);
    let (family, kind, capabilities, capability_profiles, tooling) = if is_rezka {
        (
            ApparatusFamily::Rezka,
            ApparatusKind::Rezka,
            vec![CapabilityCode::Cut],
            Vec::new(),
            ToolingPolicy::QolipScanNotRequired,
        )
    } else {
        (
            ApparatusFamily::Pechat,
            ApparatusKind::Flexo,
            vec![
                CapabilityCode::Print,
                CapabilityCode::Pechat,
                CapabilityCode::Flexo,
            ],
            vec![CapabilityProfile {
                code: CapabilityCode::Flexo,
                level: 3,
                valid_from_unix: None,
                valid_to_unix: None,
                enabled: true,
            }],
            ToolingPolicy::QolipScanRequired,
        )
    };
    Arc::new(CanonicalApparatus {
        identity: ApparatusIdentity {
            id: id.clone(),
            display: ApparatusDisplayMetadata {
                display_name: display_name.to_string(),
                description: String::new(),
                catalog_order: 1,
            },
        },
        classification: ApparatusClassification {
            family,
            kind,
            color_stations: None,
        },
        capabilities,
        capability_profiles,
        policies: OperationalPolicies {
            queue: QueuePolicy::StrictSequence,
            material: Default::default(),
            tooling,
        },
        capacity: CapacityConfiguration {
            capacity_slots: 1,
            setup_minutes,
            cleanup_minutes,
            efficiency_percent: 100,
            finite_capacity,
            working_windows: Vec::new(),
        },
        placement: None,
        training: TrainingReference { enabled: true },
        provenance: crate::core::apparatus_standard::Provenance {
            source: CatalogSource::Custom,
            source_ref: None,
        },
        versioning: Versioning { revision: 1 },
        aas: aas_package_metadata_for_apparatus(&id),
    })
}

async fn test_service() -> (ProductionMapService, ApparatusGroupService) {
    test_service_with_store(Arc::new(MemoryProductionMapStore::new())).await
}

async fn test_service_with_store(
    store: Arc<MemoryProductionMapStore>,
) -> (ProductionMapService, ApparatusGroupService) {
    let mut apparatus = BTreeMap::new();
    for (id, name, setup, cleanup) in [
        (FLEXO_ID, FLEXO_NAME, 5, 5),
        (RESERVE_ID, RESERVE_NAME, 0, 0),
        (REZKA_ID, REZKA_NAME, 0, 0),
    ] {
        let canonical = canonical_apparatus(id, name, setup, cleanup, true);
        apparatus.insert(canonical.identity.id.clone(), canonical);
    }
    let apparatus_groups = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));
    for canonical in apparatus.values() {
        apparatus_groups
            .upsert_apparatus(ApparatusUpsert {
                id: Some(canonical.identity.id.to_string()),
                name: canonical.identity.display.display_name.clone(),
                master: ApparatusMasterData::default(),
            })
            .await
            .expect("seed apparatus catalog");
        let mut canonical = (**canonical).clone();
        canonical.versioning.revision = 2;
        apparatus_groups
            .put_canonical_apparatus(1, canonical)
            .await
            .expect("seed canonical apparatus");
    }
    let service = ProductionMapService::new(store)
        .with_canonical_apparatus_resolver(Arc::new(ApparatusGroupCanonicalResolver::new(
            apparatus_groups.clone(),
        )));
    (service, apparatus_groups)
}

async fn unlimited_test_service() -> (ProductionMapService, ApparatusGroupService) {
    let mut apparatus = BTreeMap::new();
    let canonical = canonical_apparatus(FLEXO_ID, FLEXO_NAME, 5, 5, false);
    apparatus.insert(canonical.identity.id.clone(), canonical);
    let apparatus_groups = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));
    for canonical in apparatus.values() {
        apparatus_groups
            .upsert_apparatus(ApparatusUpsert {
                id: Some(canonical.identity.id.to_string()),
                name: canonical.identity.display.display_name.clone(),
                master: ApparatusMasterData::default(),
            })
            .await
            .expect("seed apparatus catalog");
        let mut canonical = (**canonical).clone();
        canonical.versioning.revision = 2;
        apparatus_groups
            .put_canonical_apparatus(1, canonical)
            .await
            .expect("seed canonical apparatus");
    }
    let service = ProductionMapService::new(Arc::new(MemoryProductionMapStore::new()))
        .with_canonical_apparatus_resolver(Arc::new(ApparatusGroupCanonicalResolver::new(
            apparatus_groups.clone(),
        )));
    (service, apparatus_groups)
}

fn capacity_map(id: &str, apparatus: &str) -> ProductionMapDefinition {
    let mut map = apparatus_stage_map(id, apparatus);
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus {
            node.apparatus_id = FLEXO_ID.to_string();
        }
    }
    map
}

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "admin".to_string(),
        ref_: "capacity-test".to_string(),
        display_name: "Capacity Test".to_string(),
    }
}

fn profile_for(
    id: &str,
    name: &str,
    setup_minutes: u32,
    cleanup_minutes: u32,
    finite_capacity: bool,
) -> ApparatusCapacityProfile {
    ApparatusCapacityProfile {
        apparatus_id: apparatus_id(id),
        apparatus: name.to_string(),
        capacity_slots: 1,
        setup_minutes,
        cleanup_minutes,
        efficiency_percent: 100,
        finite_capacity,
        working_windows: Vec::new(),
        capabilities: vec![
            "print".to_string(),
            "pechat".to_string(),
            "flexo".to_string(),
        ],
        capability_levels: BTreeMap::from([
            (String::from("print"), 1),
            (String::from("pechat"), 1),
            (String::from("flexo"), 3),
        ]),
        notes: String::new(),
        updated_at_unix: 0,
    }
}

fn profile() -> ApparatusCapacityProfile {
    profile_for(FLEXO_ID, FLEXO_NAME, 5, 5, true)
}

fn qolip_validation(order_id: &str) -> TrustedQolipStartValidation {
    TrustedQolipStartValidation::from_preparations(
        &apparatus_id(FLEXO_ID),
        order_id,
        &[QolipOrderStartPreparation {
            spec: QolipProductSpec {
                qolip_code: "QOLIP-CAPACITY-TEST".to_string(),
                ..QolipProductSpec::default()
            },
            checkout: None,
        }],
    )
    .expect("trusted Qolip validation")
}

async fn start_with_qolip(
    service: &ProductionMapService,
    order_id: &str,
    actor: QueueActionActor,
) -> Result<ApparatusQueueActionResult, ProductionMapError> {
    let assigned_apparatus = [FLEXO_ID.to_string()];
    service
        .apply_apparatus_queue_action_with_material_scan_and_progress(MaterialScanProgressAction {
            apparatus: FLEXO_ID,
            order_id,
            action: queue_state::ApparatusQueueAction::Start,
            assigned_apparatus: &assigned_apparatus,
            actor,
            material_barcode: "",
            state_material_barcodes: &[],
            progress: QueueProgressInput::default(),
            qolip_validation: Some(qolip_validation(order_id)),
        })
        .await
}

#[tokio::test]
async fn schedule_requires_canonical_capacity_profile() {
    let service = ProductionMapService::new(Arc::new(MemoryProductionMapStore::new()));
    service
        .upsert_map(capacity_map("capacity-order-missing-profile", FLEXO_NAME))
        .await
        .expect("map");

    assert_eq!(
        service
            .schedule_apparatus_order(schedule(
                "capacity-order-missing-profile",
                "capacity-key-missing-profile",
                10,
            ))
            .await,
        Err(ProductionMapError::StoreFailed)
    );
}

#[tokio::test]
async fn schedule_identity_survives_a_display_name_change() {
    let (service, apparatus_groups) = test_service().await;
    service
        .upsert_map(capacity_map("capacity-order-renamed", FLEXO_NAME))
        .await
        .expect("map");
    let mut renamed_profile = profile();
    renamed_profile.apparatus = "Renamed Flexo".to_string();
    let effective_profile = service
        .put_apparatus_capacity_profile(renamed_profile, &apparatus_groups)
        .await
        .expect("profile");
    assert_eq!(effective_profile.apparatus, FLEXO_NAME);
    let mut request = schedule("capacity-order-renamed", "capacity-key-renamed", 10);
    request.apparatus = "Historical Flexo title".to_string();

    let reservation = service
        .schedule_apparatus_order(request)
        .await
        .expect("reservation")
        .reservation;
    assert_eq!(reservation.apparatus_id.as_str(), FLEXO_ID);
    assert_eq!(reservation.apparatus, FLEXO_NAME);
}

#[tokio::test]
async fn capacity_profile_update_changes_canonical_capacity() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let (service, apparatus_groups) = test_service_with_store(store.clone()).await;
    let mut override_profile = profile();
    override_profile.capacity_slots = 2;
    service
        .upsert_map(capacity_map("capacity-canonical-authority", FLEXO_NAME))
        .await
        .expect("map");
    service
        .upsert_map(capacity_map("capacity-canonical-authority-2", FLEXO_NAME))
        .await
        .expect("second map");

    let updated = service
        .put_apparatus_capacity_profile(override_profile, &apparatus_groups)
        .await
        .expect("canonical capacity update");
    assert_eq!(updated.capacity_slots, 2);
    let canonical = apparatus_groups
        .canonical_apparatus_by_id(&apparatus_id(FLEXO_ID))
        .await
        .expect("canonical apparatus lookup")
        .expect("canonical apparatus");
    assert_eq!(canonical.capacity.capacity_slots, 2);
    let snapshot = service
        .apparatus_capacity_snapshot()
        .await
        .expect("canonical capacity snapshot");
    assert_eq!(snapshot.profiles[0].capacity_slots, 2);

    let mut stale_projection = profile();
    stale_projection.capacity_slots = 1;
    store
        .put_apparatus_capacity_profile(stale_projection)
        .await
        .expect("stale capacity projection");
    let first = service
        .schedule_apparatus_order(schedule(
            "capacity-canonical-authority",
            "capacity-canonical-authority-key-1",
            20,
        ))
        .await
        .expect("first canonical-capacity reservation")
        .reservation;
    let second = service
        .schedule_apparatus_order(schedule(
            "capacity-canonical-authority-2",
            "capacity-canonical-authority-key-2",
            20,
        ))
        .await
        .expect("second canonical-capacity reservation")
        .reservation;
    assert_eq!(first.starts_at_unix, START);
    assert_eq!(second.starts_at_unix, START);
}

fn schedule(order_id: &str, key: &str, duration_minutes: u32) -> ApparatusScheduleRequest {
    ApparatusScheduleRequest {
        order_id: order_id.to_string(),
        apparatus_id: FLEXO_ID.to_string(),
        apparatus: FLEXO_NAME.to_string(),
        earliest_start_unix: START,
        latest_end_unix: None,
        duration_minutes,
        priority: 0,
        source: "capacity-test".to_string(),
        reason: String::new(),
        idempotency_key: key.to_string(),
        capability_requirements: vec![ApparatusCapabilityRequirement {
            code: "flexo".to_string(),
            min_level: 2,
        }],
        candidate_apparatuses: Vec::new(),
        actor: actor(),
    }
}

#[tokio::test]
async fn scheduler_respects_setup_cleanup_finite_capacity_and_idempotency() {
    let (service, apparatus_groups) = test_service().await;
    service
        .upsert_map(capacity_map("capacity-order-1", FLEXO_NAME))
        .await
        .expect("first map");
    service
        .upsert_map(capacity_map("capacity-order-2", FLEXO_NAME))
        .await
        .expect("second map");
    service
        .put_apparatus_capacity_profile(profile(), &apparatus_groups)
        .await
        .expect("profile");

    let first = service
        .schedule_apparatus_order(schedule("capacity-order-1", "capacity-key-1", 20))
        .await
        .expect("first reservation")
        .reservation;
    assert_eq!(first.reserved_duration_minutes, 30);
    assert_eq!(first.starts_at_unix, START);
    assert_eq!(first.ends_at_unix, START + 30 * 60);

    let second = service
        .schedule_apparatus_order(schedule("capacity-order-2", "capacity-key-2", 20))
        .await
        .expect("second reservation")
        .reservation;
    assert_eq!(second.starts_at_unix, first.ends_at_unix);

    let retry = service
        .schedule_apparatus_order(schedule("capacity-order-1", "capacity-key-1", 20))
        .await
        .expect("idempotent retry")
        .reservation;
    assert_eq!(retry, first);
}

#[tokio::test]
async fn queue_execution_keeps_schedule_reservation_in_sync_with_run_status() {
    let (service, apparatus_groups) = test_service().await;
    let order_id = "zakaz-capacity-order-lifecycle";
    service
        .upsert_map(capacity_map(order_id, FLEXO_NAME))
        .await
        .expect("map");
    service
        .put_apparatus_capacity_profile(profile(), &apparatus_groups)
        .await
        .expect("profile");
    let reservation = service
        .schedule_apparatus_order(schedule(order_id, "capacity-key-lifecycle", 20))
        .await
        .expect("reservation")
        .reservation;
    assert_eq!(reservation.status, ApparatusScheduleStatus::Planned);
    assert!(
        service
            .maps()
            .await
            .expect("maps")
            .iter()
            .any(|saved| saved.map.id == order_id)
    );

    let actor = actor();
    start_with_qolip(&service, order_id, actor.clone())
        .await
        .expect("start");
    let snapshot = service
        .apparatus_capacity_snapshot()
        .await
        .expect("active snapshot");
    assert_eq!(
        snapshot.reservations[0].status,
        ApparatusScheduleStatus::Active
    );

    let paused = service
        .apply_apparatus_queue_action_with_progress(
            FLEXO_ID,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[FLEXO_ID.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(1.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("pause");
    let snapshot = service
        .apparatus_capacity_snapshot()
        .await
        .expect("paused snapshot");
    assert_eq!(
        snapshot.reservations[0].status,
        ApparatusScheduleStatus::Paused
    );

    service
        .apply_apparatus_queue_action_with_progress(
            FLEXO_ID,
            order_id,
            queue_state::ApparatusQueueAction::Resume,
            &[FLEXO_ID.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: paused.progress_batch.expect("pause batch").qr_payload,
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("resume");
    let snapshot = service
        .apparatus_capacity_snapshot()
        .await
        .expect("resumed snapshot");
    assert_eq!(
        snapshot.reservations[0].status,
        ApparatusScheduleStatus::Active
    );

    service
        .apply_apparatus_queue_action_with_progress(
            FLEXO_ID,
            order_id,
            queue_state::ApparatusQueueAction::Complete,
            &[FLEXO_ID.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(1.0),
                uom: "kg".to_string(),
                return_ink_kg: Some(0.1),
                total_waste: Some(0.1),
                finished_goods_kg: Some(1.0),
                finished_goods_meter: Some(1.0),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("complete");
    let snapshot = service
        .apparatus_capacity_snapshot()
        .await
        .expect("completed snapshot");
    assert_eq!(
        snapshot.reservations[0].status,
        ApparatusScheduleStatus::Completed
    );
}

#[tokio::test]
async fn active_unscheduled_execution_blocks_capacity_until_pause() {
    let (service, apparatus_groups) = test_service().await;
    let first_order = "zakaz-capacity-active-1";
    let second_order = "zakaz-capacity-active-2";
    for order_id in [first_order, second_order] {
        service
            .upsert_map(capacity_map(order_id, FLEXO_NAME))
            .await
            .expect("map");
    }
    service
        .put_apparatus_capacity_profile(profile(), &apparatus_groups)
        .await
        .expect("profile");
    service
        .set_apparatus_sequence(
            FLEXO_ID,
            vec![first_order.to_string(), second_order.to_string()],
        )
        .await
        .expect("queue sequence");
    let actor = actor();
    start_with_qolip(&service, first_order, actor.clone())
        .await
        .expect("start unscheduled work");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64;
    let blocked = service
        .schedule_apparatus_order(ApparatusScheduleRequest {
            order_id: second_order.to_string(),
            apparatus_id: FLEXO_ID.to_string(),
            apparatus: FLEXO_NAME.to_string(),
            earliest_start_unix: now.saturating_sub(60),
            latest_end_unix: None,
            duration_minutes: 20,
            priority: 0,
            source: "capacity-test".to_string(),
            reason: String::new(),
            idempotency_key: "capacity-key-active-blocked".to_string(),
            capability_requirements: Vec::new(),
            candidate_apparatuses: Vec::new(),
            actor: actor.clone(),
        })
        .await;
    assert_eq!(blocked, Err(ProductionMapError::CapacityNoWorkingWindow));

    service
        .apply_apparatus_queue_action_with_progress(
            FLEXO_ID,
            first_order,
            queue_state::ApparatusQueueAction::Pause,
            &[FLEXO_ID.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(1.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("pause unscheduled work");
    let scheduled = service
        .schedule_apparatus_order(ApparatusScheduleRequest {
            order_id: second_order.to_string(),
            apparatus_id: FLEXO_ID.to_string(),
            apparatus: FLEXO_NAME.to_string(),
            earliest_start_unix: now.saturating_sub(60),
            latest_end_unix: None,
            duration_minutes: 20,
            priority: 0,
            source: "capacity-test".to_string(),
            reason: String::new(),
            idempotency_key: "capacity-key-active-released".to_string(),
            capability_requirements: Vec::new(),
            candidate_apparatuses: Vec::new(),
            actor: actor.clone(),
        })
        .await
        .expect("schedule after pause")
        .reservation;
    assert_eq!(scheduled.status, ApparatusScheduleStatus::Planned);
}

#[tokio::test]
async fn queue_start_rejects_an_apparatus_during_active_downtime() {
    let (service, apparatus_groups) = test_service().await;
    let order_id = "zakaz-capacity-downtime";
    service
        .upsert_map(capacity_map(order_id, FLEXO_NAME))
        .await
        .expect("map");
    service
        .put_apparatus_capacity_profile(profile(), &apparatus_groups)
        .await
        .expect("profile");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64;
    service
        .put_apparatus_downtime(ApparatusDowntime {
            id: "downtime-execution-active".to_string(),
            apparatus_id: apparatus_id(FLEXO_ID),
            apparatus: FLEXO_NAME.to_string(),
            starts_at_unix: now.saturating_sub(60),
            ends_at_unix: now + 3_600,
            reason: "planned maintenance".to_string(),
            active: true,
            actor: actor(),
            created_at_unix: now,
        })
        .await
        .expect("downtime");
    let result = start_with_qolip(&service, order_id, actor()).await;
    assert_eq!(result, Err(ProductionMapError::CapacityUnavailable));
}

#[tokio::test]
async fn scheduler_allows_parallel_reservations_when_capacity_is_unlimited() {
    let (service, apparatus_groups) = unlimited_test_service().await;
    service
        .upsert_map(capacity_map("capacity-order-unlimited-1", FLEXO_NAME))
        .await
        .expect("first map");
    service
        .upsert_map(capacity_map("capacity-order-unlimited-2", FLEXO_NAME))
        .await
        .expect("second map");
    let unlimited = profile_for(FLEXO_ID, FLEXO_NAME, 5, 5, false);
    service
        .put_apparatus_capacity_profile(unlimited, &apparatus_groups)
        .await
        .expect("unlimited profile");

    let first = service
        .schedule_apparatus_order(schedule(
            "capacity-order-unlimited-1",
            "capacity-key-unlimited-1",
            20,
        ))
        .await
        .expect("first reservation")
        .reservation;
    let second = service
        .schedule_apparatus_order(schedule(
            "capacity-order-unlimited-2",
            "capacity-key-unlimited-2",
            20,
        ))
        .await
        .expect("parallel reservation")
        .reservation;

    assert_eq!(second.starts_at_unix, first.starts_at_unix);
    assert_eq!(second.ends_at_unix, first.ends_at_unix);
}

#[tokio::test]
async fn scheduler_selects_the_earliest_compatible_alternative_apparatus() {
    let (service, apparatus_groups) = test_service().await;
    let mut map = capacity_map("capacity-order-alternative", FLEXO_NAME);
    let mut alternative_node = map
        .nodes
        .iter()
        .find(|node| node.kind == ProductionMapNodeKind::Apparatus)
        .cloned()
        .expect("primary apparatus node");
    alternative_node.id = "apparatus-reserve-node".to_string();
    alternative_node.title = RESERVE_NAME.to_string();
    alternative_node.apparatus_id = RESERVE_ID.to_string();
    map.nodes.push(alternative_node);
    service.upsert_map(map).await.expect("map");
    service
        .put_apparatus_capacity_profile(profile(), &apparatus_groups)
        .await
        .expect("primary profile");
    service
        .put_apparatus_capacity_profile(
            profile_for(RESERVE_ID, RESERVE_NAME, 0, 0, true),
            &apparatus_groups,
        )
        .await
        .expect("alternative profile");
    service
        .put_apparatus_downtime(ApparatusDowntime {
            id: "downtime-primary".to_string(),
            apparatus_id: apparatus_id(FLEXO_ID),
            apparatus: FLEXO_NAME.to_string(),
            starts_at_unix: START,
            ends_at_unix: START + 60 * 60,
            reason: "primary breakdown".to_string(),
            active: true,
            actor: actor(),
            created_at_unix: START,
        })
        .await
        .expect("downtime");
    let mut request = schedule("capacity-order-alternative", "capacity-key-alternative", 20);
    request.candidate_apparatuses = vec![ApparatusScheduleCandidate {
        apparatus_id: apparatus_id(RESERVE_ID),
        apparatus: RESERVE_NAME.to_string(),
    }];

    let reservation = service
        .schedule_apparatus_order(request)
        .await
        .expect("alternative reservation")
        .reservation;

    assert_eq!(reservation.apparatus_id.as_str(), RESERVE_ID);
    assert_eq!(reservation.apparatus, RESERVE_NAME);
    assert_eq!(reservation.starts_at_unix, START);
}

#[tokio::test]
async fn scheduler_rejects_an_apparatus_outside_the_order_route() {
    let (service, _apparatus_groups) = test_service().await;
    service
        .upsert_map(capacity_map("capacity-order-route", FLEXO_NAME))
        .await
        .expect("map");
    let mut request = schedule("capacity-order-route", "capacity-key-route", 10);
    request.apparatus_id = REZKA_ID.to_string();
    request.apparatus = REZKA_NAME.to_string();

    assert_eq!(
        service.schedule_apparatus_order(request).await,
        Err(ProductionMapError::MoveNotAllowed)
    );
}

#[tokio::test]
async fn scheduler_skips_downtime_and_rejects_missing_capability() {
    let (service, apparatus_groups) = test_service().await;
    service
        .upsert_map(capacity_map("capacity-order-3", FLEXO_NAME))
        .await
        .expect("map");
    service
        .put_apparatus_capacity_profile(profile(), &apparatus_groups)
        .await
        .expect("profile");
    service
        .put_apparatus_downtime(ApparatusDowntime {
            id: "downtime-1".to_string(),
            apparatus_id: apparatus_id(FLEXO_ID),
            apparatus: FLEXO_NAME.to_string(),
            starts_at_unix: START,
            ends_at_unix: START + 20 * 60,
            reason: "planned maintenance".to_string(),
            active: true,
            actor: actor(),
            created_at_unix: START,
        })
        .await
        .expect("downtime");

    let reservation = service
        .schedule_apparatus_order(schedule("capacity-order-3", "capacity-key-3", 10))
        .await
        .expect("reservation after downtime")
        .reservation;
    assert_eq!(reservation.starts_at_unix, START + 20 * 60);

    let mut unsupported = schedule("capacity-order-3", "capacity-key-4", 10);
    unsupported.capability_requirements = vec![ApparatusCapabilityRequirement {
        code: "rotogravure".to_string(),
        min_level: 1,
    }];
    assert_eq!(
        service.schedule_apparatus_order(unsupported).await,
        Err(ProductionMapError::CapabilityNotSupported)
    );
}

#[tokio::test]
async fn cancelled_reservation_releases_capacity() {
    let (service, apparatus_groups) = test_service().await;
    service
        .upsert_map(capacity_map("capacity-order-4", FLEXO_NAME))
        .await
        .expect("map");
    service
        .put_apparatus_capacity_profile(profile(), &apparatus_groups)
        .await
        .expect("profile");
    let first = service
        .schedule_apparatus_order(schedule("capacity-order-4", "capacity-key-5", 10))
        .await
        .expect("reservation")
        .reservation;
    let cancelled = service
        .cancel_apparatus_schedule_reservation(ApparatusScheduleCancelRequest {
            reservation_id: first.reservation_id.clone(),
            reason: "operator changed plan".to_string(),
            actor: actor(),
        })
        .await
        .expect("cancel");
    assert_eq!(cancelled.status, ApparatusScheduleStatus::Cancelled);

    let next = service
        .schedule_apparatus_order(schedule("capacity-order-4", "capacity-key-6", 10))
        .await
        .expect("replacement reservation")
        .reservation;
    assert_eq!(next.starts_at_unix, first.starts_at_unix);
}
