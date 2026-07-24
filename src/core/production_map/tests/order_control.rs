use crate::core::production_map::*;

use super::fixtures::apparatus_stage_map;

fn actor(role: &str) -> QueueActionActor {
    QueueActionActor {
        role: role.to_string(),
        ref_: format!("{role}-1"),
        display_name: role.to_string(),
    }
}

#[tokio::test]
async fn freeze_request_requires_worker_pause_then_blocks_worker_actions() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let apparatus = "7 ta rangli pechat";
    let order_id = "zakaz-freeze-1";
    service
        .upsert_map(apparatus_stage_map(order_id, apparatus))
        .await
        .expect("map");
    service
        .apply_apparatus_queue_action(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[apparatus.to_string()],
            actor("worker"),
        )
        .await
        .expect("start");

    let requested = service
        .request_order_freeze(order_id, actor("admin"))
        .await
        .expect("freeze request");
    assert_eq!(requested.state, OrderControlState::FreezeRequested);
    let freeze_request = requested
        .freeze_request
        .as_ref()
        .expect("bound freeze request");
    assert_eq!(freeze_request.target_apparatus, apparatus);
    assert_eq!(freeze_request.target_worker_role, "worker");
    assert_eq!(freeze_request.target_worker_ref, "worker-1");

    let wrong_worker_pause = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[apparatus.to_string()],
            actor("other-worker"),
            QueueProgressInput {
                produced_qty: Some(1.0),
                uom: "kg".to_string(),
                freeze_request_id: freeze_request.request_id.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await;
    assert_eq!(
        wrong_worker_pause,
        Err(ProductionMapError::OrderFreezeRequestMismatch)
    );

    let wrong_request_pause = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[apparatus.to_string()],
            actor("worker"),
            QueueProgressInput {
                produced_qty: Some(1.0),
                uom: "kg".to_string(),
                freeze_request_id: "order-freeze-request_wrong".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await;
    assert_eq!(
        wrong_request_pause,
        Err(ProductionMapError::OrderFreezeRequestMismatch)
    );

    let complete_while_requested = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Complete,
            &[apparatus.to_string()],
            actor("worker"),
            QueueProgressInput {
                produced_qty: Some(1.0),
                uom: "kg".to_string(),
                freeze_request_id: freeze_request.request_id.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await;
    assert_eq!(
        complete_while_requested,
        Err(ProductionMapError::OrderFreezeRequested)
    );

    service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[apparatus.to_string()],
            actor("worker"),
            QueueProgressInput {
                produced_qty: Some(1.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("worker pause acknowledgement");
    assert_eq!(
        service
            .order_control_state(order_id)
            .await
            .expect("control")
            .state,
        OrderControlState::Frozen
    );

    let resume_while_frozen = service
        .apply_apparatus_queue_action(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Resume,
            &[apparatus.to_string()],
            actor("worker"),
        )
        .await;
    assert_eq!(resume_while_frozen, Err(ProductionMapError::OrderFrozen));
}

#[tokio::test]
async fn cancelled_freeze_request_rejects_a_late_card_pause() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let apparatus = "7 ta rangli pechat";
    let order_id = "zakaz-freeze-cancel-race";
    service
        .upsert_map(apparatus_stage_map(order_id, apparatus))
        .await
        .expect("map");
    service
        .apply_apparatus_queue_action(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[apparatus.to_string()],
            actor("worker"),
        )
        .await
        .expect("start");
    let requested = service
        .request_order_freeze(order_id, actor("admin"))
        .await
        .expect("freeze request");
    let request_id = requested.freeze_request.expect("bound request").request_id;

    service
        .cancel_order_freeze_request(order_id, actor("admin"))
        .await
        .expect("cancel request");

    let late_pause = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[apparatus.to_string()],
            actor("worker"),
            QueueProgressInput {
                produced_qty: Some(1.0),
                uom: "kg".to_string(),
                freeze_request_id: request_id.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await;
    assert_eq!(
        late_pause,
        Err(ProductionMapError::OrderFreezeRequestMismatch)
    );
    assert_eq!(
        service
            .order_control_state(order_id)
            .await
            .expect("control")
            .state,
        OrderControlState::Active
    );

    let second_request = service
        .request_order_freeze(order_id, actor("admin"))
        .await
        .expect("new freeze after cancellation");
    assert_eq!(second_request.state, OrderControlState::FreezeRequested);
    assert_ne!(
        second_request
            .freeze_request
            .expect("second request")
            .request_id,
        request_id
    );
}

#[tokio::test]
async fn already_paused_order_freezes_immediately_and_frozen_order_can_reorder() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let apparatus = "Laminatsiya";
    let frozen_id = "zakaz-freeze-paused";
    let next_id = "zakaz-after-frozen";
    service
        .upsert_map(apparatus_stage_map(frozen_id, apparatus))
        .await
        .expect("frozen map");
    service
        .upsert_map(apparatus_stage_map(next_id, apparatus))
        .await
        .expect("next map");
    service
        .set_apparatus_sequence(apparatus, vec![frozen_id.to_string(), next_id.to_string()])
        .await
        .expect("sequence");
    service
        .apply_apparatus_queue_action(
            apparatus,
            frozen_id,
            queue_state::ApparatusQueueAction::Start,
            &[apparatus.to_string()],
            actor("worker"),
        )
        .await
        .expect("start");
    service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            frozen_id,
            queue_state::ApparatusQueueAction::Pause,
            &[apparatus.to_string()],
            actor("worker"),
            QueueProgressInput {
                produced_qty: Some(2.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("ordinary pause");

    let frozen = service
        .request_order_freeze(frozen_id, actor("admin"))
        .await
        .expect("direct freeze");
    assert_eq!(frozen.state, OrderControlState::Frozen);
    service
        .set_apparatus_sequence(apparatus, vec![next_id.to_string(), frozen_id.to_string()])
        .await
        .expect("frozen order can move later");
    service
        .apply_apparatus_queue_action(
            apparatus,
            next_id,
            queue_state::ApparatusQueueAction::Start,
            &[apparatus.to_string()],
            actor("worker"),
        )
        .await
        .expect("next order starts while frozen order remains paused");

    service
        .unfreeze_order(frozen_id, actor("admin"))
        .await
        .expect("unfreeze");
    let resume_while_second = service
        .apply_apparatus_queue_action(
            apparatus,
            frozen_id,
            queue_state::ApparatusQueueAction::Resume,
            &[apparatus.to_string()],
            actor("worker"),
        )
        .await;
    assert_eq!(
        resume_while_second,
        Err(ProductionMapError::QueueActionNotAllowed)
    );
}

#[tokio::test]
async fn delete_uses_current_three_conditions_and_returns_all_blockers() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new(store.clone());
    let apparatus = "7 ta rangli pechat";
    let blocked_id = "zakaz-delete-blocked";
    let removable_id = "zakaz-delete-removable";
    service
        .upsert_map(apparatus_stage_map(blocked_id, apparatus))
        .await
        .expect("blocked map");
    service
        .upsert_map(apparatus_stage_map(removable_id, apparatus))
        .await
        .expect("removable map");
    service
        .set_apparatus_sequence(
            apparatus,
            vec![blocked_id.to_string(), removable_id.to_string()],
        )
        .await
        .expect("sequence");
    service
        .apply_apparatus_queue_action(
            apparatus,
            blocked_id,
            queue_state::ApparatusQueueAction::Start,
            &[apparatus.to_string()],
            actor("worker"),
        )
        .await
        .expect("start blocked order");
    store
        .put_raw_material_assignment(RawMaterialAssignment {
            order_id: blocked_id.to_string(),
            apparatus: apparatus.to_string(),
            barcode: "RAW-DELETE-1".to_string(),
            item_code: "RAW".to_string(),
            item_name: "Raw".to_string(),
            item_group: "Raw".to_string(),
            assigned_by_role: "admin".to_string(),
            assigned_by_ref: "admin-1".to_string(),
            assigned_by_display_name: "Admin".to_string(),
            assigned_at: "now".to_string(),
        })
        .await
        .expect("material");

    let blocked = service.delete_order(blocked_id).await;
    let Err(ProductionMapError::OrderDeleteBlocked(blockers)) = blocked else {
        panic!("delete must return blockers: {blocked:?}");
    };
    let codes = blockers
        .iter()
        .map(|blocker| blocker.code.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        codes,
        std::collections::BTreeSet::from([
            "first_in_sequence",
            "raw_material_attached",
            "work_started",
        ])
    );

    store
        .put_raw_material_assignment(RawMaterialAssignment {
            order_id: removable_id.to_string(),
            apparatus: apparatus.to_string(),
            barcode: "RAW-DELETE-2".to_string(),
            item_code: "RAW".to_string(),
            item_name: "Raw".to_string(),
            item_group: "Raw".to_string(),
            assigned_by_role: "admin".to_string(),
            assigned_by_ref: "admin-1".to_string(),
            assigned_by_display_name: "Admin".to_string(),
            assigned_at: "now".to_string(),
        })
        .await
        .expect("temporary material");
    service
        .unlink_raw_material_assignment(RawMaterialAssignmentDeleteInput {
            order_id: removable_id.to_string(),
            barcode: "RAW-DELETE-2".to_string(),
        })
        .await
        .expect("unlink material");
    let deleted = service
        .delete_order(removable_id)
        .await
        .expect("delete currently clean order");
    assert!(deleted.deleted);
    assert!(
        service
            .raw_map(removable_id)
            .await
            .expect("map lookup")
            .is_none()
    );
}
