use std::collections::BTreeMap;
use std::sync::Arc;

use crate::core::production_map::*;

use super::fixtures::apparatus_stage_map;

const FLEXO_ID: &str = "apparatus:flexo";
const FLEXO_NAME: &str = "Flexo pechat";
const START: i64 = 1_700_000_040;

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "admin".to_string(),
        ref_: "capacity-test".to_string(),
        display_name: "Capacity Test".to_string(),
    }
}

fn profile() -> ApparatusCapacityProfile {
    ApparatusCapacityProfile {
        apparatus_id: FLEXO_ID.to_string(),
        apparatus: FLEXO_NAME.to_string(),
        capacity_slots: 1,
        setup_minutes: 5,
        cleanup_minutes: 5,
        efficiency_percent: 100,
        finite_capacity: true,
        working_windows: Vec::new(),
        capabilities: vec!["flexo".to_string()],
        capability_levels: BTreeMap::from([(String::from("flexo"), 3)]),
        notes: String::new(),
        updated_at_unix: 0,
    }
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
        actor: actor(),
    }
}

#[tokio::test]
async fn scheduler_respects_setup_cleanup_finite_capacity_and_idempotency() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new(store);
    service
        .upsert_map(apparatus_stage_map("capacity-order-1", FLEXO_NAME))
        .await
        .expect("first map");
    service
        .upsert_map(apparatus_stage_map("capacity-order-2", FLEXO_NAME))
        .await
        .expect("second map");
    service
        .put_apparatus_capacity_profile(profile())
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
async fn scheduler_skips_downtime_and_rejects_missing_capability() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new(store);
    service
        .upsert_map(apparatus_stage_map("capacity-order-3", FLEXO_NAME))
        .await
        .expect("map");
    service
        .put_apparatus_capacity_profile(profile())
        .await
        .expect("profile");
    service
        .put_apparatus_downtime(ApparatusDowntime {
            id: "downtime-1".to_string(),
            apparatus_id: FLEXO_ID.to_string(),
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
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new(store);
    service
        .upsert_map(apparatus_stage_map("capacity-order-4", FLEXO_NAME))
        .await
        .expect("map");
    service
        .put_apparatus_capacity_profile(profile())
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
