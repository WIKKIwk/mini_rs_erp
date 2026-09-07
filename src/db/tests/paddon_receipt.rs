use super::*;
use crate::core::production_map::{QueueActionActor, paddon_snapshot_token};

#[tokio::test]
async fn postgres_werka_paddon_receipt_atomic_retry_and_locks() {
    let admin_url =
        std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").expect("isolated PostgreSQL test URL");
    let db_name = format!("mini_rs_erp_test_paddon_{:08x}", rand::random::<u32>());
    let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\" "))
        .execute(&admin)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &db_name))
        .await
        .unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    seed_standard_canonical_apparatus(&pool).await;
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new_for_test(store.clone());
    let mut map = test_map("receipt-order", "89001", "PRODUCT-1");
    // Use the canonical cutting station, not a label-based guess.
    let station: String = sqlx::query_scalar(
        "SELECT id FROM mini_apparatus WHERE lower(name) LIKE '%rezka%' ORDER BY id LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    map.nodes[1].apparatus_id = station.clone();
    map.nodes[1].title = station.clone();
    service.upsert_map(map).await.unwrap();
    let actor = QueueActionActor {
        role: "werka".into(),
        ref_: "keeper-1".into(),
        display_name: "Omborchi".into(),
    };
    let paddon = service.create_paddon("Rezka", "", &actor).await.unwrap();
    let mut ids = Vec::new();
    for i in 0..2 {
        let mut b = wip_batch(&station);
        b.batch_id = format!("paddon-roll-{i}");
        b.qr_payload = format!("PROGRESS:roll-{i}");
        b.apparatus = station.clone();
        b.order_id = "receipt-order".into();
        b.next_apparatus.clear();
        b.action = queue_state::ApparatusQueueAction::RollComplete;
        b.status = OrderProgressBatchStatus::Completed;
        b.finished_goods_kg = Some(10.0 + i as f64);
        b.refresh_status_detail();
        ids.push(b.batch_id.clone());
        store.put_order_progress_batch(b).await.unwrap();
    }
    service
        .add_paddon_items(&paddon.code, &ids, &actor)
        .await
        .unwrap();
    let preview = service.paddon_scan_snapshot(&paddon.code).await.unwrap();
    let token = paddon_snapshot_token(&preview);
    assert!(
        service
            .receive_paddon(&paddon.code, "WH-1", &ids, "stale", actor.clone())
            .await
            .is_err()
    );
    assert!(
        service
            .receive_paddon(&paddon.code, "WH-1", &ids[..1], &token, actor.clone())
            .await
            .is_err()
    );
    // An invalid second roll must not leave the first roll partially received.
    let mut invalid = store.progress_batch(&ids[1]).await.unwrap().unwrap();
    invalid.wip_status = OrderProgressBatchWipStatus::InUse;
    store
        .put_order_progress_batch(invalid.clone())
        .await
        .unwrap();
    let bad = service.paddon_scan_snapshot(&paddon.code).await.unwrap();
    assert!(
        service
            .receive_paddon(
                &paddon.code,
                "WH-1",
                &ids,
                &paddon_snapshot_token(&bad),
                actor.clone()
            )
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mini_finished_goods_stock")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    invalid.wip_status = OrderProgressBatchWipStatus::Waiting;
    store.put_order_progress_batch(invalid).await.unwrap();
    let ready = service.paddon_scan_snapshot(&paddon.code).await.unwrap();
    let token = paddon_snapshot_token(&ready);
    // Force a database failure on the final stock insert, after the first insert.
    sqlx::query("ALTER TABLE mini_finished_goods_stock ADD CONSTRAINT test_receipt_failure CHECK (qty <> 10)").execute(&pool).await.unwrap();
    assert!(
        service
            .receive_paddon(&paddon.code, "WH-1", &ids, &token, actor.clone())
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mini_finished_goods_stock")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert!(
        service
            .paddon_receipt(&paddon.code)
            .await
            .unwrap()
            .is_none()
    );
    for id in &ids {
        assert_eq!(
            store.progress_batch(id).await.unwrap().unwrap().wip_status,
            OrderProgressBatchWipStatus::Waiting
        );
    }
    sqlx::query("ALTER TABLE mini_finished_goods_stock DROP CONSTRAINT test_receipt_failure")
        .execute(&pool)
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        service.receive_paddon(&paddon.code, "WH-1", &ids, &token, actor.clone()),
        service.receive_paddon(&paddon.code, "WH-1", &ids, &token, actor.clone())
    );
    let receipt = a.unwrap();
    assert_eq!(receipt, b.unwrap());
    assert_eq!(receipt.stocks.len(), 2);
    assert_eq!(receipt.stocks.iter().map(|s| s.qty).sum::<f64>(), 21.0);
    assert!(
        receipt.items.iter().all(|b| b.current_location == "WH-1"
            && b.wip_status == OrderProgressBatchWipStatus::Processed)
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mini_finished_goods_stock")
            .fetch_one(&pool)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        service
            .paddon_scan_snapshot(&paddon.code)
            .await
            .unwrap()
            .paddon
            .location,
        "WH-1"
    );
    assert!(
        service
            .receive_paddon(&paddon.code, "WH-2", &ids, &token, actor.clone())
            .await
            .is_err()
    );
    assert!(
        service
            .remove_paddon_items(&paddon.code, &ids, &actor)
            .await
            .is_err()
    );
    assert!(
        service
            .add_paddon_items(&paddon.code, &ids, &actor)
            .await
            .is_err()
    );
    // An old single-roll write must not change a committed pallet receipt.
    let mut stale_stock = receipt.stocks[0].clone();
    stale_stock.warehouse = "WH-2".into();
    assert!(
        store
            .receive_finished_goods_batch(receipt.items[0].clone(), stale_stock)
            .await
            .is_err()
    );
    let retry_write = crate::core::production_map::PaddonReceiveWrite {
        originals: ready.items,
        receipt: receipt.clone(),
    };
    let other_store = PostgresProductionMapStore::new(pool.clone());
    let (a, b) = tokio::join!(
        store.receive_paddon(retry_write.clone()),
        other_store.receive_paddon(retry_write)
    );
    assert_eq!(a.unwrap(), receipt);
    assert_eq!(b.unwrap(), receipt);
    let restarted =
        ProductionMapService::new_for_test(Arc::new(PostgresProductionMapStore::new(pool.clone())));
    assert_eq!(
        restarted
            .receive_paddon(&paddon.code, "WH-1", &ids, &token, actor)
            .await
            .unwrap(),
        receipt
    );
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{db_name}\" "))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
