use crate::core::production_map::*;

use std::collections::BTreeMap;

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
    let freeze_request_id = freeze_request.request_id.clone();
    let requested_snapshot = service.live_snapshot().await.expect("requested snapshot");
    let action_control = requested_snapshot
        .queue_action_controls
        .get(apparatus)
        .and_then(|controls| controls.get(order_id))
        .expect("freeze-request action control");
    assert_eq!(
        action_control
            .freeze_request
            .as_ref()
            .map(|request| request.request_id.as_str()),
        Some(freeze_request_id.as_str())
    );

    let wrong_worker_pause = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Freeze,
            &[apparatus.to_string()],
            actor("other-worker"),
            QueueProgressInput {
                produced_qty: Some(1.0),
                uom: "kg".to_string(),
                freeze_request_id: freeze_request_id.clone(),
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
                freeze_request_id: freeze_request_id.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await;
    assert_eq!(
        complete_while_requested,
        Err(ProductionMapError::OrderFreezeRequested)
    );

    let missing_request_metadata = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::DetachRoll,
            &[apparatus.to_string()],
            actor("worker"),
            QueueProgressInput {
                produced_qty: Some(10.0),
                gross_qty: Some(2.0),
                bobina_kg: Some(0.5),
                uom: "m".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await;
    assert_eq!(
        missing_request_metadata,
        Err(ProductionMapError::OrderFreezeRequestMismatch)
    );

    let frozen = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::DetachRoll,
            &[apparatus.to_string()],
            actor("worker"),
            QueueProgressInput {
                produced_qty: Some(10.0),
                gross_qty: Some(2.0),
                bobina_kg: Some(0.5),
                uom: "m".to_string(),
                freeze_request_id,
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("worker pause acknowledgement");
    assert!(frozen.progress_batch.is_some());
    assert_eq!(
        frozen.session.as_ref().map(|session| session.status),
        Some(OrderRunStatus::Frozen)
    );
    assert_eq!(
        service
            .order_control_state(order_id)
            .await
            .expect("control")
            .state,
        OrderControlState::Frozen
    );
    assert!(
        service
            .wip_progress_batches(WipProgressBatchQuery::new(
                apparatus,
                "",
                "",
                Some(OrderProgressBatchWipStatus::Waiting),
                false,
                order_id,
                10,
            ))
            .await
            .expect("frozen WIP query")
            .is_empty()
    );

    let resume_while_frozen = service
        .apply_apparatus_queue_action(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[apparatus.to_string()],
            actor("worker"),
        )
        .await;
    assert_eq!(resume_while_frozen, Err(ProductionMapError::OrderFrozen));

    service
        .unfreeze_order(order_id, actor("admin"))
        .await
        .expect("unfreeze");
    assert_eq!(
        service
            .wip_progress_batches(WipProgressBatchQuery::new(
                apparatus,
                "",
                "",
                Some(OrderProgressBatchWipStatus::Waiting),
                false,
                order_id,
                10,
            ))
            .await
            .expect("unfrozen WIP query")
            .len(),
        1
    );
}

#[tokio::test]
async fn freeze_request_issue_safe_stop_rejects_partial_output_without_mutation() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let apparatus = "7 ta rangli pechat";
    let order_id = "zakaz-freeze-request-issue";
    let issue_note = "Valdan sog‘lom mahsulot chiqarib bo‘lmaydi";
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
    let freeze_request_id = requested
        .freeze_request
        .expect("freeze request metadata")
        .request_id;

    let partial = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::DetachRoll,
            &[apparatus.to_string()],
            actor("worker"),
            QueueProgressInput {
                produced_qty: Some(10.0),
                freeze_request_id: freeze_request_id.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await;
    assert_eq!(partial, Err(ProductionMapError::ProgressInputInvalid));
    let unchanged = service.live_snapshot().await.expect("unchanged snapshot");
    assert_eq!(
        unchanged.order_controls[order_id].state,
        OrderControlState::FreezeRequested
    );
    assert_eq!(unchanged.queue_states[apparatus][order_id], "in_progress");
    assert_eq!(unchanged.sequences[apparatus], vec![order_id.to_string()]);
    assert_eq!(
        service
            .order_run_sessions_for_order(order_id)
            .await
            .expect("sessions")
            .into_iter()
            .find(|session| session.apparatus == apparatus)
            .expect("session")
            .status,
        OrderRunStatus::Active
    );

    let frozen = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::DetachRoll,
            &[apparatus.to_string()],
            actor("worker"),
            QueueProgressInput {
                description: issue_note.to_string(),
                freeze_request_id,
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("issue safe-stop");
    assert!(frozen.progress_batch.is_none());
    assert_eq!(
        frozen.session.as_ref().map(|session| session.status),
        Some(OrderRunStatus::Frozen)
    );
    let snapshot = service.live_snapshot().await.expect("frozen snapshot");
    assert_eq!(
        snapshot.order_controls[order_id].state,
        OrderControlState::Frozen
    );
    assert_eq!(snapshot.queue_states[apparatus][order_id], "frozen");
    assert!(snapshot.sequences[apparatus].is_empty());
    assert_eq!(
        snapshot.frozen_orders_by_apparatus[apparatus][0].issue_note,
        issue_note
    );
    assert!(
        service
            .completed_queue_orders_for_actor("worker-1", 10)
            .await
            .expect("completed history")
            .is_empty()
    );
}

#[tokio::test]
async fn freeze_request_rejects_an_order_frozen_on_another_apparatus() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new(store.clone());
    let apparatus = "7 ta rangli pechat";
    let other_apparatus = "Laminatsiya 1";
    let order_id = "zakaz-freeze-other-apparatus";

    service
        .upsert_map(apparatus_stage_map(order_id, apparatus))
        .await
        .expect("map");
    service
        .set_apparatus_sequence(apparatus, vec![order_id.to_string()])
        .await
        .expect("sequence");
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
    store
        .put_apparatus_queue_states(
            other_apparatus,
            BTreeMap::from([(order_id.to_string(), "frozen".to_string())]),
        )
        .await
        .expect("other apparatus frozen state");

    assert_eq!(
        service.request_order_freeze(order_id, actor("admin")).await,
        Err(ProductionMapError::OrderFrozen)
    );
}

#[tokio::test]
async fn worker_issue_freezes_bosma_without_completion_metrics_and_unfreeze_restores_validation() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let apparatus = "7 ta rangli bosma";
    let order_id = "zakaz-bosma-freeze-issue";
    let issue_note = "Bosma apparatida muammo: rang chiqishi notekis";
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
            actor("aparatchi"),
        )
        .await
        .expect("start");

    let issue = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Freeze,
            &[apparatus.to_string()],
            actor("aparatchi"),
            QueueProgressInput {
                freeze_with_issue: true,
                description: issue_note.to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("worker issue");
    assert_eq!(issue.states[order_id], "frozen");
    assert_eq!(issue.order_status.order_status, "frozen");
    assert_eq!(
        issue
            .order_control
            .as_ref()
            .expect("resulting order control")
            .state,
        OrderControlState::Frozen
    );
    assert!(issue.progress_batch.is_none());
    let frozen_snapshot = service.live_snapshot().await.expect("frozen snapshot");
    assert_eq!(
        frozen_snapshot.sequences.get(apparatus).cloned(),
        Some(Vec::new())
    );
    assert_eq!(
        frozen_snapshot
            .frozen_orders_by_apparatus
            .get(apparatus)
            .and_then(|orders| orders.first())
            .map(|order| order.issue_note.as_str()),
        Some(issue_note)
    );
    assert!(
        !frozen_snapshot
            .queue_action_controls
            .get(apparatus)
            .is_some_and(|controls| controls.contains_key(order_id))
    );

    let logs = service
        .queue_action_logs_for_order(order_id)
        .await
        .expect("history");
    let issue_log = logs
        .iter()
        .find(|log| log.issue_note == issue_note)
        .expect("issue history entry");
    assert_eq!(issue_log.action, queue_state::ApparatusQueueAction::Freeze);
    assert!(!issue_log.completed_with_issue);
    assert!(
        service
            .completion_requests(10)
            .await
            .expect("requests")
            .is_empty()
    );
    let completed_history = service
        .completed_queue_orders_for_actor("aparatchi-1", 10)
        .await
        .expect("completed orders");
    assert!(completed_history.is_empty());

    assert_eq!(
        service
            .apply_apparatus_queue_action(
                apparatus,
                order_id,
                queue_state::ApparatusQueueAction::Resume,
                &[apparatus.to_string()],
                actor("aparatchi"),
            )
            .await,
        Err(ProductionMapError::OrderFrozen)
    );
    assert_eq!(
        service
            .apply_apparatus_queue_action_with_progress(
                apparatus,
                order_id,
                queue_state::ApparatusQueueAction::Complete,
                &[apparatus.to_string()],
                actor("aparatchi"),
                QueueProgressInput::default(),
            )
            .await,
        Err(ProductionMapError::OrderFrozen)
    );

    service
        .unfreeze_order(order_id, actor("admin"))
        .await
        .expect("unfreeze");
    let unfrozen_snapshot = service.live_snapshot().await.expect("unfrozen snapshot");
    assert_eq!(
        unfrozen_snapshot.sequences.get(apparatus).cloned(),
        Some(vec![order_id.to_string()])
    );
    assert_eq!(
        unfrozen_snapshot
            .queue_action_controls
            .get(apparatus)
            .and_then(|controls| controls.get(order_id))
            .map(|control| control.allowed_actions.clone()),
        Some(vec![queue_state::ApparatusQueueAction::Resume])
    );
    assert_eq!(
        unfrozen_snapshot
            .order_statuses
            .get(order_id)
            .map(|status| status.order_status.as_str()),
        Some("ready")
    );
    assert_eq!(
        service
            .apparatus_queue_states()
            .await
            .expect("queue states")
            .get(apparatus)
            .and_then(|states| states.get(order_id))
            .map(String::as_str),
        Some("pending")
    );
    let history_after_unfreeze = service
        .completed_queue_orders_for_actor("aparatchi-1", 10)
        .await
        .expect("completed orders after unfreeze");
    assert!(history_after_unfreeze.is_empty());
    service
        .apply_apparatus_queue_action(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Resume,
            &[apparatus.to_string()],
            actor("aparatchi"),
        )
        .await
        .expect("resume after unfreeze");

    assert_eq!(
        service
            .apply_apparatus_queue_action_with_progress(
                apparatus,
                order_id,
                queue_state::ApparatusQueueAction::Complete,
                &[apparatus.to_string()],
                actor("aparatchi"),
                QueueProgressInput::default(),
            )
            .await,
        Err(ProductionMapError::BosmaCompletionMetricsRequired)
    );
    let completed = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Complete,
            &[apparatus.to_string()],
            actor("aparatchi"),
            QueueProgressInput {
                return_ink_kg: Some(1.0),
                total_waste: Some(1.0),
                finished_goods_kg: Some(1.0),
                finished_goods_meter: Some(1.0),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("ordinary completion after unfreeze");
    assert_eq!(completed.states[order_id], "completed");
    let completed_orders = service
        .completed_queue_orders_for_actor("aparatchi-1", 10)
        .await
        .expect("completed orders after completion");
    assert_eq!(completed_orders.len(), 1);
    assert_eq!(
        completed_orders[0].status,
        CompletedQueueOrderStatus::Completed
    );
}

#[tokio::test]
async fn closed_order_logs_include_freeze_lifecycle_events() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let apparatus = "7 ta rangli pechat";
    let order_id = "zakaz-freeze-history";
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
    let freeze_request_id = requested
        .freeze_request
        .expect("freeze request metadata")
        .request_id;
    service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[apparatus.to_string()],
            actor("worker"),
            QueueProgressInput {
                produced_qty: Some(10.0),
                gross_qty: Some(2.0),
                bobina_kg: Some(0.5),
                uom: "m".to_string(),
                freeze_request_id,
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("freeze pause");
    service
        .unfreeze_order(order_id, actor("admin"))
        .await
        .expect("unfreeze");
    service
        .apply_apparatus_queue_action(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Resume,
            &[apparatus.to_string()],
            actor("worker"),
        )
        .await
        .expect("resume after unfreeze");
    service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::Complete,
            &[apparatus.to_string()],
            actor("worker"),
            QueueProgressInput {
                produced_qty: Some(1.0),
                uom: "kg".to_string(),
                return_ink_kg: Some(1.0),
                total_waste: Some(1.0),
                finished_goods_kg: Some(1.0),
                finished_goods_meter: Some(1.0),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("complete");

    let closed = service
        .fully_completed_orders(10)
        .await
        .expect("closed orders");
    let logs = &closed.first().expect("closed order").logs;
    let freeze_statuses = logs
        .iter()
        .filter_map(|log| log.freeze.as_ref().map(|freeze| freeze.status.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(freeze_statuses.len(), 3);
    assert!(freeze_statuses.contains(&"pending"));
    assert!(freeze_statuses.contains(&"frozen"));
    assert!(freeze_statuses.contains(&"unfrozen"));
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
async fn already_paused_order_leaves_queue_and_unfreeze_appends_at_tail() {
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
    assert_eq!(
        service
            .apparatus_sequences()
            .await
            .expect("sequence after freeze")
            .get(apparatus),
        Some(&vec![next_id.to_string()])
    );
    service
        .apply_apparatus_queue_action(
            apparatus,
            next_id,
            queue_state::ApparatusQueueAction::Start,
            &[apparatus.to_string()],
            actor("worker"),
        )
        .await
        .expect("next order starts while frozen order remains frozen");

    service
        .unfreeze_order(frozen_id, actor("admin"))
        .await
        .expect("unfreeze");
    assert_eq!(
        service
            .apparatus_sequences()
            .await
            .expect("sequence after unfreeze")
            .get(apparatus),
        Some(&vec![next_id.to_string(), frozen_id.to_string()])
    );
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

    let refrozen = service
        .request_order_freeze(frozen_id, actor("admin"))
        .await
        .expect("refreeze requeued paused order");
    assert_eq!(refrozen.state, OrderControlState::Frozen);
    assert_eq!(
        service
            .apparatus_queue_states()
            .await
            .expect("queue states after refreeze")
            .get(apparatus)
            .and_then(|states| states.get(frozen_id))
            .map(String::as_str),
        Some("frozen")
    );
    assert_eq!(
        service
            .order_run_sessions_for_order(frozen_id)
            .await
            .expect("session after refreeze")
            .first()
            .map(|session| session.status),
        Some(OrderRunStatus::Frozen)
    );
    assert_eq!(
        service
            .apparatus_sequences()
            .await
            .expect("sequence after refreeze")
            .get(apparatus),
        Some(&vec![next_id.to_string()])
    );

    let second_unfreeze = service
        .unfreeze_order(frozen_id, actor("admin"))
        .await
        .expect("unfreeze refrozen order");
    assert_eq!(second_unfreeze.state, OrderControlState::Active);
    assert_eq!(
        service
            .apparatus_queue_states()
            .await
            .expect("queue states after second unfreeze")
            .get(apparatus)
            .and_then(|states| states.get(frozen_id))
            .map(String::as_str),
        Some("pending")
    );
    assert_eq!(
        service
            .order_run_sessions_for_order(frozen_id)
            .await
            .expect("session after second unfreeze")
            .first()
            .map(|session| session.status),
        Some(OrderRunStatus::Paused)
    );
    assert_eq!(
        service
            .apparatus_sequences()
            .await
            .expect("sequence after second unfreeze")
            .get(apparatus),
        Some(&vec![next_id.to_string(), frozen_id.to_string()])
    );
}

#[tokio::test]
async fn unfreeze_recovers_control_only_refreeze_after_order_was_requeued() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new(store.clone());
    let apparatus = "Laminatsiya";
    let order_id = "zakaz-refreeze-recovery";
    service
        .upsert_map(apparatus_stage_map(order_id, apparatus))
        .await
        .expect("map");
    service
        .set_apparatus_sequence(apparatus, vec![order_id.to_string()])
        .await
        .expect("sequence");
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
        .expect("pause");
    let first_frozen = service
        .request_order_freeze(order_id, actor("admin"))
        .await
        .expect("first freeze");
    service
        .unfreeze_order(order_id, actor("admin"))
        .await
        .expect("first unfreeze");

    let mut stuck_request = first_frozen.freeze_request.expect("first freeze request");
    stuck_request.request_id = "order-freeze-request_stuck-refreeze".to_string();
    stuck_request.status = OrderFreezeRequestStatus::Frozen;
    stuck_request.transitioned_at_unix += 1;
    store
        .put_order_control_state(OrderControlRecord {
            order_id: order_id.to_string(),
            state: OrderControlState::Frozen,
            actor: actor("admin"),
            requested_at_unix: stuck_request.requested_at_unix,
            frozen_at_unix: Some(stuck_request.transitioned_at_unix),
            freeze_request: Some(stuck_request),
        })
        .await
        .expect("controlled control-only refreeze");

    let recovered = service
        .unfreeze_order(order_id, actor("admin"))
        .await
        .expect("recover stuck refreeze");
    assert_eq!(recovered.state, OrderControlState::Active);
    assert_eq!(
        recovered
            .freeze_request
            .as_ref()
            .map(|request| request.status),
        Some(OrderFreezeRequestStatus::Unfrozen)
    );
    assert_eq!(
        service
            .apparatus_queue_states()
            .await
            .expect("recovered queue state")
            .get(apparatus)
            .and_then(|states| states.get(order_id))
            .map(String::as_str),
        Some("pending")
    );
    assert_eq!(
        service
            .order_run_sessions_for_order(order_id)
            .await
            .expect("recovered session")
            .first()
            .map(|session| session.status),
        Some(OrderRunStatus::Paused)
    );
    assert_eq!(
        service
            .apparatus_sequences()
            .await
            .expect("recovered sequence")
            .get(apparatus),
        Some(&vec![order_id.to_string()])
    );
}

#[tokio::test]
async fn roll_detached_order_can_freeze_unfreeze_and_refreeze() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let apparatus = "Laminatsiya";
    let order_id = "zakaz-refreeze-roll-detached";
    service
        .upsert_map(apparatus_stage_map(order_id, apparatus))
        .await
        .expect("map");
    service
        .set_apparatus_sequence(apparatus, vec![order_id.to_string()])
        .await
        .expect("sequence");
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
    service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order_id,
            queue_state::ApparatusQueueAction::DetachRoll,
            &[apparatus.to_string()],
            actor("worker"),
            QueueProgressInput {
                produced_qty: Some(1.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("detach roll");

    let first_freeze = service
        .request_order_freeze(order_id, actor("admin"))
        .await
        .expect("freeze detached roll");
    assert_eq!(first_freeze.state, OrderControlState::Frozen);
    assert!(
        first_freeze
            .freeze_request
            .as_ref()
            .is_some_and(|request| !request.target_session_id.is_empty())
    );
    service
        .unfreeze_order(order_id, actor("admin"))
        .await
        .expect("first unfreeze");
    let refrozen = service
        .request_order_freeze(order_id, actor("admin"))
        .await
        .expect("refreeze detached roll");
    assert_eq!(refrozen.state, OrderControlState::Frozen);
    let second_unfreeze = service
        .unfreeze_order(order_id, actor("admin"))
        .await
        .expect("second unfreeze");
    assert_eq!(second_unfreeze.state, OrderControlState::Active);
    assert_eq!(
        service
            .apparatus_queue_states()
            .await
            .expect("queue states")
            .get(apparatus)
            .and_then(|states| states.get(order_id))
            .map(String::as_str),
        Some("pending")
    );
    assert_eq!(
        service
            .apparatus_sequences()
            .await
            .expect("sequence")
            .get(apparatus),
        Some(&vec![order_id.to_string()])
    );
}

#[tokio::test]
async fn freeze_and_unfreeze_only_update_target_apparatus_and_append_tail() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let target_apparatus = "Laminatsiya";
    let other_apparatus = "Rezka apparat";
    let frozen_id = "zakaz-freeze-independent";
    let next_id = "zakaz-freeze-independent-next";
    let other_id = "zakaz-freeze-independent-other";
    for (order_id, apparatus) in [
        (frozen_id, target_apparatus),
        (next_id, target_apparatus),
        (other_id, other_apparatus),
    ] {
        service
            .upsert_map(apparatus_stage_map(order_id, apparatus))
            .await
            .expect("map");
    }
    service
        .set_apparatus_sequence(
            target_apparatus,
            vec![frozen_id.to_string(), next_id.to_string()],
        )
        .await
        .expect("target sequence");
    service
        .set_apparatus_sequence(other_apparatus, vec![other_id.to_string()])
        .await
        .expect("other sequence");
    service
        .apply_apparatus_queue_action(
            target_apparatus,
            frozen_id,
            queue_state::ApparatusQueueAction::Start,
            &[target_apparatus.to_string()],
            actor("worker"),
        )
        .await
        .expect("start target order");
    service
        .apply_apparatus_queue_action_with_progress(
            target_apparatus,
            frozen_id,
            queue_state::ApparatusQueueAction::Pause,
            &[target_apparatus.to_string()],
            actor("worker"),
            QueueProgressInput {
                produced_qty: Some(1.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("pause target order");

    service
        .request_order_freeze(frozen_id, actor("admin"))
        .await
        .expect("freeze target order");
    assert_eq!(
        service
            .apparatus_sequences()
            .await
            .expect("sequences after freeze"),
        BTreeMap::from([
            (target_apparatus.to_string(), vec![next_id.to_string()],),
            (other_apparatus.to_string(), vec![other_id.to_string()]),
        ])
    );

    service
        .unfreeze_order(frozen_id, actor("admin"))
        .await
        .expect("unfreeze target order");
    assert_eq!(
        service
            .apparatus_sequences()
            .await
            .expect("sequences after unfreeze"),
        BTreeMap::from([
            (
                target_apparatus.to_string(),
                vec![next_id.to_string(), frozen_id.to_string()],
            ),
            (other_apparatus.to_string(), vec![other_id.to_string()]),
        ])
    );
    assert_eq!(
        service
            .apparatus_queue_states()
            .await
            .expect("queue states")
            .get(target_apparatus)
            .and_then(|states| states.get(frozen_id))
            .map(String::as_str),
        Some("pending")
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
