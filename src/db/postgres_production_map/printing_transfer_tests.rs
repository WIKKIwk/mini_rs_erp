use super::*;

const FROM: &str = "apparatus:default:bosma_7";
const TO: &str = "apparatus:default:bosma_8";

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "admin".into(),
        ref_: "transfer-admin".into(),
        display_name: "Admin".into(),
    }
}

fn request(order: &str) -> ProductionMapApparatusTransferRequest {
    ProductionMapApparatusTransferRequest {
        order_id: order.into(),
        from_apparatus: FROM.into(),
        to_apparatus: TO.into(),
        reason: "Press failure".into(),
        idempotency_key: format!("transfer-{order}"),
    }
}

async fn seed(store: &PostgresProductionMapStore, order: &str, status: OrderRunStatus) {
    let map = serde_json::from_value(serde_json::json!({
        "id":order,"product_code":"TRANSFER","title":"Print failover","roll_count":7,"width_mm":800,
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"print","kind":"apparatus","title":"Bosma 7","apparatus_id":FROM},
            {"id":"lam","kind":"apparatus","title":"Lamination","apparatus_id":LAM},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"print"},{"from":"print","to":"lam"},{"from":"lam","to":"end"}]
    })).unwrap();
    store.put_map(map).await.unwrap();
    store
        .put_apparatus_queue_states(FROM, [(order.into(), "paused".into())].into())
        .await
        .unwrap();
    store
        .put_apparatus_sequence(FROM, vec![order.into()])
        .await
        .unwrap();
    store
        .put_order_run_session(OrderRunSession {
            session_id: format!("run-{order}"),
            apparatus: FROM.into(),
            order_id: order.into(),
            stage_node_id: "print".into(),
            status,
            worker_role: "aparatchi".into(),
            worker_ref: "print-worker".into(),
            worker_display_name: "Printer".into(),
            started_at_unix: 1_700_000_000,
            updated_at_unix: 1_700_000_010,
            payload_json: serde_json::json!({"qolip_lock_owner":false}),
        })
        .await
        .unwrap();
}

fn service(pool: &PgPool, store: Arc<PostgresProductionMapStore>) -> ProductionMapService {
    let canonical = CanonicalApparatusService::new(Arc::new(
        PostgresCanonicalApparatusRepository::new(pool.clone()),
    ));
    ProductionMapService::new(
        store,
        Arc::new(CanonicalServiceApparatusResolver::new(canonical)),
    )
}

#[tokio::test]
#[ignore = "requires an explicitly isolated PostgreSQL test URL; creates and drops a test database"]
async fn postgres_printing_transfer_preserves_live_wip_and_replays_across_service_instances() {
    let (pool, store, admin, name) = fixture().await;
    for (index, wip_status) in [
        OrderProgressBatchWipStatus::Waiting,
        OrderProgressBatchWipStatus::InUse,
        OrderProgressBatchWipStatus::Processed,
    ]
    .into_iter()
    .enumerate()
    {
        let order = format!("zakaz-pg-transfer-{index}");
        seed(&store, &order, OrderRunStatus::RollDetached).await;
        let mut batch = original_batch();
        batch.batch_id = format!("roll-{order}");
        batch.session_id = format!("run-{order}");
        batch.order_id = order.clone();
        batch.apparatus = FROM.into();
        batch.current_apparatus = FROM.into();
        batch.next_apparatus = LAM.into();
        batch.qr_payload = format!("QR-{order}");
        batch.payload_json =
            serde_json::json!({"stage_node_id":"print","next_stage_node_id":"lam"});
        batch.wip_status = wip_status;
        if wip_status != OrderProgressBatchWipStatus::Waiting {
            batch.current_apparatus = LAM.into();
            batch.used_by_apparatus = LAM.into();
            batch.used_by_session_id = format!("lam-{order}");
            if wip_status == OrderProgressBatchWipStatus::Processed {
                batch.processed_by_apparatus = LAM.into();
                batch.processed_by_session_id = format!("lam-{order}");
            }
        }
        batch.refresh_status_detail();
        store.put_order_progress_batch(batch.clone()).await.unwrap();
        let before = store
            .progress_batch(&batch.batch_id)
            .await
            .unwrap()
            .unwrap();
        let first = service(&pool, store.clone());
        let second = service(&pool, store.clone());
        let (left, right) = tokio::join!(
            first.transfer_apparatus_order(request(&order), actor()),
            second.transfer_apparatus_order(request(&order), actor())
        );
        let left = left.unwrap();
        let right = right.unwrap();
        assert_eq!(left.transfer, right.transfer);
        assert_eq!(
            store.progress_batch(&batch.batch_id).await.unwrap(),
            Some(before)
        );
        assert_eq!(
            store
                .order_run_session(&format!("run-{order}"))
                .await
                .unwrap()
                .unwrap()
                .apparatus,
            TO
        );
        assert_eq!(
            store.apparatus_queue_states().await.unwrap()[TO][&order],
            "paused"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM mini_apparatus_order_transfers WHERE order_id=$1"
            )
            .bind(&order)
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
        drop(first);
        drop(second);
    }
    cleanup(pool, store, admin, name).await;
}

#[tokio::test]
#[ignore = "requires an explicitly isolated PostgreSQL test URL; creates and drops a test database"]
async fn postgres_printing_transfer_rolls_back_map_queue_and_session_when_material_transfer_fails()
{
    let (pool, store, admin, name) = fixture().await;
    let order = "zakaz-pg-transfer-rollback";
    seed(&store, order, OrderRunStatus::Paused).await;
    sqlx::raw_sql("INSERT INTO mini_item_groups(name,parent_item_group) VALUES('Film','All Item Groups') ON CONFLICT DO NOTHING;
        INSERT INTO mini_items(code,name,item_group) VALUES('FILM','Film','Film') ON CONFLICT DO NOTHING;")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,barcode,qty,status,reserved_order_id) VALUES('stock','Test','FILM','TRANSFER-STOCK',1,'in_use','other-order')")
        .execute(&pool).await.unwrap();
    let assignment = RawMaterialAssignment {
        order_id: order.into(),
        apparatus_id: ApparatusId::new(FROM).unwrap(),
        apparatus: "Bosma 7".into(),
        barcode: "TRANSFER-STOCK".into(),
        item_code: "FILM".into(),
        item_name: "Film".into(),
        item_group: "Film".into(),
        assigned_by_role: "admin".into(),
        assigned_by_ref: "admin".into(),
        assigned_by_display_name: "Admin".into(),
        assigned_at: "now".into(),
    };
    sqlx::query("INSERT INTO mini_raw_material_assignments(barcode,order_id,apparatus,canonical_apparatus_id,item_code,item_group,payload_json) VALUES($1,$2,'Bosma 7',$3,'FILM','Film',$4)")
        .bind(&assignment.barcode).bind(order).bind(FROM).bind(serde_json::to_value(&assignment).unwrap()).execute(&pool).await.unwrap();
    let before_map = store.maps().await.unwrap();
    let before_session = store
        .order_run_session(&format!("run-{order}"))
        .await
        .unwrap();
    let before_states = store.apparatus_queue_states().await.unwrap();
    let service = service(&pool, store.clone());
    assert_eq!(
        service
            .transfer_apparatus_order(request(order), actor())
            .await,
        Err(ProductionMapError::RawMaterialStockUnavailable)
    );
    assert_eq!(store.maps().await.unwrap(), before_map);
    assert_eq!(
        store
            .order_run_session(&format!("run-{order}"))
            .await
            .unwrap(),
        before_session
    );
    assert_eq!(store.apparatus_queue_states().await.unwrap(), before_states);
    assert!(
        store
            .apparatus_transfer_by_idempotency_key(&request(order).idempotency_key)
            .await
            .unwrap()
            .is_none()
    );
    // A consumed assignment is historical usage; it must neither block a
    // reopened run's transfer nor be moved to the replacement press.
    sqlx::query("UPDATE mini_raw_material_stock SET status='consumed' WHERE barcode='TRANSFER-STOCK'")
        .execute(&pool).await.unwrap();
    let result = service
        .transfer_apparatus_order(request(order), actor())
        .await
        .unwrap();
    assert!(result.transfer.material_barcodes.is_empty());
    assert_eq!(
        store.raw_material_assignments().await.unwrap()[0]
            .apparatus_id
            .as_str(),
        FROM
    );
    drop(service);
    cleanup(pool, store, admin, name).await;
}
