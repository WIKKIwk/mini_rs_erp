use std::sync::Arc;

use super::*;
use crate::core::auth::models::{Principal, PrincipalRole};

fn actor() -> Principal {
    Principal {
        role: PrincipalRole::Qolipchi,
        ref_: "clerk-1".into(),
        display_name: "Qolipchi".into(),
        legal_name: "Qolipchi".into(),
        phone: String::new(),
        avatar_url: String::new(),
    }
}

async fn fixture() -> (Arc<MemoryQolipStore>, QolipService) {
    let store = Arc::new(MemoryQolipStore::new());
    store
        .seed_blocks(vec![QolipBlock {
            name: "Block A".into(),
            warehouse: "Qolip ombor".into(),
        }])
        .await;
    store
        .seed_products(
            ["ITEM-A", "ITEM-B"]
                .into_iter()
                .map(|code| QolipProduct {
                    code: code.into(),
                    name: code.into(),
                    item_group: "Flexo".into(),
                    ..Default::default()
                })
                .collect(),
        )
        .await;
    let service = QolipService::new(store.clone());
    let set = service
        .create_set(
            &QolipSetCreate {
                warehouse: "Qolip ombor".into(),
                members: ["Q-A", "Q-B"]
                    .into_iter()
                    .map(|code| QolipMoldDraft {
                        qolip_code: code.into(),
                        size: 420,
                        color: "BLACK".into(),
                        ..Default::default()
                    })
                    .collect(),
            },
            &actor(),
            false,
        )
        .await
        .unwrap();
    for product in ["ITEM-A", "ITEM-B"] {
        service
            .reuse_shared_set(
                &QolipSetReuse {
                    set_id: set.snapshot.set_id.clone(),
                    fingerprint: set.snapshot.fingerprint.clone(),
                    item_code: product.into(),
                },
                &actor(),
                false,
            )
            .await
            .unwrap();
    }
    (store, service)
}

fn send(request_id: &str, codes: &[&str]) -> QolipReworkSend {
    QolipReworkSend {
        request_id: request_id.into(),
        qolip_codes: codes.iter().map(|code| (*code).into()).collect(),
    }
}

fn receive(request_id: &str, entries: &[&QolipRework]) -> QolipReworkReturn {
    QolipReworkReturn {
        request_id: request_id.into(),
        rework_ids: entries.iter().map(|entry| entry.id.clone()).collect(),
    }
}

#[tokio::test]
async fn rework_partial_returns_enable_only_selected_mileage_and_stale_ids_do_not_reset_later_visits()
 {
    let (store, service) = fixture().await;
    assert!(!service.mileage("Q-A").await.unwrap().tracking_enabled);
    let sent = service
        .send_reworks(send("send-1", &[" q-b ", "Q-A", "q-a"]), &actor(), false)
        .await
        .unwrap();
    assert_eq!(sent.len(), 2);
    let a = sent.iter().find(|entry| entry.qolip_code == "Q-A").unwrap();
    let b = sent.iter().find(|entry| entry.qolip_code == "Q-B").unwrap();
    assert_eq!(
        service
            .send_reworks(send("send-1", &["q-a", "q-b"]), &actor(), false)
            .await
            .unwrap(),
        sent
    );
    assert_eq!(
        service
            .send_reworks(send("send-1", &["q-a"]), &actor(), false)
            .await
            .unwrap_err(),
        QolipError::ReworkRequestConflict
    );
    service
        .return_reworks(receive("return-a", &[a]), &actor(), false)
        .await
        .unwrap();
    assert_eq!(
        service.reworks("open", &actor(), false).await.unwrap(),
        vec![b.clone()]
    );
    let first = service.mileage("Q-A").await.unwrap();
    assert_eq!(first.current_meter, Some(0.0));
    assert_eq!(first.cycles.len(), 1);
    assert!(!service.mileage("Q-B").await.unwrap().tracking_enabled);
    service
        .return_reworks(receive("return-a", &[a]), &actor(), false)
        .await
        .unwrap();
    assert_eq!(service.mileage("Q-A").await.unwrap().cycles.len(), 1);
    let next = service
        .send_reworks(send("send-a-again", &["Q-A"]), &actor(), false)
        .await
        .unwrap();
    assert_eq!(
        service
            .return_reworks(receive("stale-old-return", &[a]), &actor(), false)
            .await
            .unwrap_err(),
        QolipError::ReworkConflict
    );
    assert_eq!(
        store.active_rework_codes(&["Q-A".into()]).await.unwrap(),
        vec!["Q-A"]
    );
    assert_eq!(service.mileage("Q-A").await.unwrap().cycles.len(), 1);
    service
        .return_reworks(receive("return-a-again", &[&next[0]]), &actor(), false)
        .await
        .unwrap();
    let cycles = service.mileage("Q-A").await.unwrap().cycles;
    assert_eq!(cycles.len(), 2);
    assert!(cycles[0].ended_at.is_some());
    assert!(cycles[1].ended_at.is_none());
    assert_eq!(cycles[1].meter, 0.0);
    assert_eq!(
        service.reworks("all", &actor(), false).await.unwrap().len(),
        3
    );
}

#[tokio::test]
async fn rework_shared_mold_blocks_both_products_despite_an_intact_alternate_set() {
    let (_, service) = fixture().await;
    let alternate = service
        .create_set(
            &QolipSetCreate {
                warehouse: "Qolip ombor".into(),
                members: vec![QolipMoldDraft {
                    qolip_code: "ALT-A".into(),
                    size: 420,
                    color: "BLACK".into(),
                    ..Default::default()
                }],
            },
            &actor(),
            false,
        )
        .await
        .unwrap();
    for item in ["ITEM-A", "ITEM-B"] {
        service
            .reuse_shared_set(
                &QolipSetReuse {
                    set_id: alternate.snapshot.set_id.clone(),
                    fingerprint: alternate.snapshot.fingerprint.clone(),
                    item_code: item.into(),
                },
                &actor(),
                false,
            )
            .await
            .unwrap();
        service
            .require_qolips_available_for_layout(&[], item, item)
            .await
            .unwrap();
    }
    let sent = service
        .send_reworks(send("shared-away", &["Q-A"]), &actor(), false)
        .await
        .unwrap();
    for item in ["ITEM-A", "ITEM-B"] {
        assert_eq!(
            service
                .require_qolips_available_for_layout(&[], item, item)
                .await
                .unwrap_err(),
            QolipError::QolipInRework
        );
        let required = service.required_qolips_for_order(item, item).await.unwrap();
        assert_eq!(
            required.len(),
            3,
            "away mold remains required catalog composition"
        );
    }
    service
        .return_reworks(receive("shared-back", &[&sent[0]]), &actor(), false)
        .await
        .unwrap();
    for item in ["ITEM-A", "ITEM-B"] {
        service
            .require_qolips_available_for_layout(&[], item, item)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn rework_preserves_home_stock_and_legacy_identity_without_phantom_return_copies() {
    let (store, service) = fixture().await;
    let location = QolipLocation {
        id: "legacy-home".into(),
        block: "Block A".into(),
        warehouse: "Qolip ombor".into(),
        item_code: "ITEM-A".into(),
        item_name: "ITEM-A".into(),
        qolip_code: "LEGACY".into(),
        size: 400,
        quantity: 1,
        row_letter: "A".into(),
        column_number: Some(1),
        location_label: "A1".into(),
        created_by_role: "qolipchi".into(),
        created_by_ref: "clerk-1".into(),
        created_by_name: "Qolipchi".into(),
    };
    QolipStorePort::put_location(&*store, location.clone())
        .await
        .unwrap();
    let sent = service
        .send_reworks(send("legacy-away", &["LEGACY"]), &actor(), false)
        .await
        .unwrap();
    assert!(
        QolipStorePort::locations(&*store, "Block A")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        QolipStorePort::location_by_id(&*store, &location.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .product_spec_by_qolip_code("LEGACY")
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        QolipStorePort::delete_product_specs(&*store, &["LEGACY".into()])
            .await
            .unwrap_err(),
        QolipError::QolipInRework
    );
    assert_eq!(
        QolipStorePort::put_location(&*store, location.clone())
            .await
            .unwrap_err(),
        QolipError::QolipInRework
    );
    service
        .return_reworks(receive("legacy-back", &[&sent[0]]), &actor(), false)
        .await
        .unwrap();
    service
        .return_reworks(receive("legacy-back", &[&sent[0]]), &actor(), false)
        .await
        .unwrap();
    assert_eq!(
        QolipStorePort::locations(&*store, "Block A").await.unwrap(),
        vec![location]
    );
}

#[tokio::test]
async fn rework_batch_validation_is_atomic_and_retries_recheck_warehouse_scope() {
    let (store, service) = fixture().await;
    let invalid = service
        .send_reworks(send("invalid-batch", &["Q-A", "MISSING"]), &actor(), false)
        .await;
    assert_eq!(invalid.unwrap_err(), QolipError::QolipCodeNotFound);
    assert!(
        service
            .reworks("all", &actor(), false)
            .await
            .unwrap()
            .is_empty()
    );
    let request = send("scoped", &["q-a"]);
    let denied =
        QolipStorePort::send_reworks(&*store, &request, &actor(), Some(&["Other".into()])).await;
    assert_eq!(denied.unwrap_err(), QolipError::AccessDenied);
    service
        .send_reworks(request.clone(), &actor(), false)
        .await
        .unwrap();
    let retry =
        QolipStorePort::send_reworks(&*store, &request, &actor(), Some(&["Other".into()])).await;
    assert_eq!(retry.unwrap_err(), QolipError::AccessDenied);
}

#[tokio::test]
async fn rework_concurrent_send_only_opens_one_visit() {
    let (_, service) = fixture().await;
    let principal = actor();
    let (first, second) = tokio::join!(
        service.send_reworks(send("race-a", &["Q-A"]), &principal, false),
        service.send_reworks(send("race-b", &["Q-A"]), &principal, false),
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let error = first.err().or(second.err()).unwrap();
    assert_eq!(error, QolipError::QolipInRework);
}
