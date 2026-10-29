use std::sync::Arc;

use crate::core::production_map::*;

fn actor(role: &str) -> QueueActionActor {
    QueueActionActor {
        role: role.to_string(),
        ref_: format!("{role}_personal_account"),
        display_name: format!("{role} account"),
    }
}

#[tokio::test]
async fn brigader_can_manage_existing_additional_settings() {
    let service = ProductionMapService::new_for_test(Arc::new(MemoryProductionMapStore::new()));
    let brigader = actor("brigader");

    let paddon = service
        .update_paddon_settings(Some(true), Some(true), &brigader)
        .await
        .expect("brigader updates paddon settings");
    assert!(paddon.free_movement_enabled && paddon.worker_visibility_enabled);
    assert!(
        service
            .update_lamination_settings(true, &brigader)
            .await
            .expect("brigader updates lamination settings")
            .layer_remainders_required
    );
    assert!(
        service
            .update_qolip_start_settings(true, &brigader)
            .await
            .expect("brigader updates mold start settings")
            .automatic_confirmation_enabled
    );

    let worker = actor("aparatchi");
    assert_eq!(
        service
            .update_paddon_settings(Some(false), None, &worker)
            .await,
        Err(ProductionMapError::PaddonInvalidInput)
    );
    assert_eq!(
        service.update_lamination_settings(false, &worker).await,
        Err(ProductionMapError::QueueActionNotAllowed)
    );
    assert_eq!(
        service.update_qolip_start_settings(false, &worker).await,
        Err(ProductionMapError::QueueActionNotAllowed)
    );
}

#[tokio::test]
async fn brigader_can_unlock_another_workers_paddon_without_free_movement() {
    let service = ProductionMapService::new_for_test(Arc::new(MemoryProductionMapStore::new()));
    let worker = actor("aparatchi");
    let paddon = service
        .create_paddon("", "", &worker)
        .await
        .expect("paddon");
    service
        .confirm_paddon_print(&paddon.code, &worker)
        .await
        .expect("lock paddon");

    let mut other_worker = worker.clone();
    other_worker.ref_ = "another_worker".to_string();
    assert!(
        !service
            .can_unlock_paddon(&paddon.code, &other_worker)
            .await
            .unwrap()
    );
    assert_eq!(
        service.unlock_paddon(&paddon.code, &other_worker).await,
        Err(ProductionMapError::PaddonUnlockForbidden)
    );

    let brigader = actor("brigader");
    assert!(
        service
            .can_unlock_paddon(&paddon.code, &brigader)
            .await
            .unwrap()
    );
    let unlocked = service
        .unlock_paddon(&paddon.code, &brigader)
        .await
        .unwrap();
    assert!(unlocked.locked_at_unix.is_none());
    assert_eq!(unlocked.created_by_ref, worker.ref_);
    assert_eq!(brigader.role, "brigader");
}
