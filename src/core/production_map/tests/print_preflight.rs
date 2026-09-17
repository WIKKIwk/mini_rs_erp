use std::sync::Arc;

use crate::core::production_map::*;

use super::fixtures::{canonical_apparatus_stage_map, service_with_default_apparatus};

const PRINT_ID: &str = "apparatus:default:bosma_7";

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "print-preflight-test".to_string(),
        display_name: "Print preflight test".to_string(),
    }
}

#[tokio::test]
async fn color_button_starts_preflight_and_locks_order_operations() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = service_with_default_apparatus(store).await;
    let order_id = "zakaz-print-preflight-running";
    service
        .upsert_map(canonical_apparatus_stage_map(
            order_id,
            PRINT_ID,
            "7 ta rangli bosma aparat",
        ))
        .await
        .expect("map");

    let hold = service
        .begin_print_preflight(
            PRINT_ID,
            order_id,
            "print-preflight-running",
            "print-preflight-running-idempotency",
            actor(),
        )
        .await
        .expect("preflight start");
    assert_eq!(hold.status, PrintPreflightStatus::Running);

    let snapshot = service.live_snapshot().await.expect("snapshot");
    let control = snapshot
        .queue_action_controls
        .get(PRINT_ID)
        .and_then(|orders| orders.get(order_id))
        .expect("queue control");
    assert_eq!(
        control
            .print_preflight
            .as_ref()
            .expect("active preflight")
            .status,
        PrintPreflightStatus::Running
    );
    assert!(control.allowed_actions.is_empty());
    assert_eq!(
        control.interaction.blocking_reason_code,
        "print_preflight_active"
    );

    assert!(matches!(
        service
            .set_apparatus_sequence(PRINT_ID, vec![order_id.to_string()])
            .await,
        Err(ProductionMapError::PrintPreflightActive)
    ));

    let Err(ProductionMapError::OrderDeleteBlocked(blockers)) =
        service.delete_order(order_id).await
    else {
        panic!("active color preflight must block deletion");
    };
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.code == "print_preflight_active"),
        "preflight blocker missing: {blockers:?}"
    );
}
