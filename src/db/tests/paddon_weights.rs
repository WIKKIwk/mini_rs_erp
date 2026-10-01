use super::*;
use crate::core::production_map::{QueueActionActor, paddon_snapshot_token};
use sqlx::PgPool;

#[tokio::test]
async fn postgres_paddon_product_totals_membership_corrections_and_frozen_receipts() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").expect("isolated test URL");
    let db_name = format!(
        "mini_rs_erp_test_paddon_weights_{:08x}",
        rand::random::<u32>()
    );
    let admin = PgPool::connect(&admin_url).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(postgres_test_database_options(&admin_url, &db_name))
        .await
        .unwrap();
    crate::db::postgres::apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    seed_standard_canonical_apparatus(&pool).await;
    apply_foundation_migration(&pool).await.unwrap();
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new_for_test(store.clone());
    let station = "apparatus:default:asset-010";
    let actor = QueueActionActor {
        role: "omborchi".into(),
        ref_: "worker-wip-suffix".into(),
        display_name: "Keeper".into(),
    };
    let mut map = test_map("weights-order", "89001", "PRODUCT-1");
    map.nodes[1].apparatus_id = station.into();
    map.nodes[1].title = "Rezka".into();
    map.nodes[1].rezka_kadr_count = Some(1);
    service.upsert_map(map).await.unwrap();
    let p = service.create_paddon("Rezka", "", &actor).await.unwrap();
    let second = service.create_paddon("Rezka", "", &actor).await.unwrap();
    assert_eq!((p.total_gross_kg, p.total_net_kg), (Some(0.0), Some(0.0)));
    let mut b = wip_batch(station);
    b.batch_id = "weights-roll".into();
    b.qr_payload = "weights-qr".into();
    b.order_id = "weights-order".into();
    b.apparatus = station.into();
    b.next_apparatus.clear();
    b.action = queue_state::ApparatusQueueAction::RollComplete;
    b.status = OrderProgressBatchStatus::Completed;
    b.produced_qty = 100.0;
    b.uom = "m".into();
    b.finished_goods_kg = Some(10.0);
    b.bobina_kg = Some(0.5);
    b.payload_json = serde_json::json!({"gross_qty":12.123456});
    b.refresh_status_detail();
    store.put_order_progress_batch(b.clone()).await.unwrap();
    let ids = vec![b.batch_id.clone()];
    for _ in 0..2 {
        service
            .add_paddon_items(&p.code, &ids, &actor)
            .await
            .unwrap();
    }
    let before = service.paddon_scan_snapshot(&p.code).await.unwrap();
    assert_eq!(
        (before.paddon.total_gross_kg, before.paddon.total_net_kg),
        (Some(12.123456), Some(11.623456))
    );
    let detail = service.paddon_snapshot(&p.code).await.unwrap();
    assert_eq!(before.paddon, detail.paddon);
    let listed = service.paddons(1).await.unwrap();
    assert_eq!(listed[0].total_gross_kg, Some(12.123456));
    service
        .remove_paddon_items(&p.code, &ids, &actor)
        .await
        .unwrap();
    service
        .add_paddon_items(&second.code, &ids, &actor)
        .await
        .unwrap();
    assert_eq!(
        service
            .paddon_summary(&p.code)
            .await
            .unwrap()
            .total_gross_kg,
        Some(0.0)
    );
    // Unavailable membership is excluded, and missing core is preserved as unknown.
    b.wip_status = OrderProgressBatchWipStatus::InUse;
    store.put_order_progress_batch(b.clone()).await.unwrap();
    assert_eq!(
        service
            .paddon_summary(&second.code)
            .await
            .unwrap()
            .total_gross_kg,
        Some(0.0)
    );
    b.wip_status = OrderProgressBatchWipStatus::Waiting;
    b.bobina_kg = None;
    store.put_order_progress_batch(b.clone()).await.unwrap();
    assert_eq!(
        service
            .paddon_summary(&second.code)
            .await
            .unwrap()
            .total_net_kg,
        None
    );
    b.bobina_kg = Some(0.5);
    store.put_order_progress_batch(b.clone()).await.unwrap();
    // Historical actual kg audit predates this fix and leaves a stale explicit gross.
    sqlx::query("INSERT INTO mini_progress_batch_corrections
        (batch_id, previous_revision, new_revision, reason, actor_role, actor_ref, actor_display_name, old_values, new_values)
        VALUES ($1,1,2,'historical kg','aparatchi','worker','Worker',$2,$3)")
        .bind(&b.batch_id).bind(serde_json::json!({"finished_goods_kg":10.0}))
        .bind(serde_json::json!({"finished_goods_kg":11.0})).execute(&pool).await.unwrap();
    b.revision = 2;
    b.finished_goods_kg = Some(11.0);
    b.payload_json["correction_revision"] = serde_json::json!(2);
    sqlx::query("UPDATE mini_progress_batches SET revision=2, finished_goods_kg=11, payload_json=$2 WHERE batch_id=$1")
        .bind(&b.batch_id).bind(&b.payload_json).execute(&pool).await.unwrap();
    let corrected = service.paddon_scan_snapshot(&second.code).await.unwrap();
    assert_eq!(
        (
            corrected.paddon.total_gross_kg,
            corrected.paddon.total_net_kg
        ),
        (Some(11.0), Some(10.5))
    );
    assert!(
        service
            .receive_paddon(
                &second.code,
                "WH-1",
                &ids,
                &paddon_snapshot_token(&before),
                actor.clone()
            )
            .await
            .is_err()
    );
    // Current public correction workflow preserves gross on core/meter/description edits.
    let mut edit: crate::core::production_map::ProgressBatchCorrectionInput = serde_json::from_value(serde_json::json!({
        "batch_id":b.batch_id,"expected_revision":b.revision,"produced_qty":b.produced_qty,"uom":b.uom,
        "finished_goods_kg":b.finished_goods_kg,"bobina_kg":b.bobina_kg,"description":"Corrected note",
        "reason":"Measurement review"
    })).unwrap();
    b = service
        .correct_progress_batch(edit.clone(), &actor)
        .await
        .unwrap();
    assert_eq!(b.payload_json["gross_qty"], serde_json::json!(12.123456));
    edit.expected_revision = b.revision;
    edit.finished_goods_kg = Some(11.0000001);
    service
        .correct_progress_batch(edit.clone(), &actor)
        .await
        .unwrap();
    b = store.progress_batch(&b.batch_id).await.unwrap().unwrap();
    assert_eq!(
        service
            .paddon_summary(&second.code)
            .await
            .unwrap()
            .total_gross_kg,
        Some(11.0)
    );
    edit.expected_revision = b.revision;
    edit.finished_goods_kg = Some(11.5);
    b = service
        .correct_progress_batch(edit.clone(), &actor)
        .await
        .unwrap();
    assert_eq!(b.payload_json["gross_qty"], serde_json::json!(11.5));
    edit.expected_revision = b.revision;
    edit.finished_goods_kg = Some(11.0);
    b = service.correct_progress_batch(edit, &actor).await.unwrap();
    let mut unknown = b.clone();
    unknown.batch_id = "unknown-roll".into();
    unknown.qr_payload = "unknown-qr".into();
    unknown.payload_json = serde_json::json!({});
    unknown.finished_goods_kg = None;
    store
        .put_order_progress_batch(unknown.clone())
        .await
        .unwrap();
    service
        .add_paddon_item(&second.code, &unknown.batch_id, &actor)
        .await
        .unwrap();
    assert_eq!(
        service
            .paddon_summary(&second.code)
            .await
            .unwrap()
            .total_gross_kg,
        None
    );
    service
        .remove_paddon_item(&second.code, &unknown.batch_id, &actor)
        .await
        .unwrap();
    let available = service.paddon_snapshot(&second.code).await.unwrap();
    assert!(
        available
            .available_items
            .iter()
            .any(|item| item.batch_id == unknown.batch_id)
    );
    let corrected = service.paddon_scan_snapshot(&second.code).await.unwrap();
    let receipt = service
        .receive_paddon(
            &second.code,
            "WH-1",
            &ids,
            &paddon_snapshot_token(&corrected),
            actor.clone(),
        )
        .await
        .unwrap();
    assert_eq!(receipt.paddon.total_gross_kg, Some(11.0));
    // Simulate a legacy frozen receipt; future audits must not rewrite its old revision.
    let mut legacy = serde_json::to_value(&receipt).unwrap();
    legacy["paddon"]
        .as_object_mut()
        .unwrap()
        .remove("total_gross_kg");
    legacy["paddon"]
        .as_object_mut()
        .unwrap()
        .remove("total_net_kg");
    sqlx::query("UPDATE mini_paddons SET receipt_json=$2 WHERE code=$1")
        .bind(&second.code)
        .bind(legacy)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO mini_progress_batch_corrections
        (batch_id, previous_revision, new_revision, reason, actor_role, actor_ref, actor_display_name, old_values, new_values)
        VALUES ($1,$4,$5,'future kg','aparatchi','worker','Worker',$2,$3)")
        .bind(&b.batch_id).bind(serde_json::json!({"finished_goods_kg":11.0}))
        .bind(serde_json::json!({"finished_goods_kg":99.0})).bind(b.revision as i64).bind((b.revision + 1) as i64)
    .execute(&pool).await.unwrap();
    let retry = service
        .receive_paddon(&second.code, "WH-1", &ids, "old-token", actor.clone())
        .await
        .unwrap();
    assert_eq!(
        (retry.paddon.total_gross_kg, retry.paddon.total_net_kg),
        (Some(11.0), Some(10.5))
    );
    assert!(
        service
            .receive_paddon(&second.code, "WH-2", &ids, "old-token", actor)
            .await
            .is_err()
    );
    assert_eq!(
        service
            .paddon_summary(&second.code)
            .await
            .unwrap()
            .total_gross_kg,
        Some(11.0)
    );
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{db_name}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
}
