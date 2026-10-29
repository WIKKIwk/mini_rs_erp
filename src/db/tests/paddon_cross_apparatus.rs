use super::*;
use crate::core::production_map::{
    CanonicalServiceApparatusResolver, OrderProgressBatch, PaddonSummary, PaddonTransferInput,
    ProductionMapError, ProductionMapService, ProductionMapStorePort, QueueActionActor,
};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    postgres_test_database_options,
};
use crate::db::postgres_production_map::PostgresProductionMapStore;

const CUT1: &str = "apparatus:default:asset-010";
const CUT5: &str = "apparatus:test:rezka-5";
const CUT3: &str = "apparatus:test:rezka-3";

struct Fixture {
    admin: PgPool,
    pool: PgPool,
    runtime: PgPool,
    database: String,
    store: Arc<PostgresProductionMapStore>,
    service: ProductionMapService,
    worker: QueueActionActor,
    manager: QueueActionActor,
    allowed: Vec<String>,
}

impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
        let admin = PgPool::connect(&url).await.unwrap();
        let database = format!(
            "mini_rs_erp_test_paddon_cross_{:016x}",
            rand::random::<u64>()
        );
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await
            .unwrap();
        let pool = PgPool::connect_with(postgres_test_database_options(&url, &database))
            .await
            .unwrap();
        apply_postgres_migrations_through_version(&pool, "0121")
            .await
            .unwrap();
        seed_standard_canonical_apparatus(&pool).await;
        for (id, name) in [(CUT5, "Rezka 5"), (CUT3, "Rezka 3")] {
            seed_canonical_apparatus(&pool, TestApparatusSpec::cut(id, name)).await;
        }
        apply_foundation_migration(&pool).await.unwrap();
        let runtime = sqlx::postgres::PgPoolOptions::new()
            .after_connect(|conn, _| {
                Box::pin(async move {
                    sqlx::query("SET ROLE mini_rs_erp").execute(conn).await?;
                    Ok(())
                })
            })
            .connect_with(postgres_test_database_options(&url, &database))
            .await
            .unwrap();
        let store = Arc::new(PostgresProductionMapStore::new(runtime.clone()));
        let canonical = CanonicalApparatusService::new(Arc::new(
            PostgresCanonicalApparatusRepository::new(runtime.clone()),
        ));
        let service = ProductionMapService::new(
            store.clone(),
            Arc::new(CanonicalServiceApparatusResolver::new(canonical)),
        );
        for (number, apparatus) in [(9851, CUT1), (9855, CUT5), (9853, CUT3)] {
            let order = format!("paddon-cross-order-{number}");
            service.upsert_map(serde_json::from_value(serde_json::json!({
                "id":order,"product_code":order,"title":order,"order_number":number.to_string(),
                "nodes":[{"id":"start","kind":"start","title":"Start"},
                    {"id":"cut","kind":"apparatus","title":"Cut","apparatus_id":apparatus,"rezka_kadr_count":1},
                    {"id":"end","kind":"end","title":"End"}],
                "edges":[{"from":"start","to":"cut"},{"from":"cut","to":"end"}]
            })).unwrap()).await.unwrap();
        }
        Self {
            admin,
            pool,
            runtime,
            database,
            store,
            service,
            worker: QueueActionActor {
                role: "aparatchi".into(),
                ref_: "multi-cut-worker".into(),
                display_name: "Worker".into(),
            },
            manager: QueueActionActor {
                role: "admin".into(),
                ref_: "manager".into(),
                display_name: "Admin".into(),
            },
            allowed: vec![CUT1.into(), CUT5.into()],
        }
    }

    async fn paddon(&self, apparatus: &str, actor: &QueueActionActor) -> PaddonSummary {
        self.service
            .create_paddon_for_apparatus(Some(apparatus), "", "", actor)
            .await
            .unwrap()
    }

    async fn roll(&self, id: &str, apparatus: &str) -> OrderProgressBatch {
        let number = match apparatus {
            CUT1 => 9851,
            CUT5 => 9855,
            CUT3 => 9853,
            _ => unreachable!(),
        };
        let batch: OrderProgressBatch = serde_json::from_value(serde_json::json!({
            "batch_id":id,"revision":1,"session_id":format!("session-{id}"),
            "started_at_unix":1700000000,"completed_at_unix":1700000300,
            "apparatus":apparatus,"current_apparatus":apparatus,"current_location":apparatus,
            "order_id":format!("paddon-cross-order-{number}"),"action":"roll_complete","status":"completed",
            "produced_qty":125.5,"uom":"m","qr_payload":format!("PROGRESS:{id}"),
            "label_item_code":"PRODUCT-1","label_item_name":"Original output","executor_name":"Worker",
            "worker_role":self.worker.role,"worker_ref":self.worker.ref_,"worker_display_name":self.worker.display_name,
            "wip_status":"waiting","finished_goods_kg":23.75,"bobina_kg":0.8,"finished_goods_meter":125.5,
            "payload_json":{"stage_node_id":"cut","rezka_output_kind":"frame","gross_qty":23.75,"lineage_marker":id}
        })).unwrap();
        self.store.put_order_progress_batch(batch).await.unwrap();
        self.store.progress_batch(id).await.unwrap().unwrap()
    }

    async fn members(&self, code: &str) -> Vec<String> {
        sqlx::query_scalar("SELECT i.progress_batch_id FROM mini_paddon_items i JOIN mini_paddons p ON p.id=i.paddon_id WHERE p.code=$1 AND i.removed_at IS NULL ORDER BY i.progress_batch_id")
            .bind(code).fetch_all(&self.pool).await.unwrap()
    }

    async fn scoped_add(
        &self,
        code: &str,
        ids: &[&str],
    ) -> Result<crate::core::production_map::PaddonSnapshot, ProductionMapError> {
        self.service
            .add_paddon_items_for_apparatuses(code, &strings(ids), &self.worker, &self.allowed)
            .await
    }

    async fn transfer(
        &self,
        source: &str,
        target: &str,
        ids: &[&str],
    ) -> Result<PaddonTransferInput, ProductionMapError> {
        self.service
            .transfer_paddon_items_for_apparatuses(
                input(source, target, ids),
                &self.worker,
                &self.allowed,
            )
            .await
    }

    async fn close(self) {
        self.runtime.close().await;
        self.pool.close().await;
        sqlx::query(&format!("DROP DATABASE {} WITH (FORCE)", self.database))
            .execute(&self.admin)
            .await
            .unwrap();
        self.admin.close().await;
    }
}

fn strings(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).into()).collect()
}

fn input(source: &str, target: &str, ids: &[&str]) -> PaddonTransferInput {
    PaddonTransferInput {
        source_code: source.into(),
        target_code: target.into(),
        progress_batch_ids: strings(ids),
    }
}

#[tokio::test]
async fn paddon_cross_apparatus_receipt_preserves_each_rolls_producer_and_stock_identity() {
    let f = Fixture::new().await;
    let target = f.paddon(CUT5, &f.worker).await;
    let originals = [
        f.roll("receipt-cut1", CUT1).await,
        f.roll("receipt-cut5", CUT5).await,
    ];
    let ids: Vec<_> = originals.iter().map(|b| b.batch_id.clone()).collect();
    f.service
        .add_paddon_items_for_apparatuses(&target.code, &ids, &f.worker, &f.allowed)
        .await
        .unwrap();
    let preview = f.service.paddon_scan_snapshot(&target.code).await.unwrap();
    let receipt = f
        .service
        .receive_paddon(
            &target.code,
            "WH-CROSS",
            &ids,
            &crate::core::production_map::paddon_snapshot_token(&preview),
            QueueActionActor {
                role: "omborchi".into(),
                ref_: "keeper".into(),
                display_name: "Keeper".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(receipt.items.len(), 2);
    assert_eq!(receipt.stocks.len(), 2);
    for original in &originals {
        let batch = f
            .store
            .progress_batch(&original.batch_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(batch.apparatus, original.apparatus);
        assert_eq!(batch.session_id, original.session_id);
        assert_eq!(batch.worker_ref, original.worker_ref);
        assert_eq!(batch.produced_qty, original.produced_qty);
        assert_eq!(batch.qr_payload, original.qr_payload);
        let stock = receipt
            .stocks
            .iter()
            .find(|s| s.source_progress_batch_id == original.batch_id)
            .unwrap();
        assert_eq!(stock.barcode, original.qr_payload);
        assert_eq!(stock.order_id, original.order_id);
        assert_eq!(stock.warehouse, "WH-CROSS");
    }
    assert!(matches!(
        f.scoped_add(&target.code, &["receipt-cut1"]).await,
        Err(ProductionMapError::PaddonAlreadyReceived)
    ));
    f.close().await;
}

#[tokio::test]
async fn paddon_cross_apparatus_remove_then_add_preserves_provenance_and_available_scope() {
    let f = Fixture::new().await;
    let source = f.paddon(CUT1, &f.worker).await;
    let target = f.paddon(CUT5, &f.worker).await;
    let foreign = f.roll("unassigned-origin-in-assigned-paddon", CUT3).await;
    f.service
        .add_paddon_items(&source.code, &[foreign.batch_id.clone()], &f.manager)
        .await
        .unwrap();

    for free in [false, true] {
        f.service
            .update_paddon_management_settings(free, &f.manager)
            .await
            .unwrap();
        let id = format!("detached-output-{free}");
        let original = f.roll(&id, CUT1).await;
        f.service
            .add_paddon_items(&source.code, &[id.clone()], &f.worker)
            .await
            .unwrap();
        f.service
            .remove_paddon_items(&source.code, &[id.clone()], &f.worker)
            .await
            .unwrap();
        assert_eq!(
            f.service
                .add_paddon_items(&target.code, &[id.clone()], &f.worker)
                .await,
            Err(ProductionMapError::PaddonInvalidInput),
            "legacy APIs retain same-apparatus fallback"
        );
        let snapshot = f
            .service
            .paddon_snapshot_for_apparatuses(&target.code, &f.worker, &f.allowed)
            .await
            .unwrap();
        assert!(snapshot.available_items.iter().any(|b| b.batch_id == id));
        assert!(
            snapshot.available_items.iter().all(|b| b.apparatus != CUT3),
            "an assigned source paddon must not expose a roll from an unassigned producer"
        );
        let moved = f.scoped_add(&target.code, &[&id]).await.unwrap();
        assert_eq!(moved.paddon.apparatus_id, CUT5);
        assert!(moved.items.iter().any(|b| b == &original));
        assert_eq!(
            f.store.progress_batch(&id).await.unwrap().unwrap(),
            original,
            "producer, QR, worker, session, quantities and lineage must stay byte-for-byte equivalent"
        );
        assert!(!f.members(&source.code).await.contains(&id));
        let active: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_paddon_items WHERE progress_batch_id=$1 AND removed_at IS NULL")
            .bind(&id).fetch_one(&f.pool).await.unwrap();
        assert_eq!(active, 1);
    }
    f.close().await;
}

#[tokio::test]
async fn paddon_cross_apparatus_authorizes_both_scopes_and_rolls_back_mixed_batches() {
    let f = Fixture::new().await;
    let source = f.paddon(CUT1, &f.worker).await;
    let target = f.paddon(CUT5, &f.worker).await;
    let unassigned = f.paddon(CUT3, &f.worker).await;
    let original = f.roll("a-authorized", CUT1).await;
    f.roll("z-unauthorized", CUT3).await;
    f.service
        .add_paddon_items(
            &source.code,
            &strings(&["a-authorized", "z-unauthorized"]),
            &f.manager,
        )
        .await
        .unwrap();
    for allowed in [vec![CUT1.into()], vec![CUT5.into()], Vec::new()] {
        assert_eq!(
            f.service
                .transfer_paddon_items_for_apparatuses(
                    input(&source.code, &target.code, &["a-authorized"]),
                    &f.worker,
                    &allowed
                )
                .await,
            Err(ProductionMapError::PaddonTransferForbidden)
        );
    }
    assert_eq!(
        f.transfer(&source.code, &unassigned.code, &["a-authorized"])
            .await,
        Err(ProductionMapError::PaddonTransferForbidden)
    );
    assert_eq!(
        f.scoped_add(&unassigned.code, &["a-authorized"]).await,
        Err(ProductionMapError::PaddonTransferForbidden)
    );
    assert_eq!(
        f.transfer(
            &source.code,
            &target.code,
            &["a-authorized", "z-unauthorized"]
        )
        .await,
        Err(ProductionMapError::PaddonTransferForbidden)
    );
    assert_eq!(
        f.members(&source.code).await,
        ["a-authorized", "z-unauthorized"]
    );
    assert!(f.members(&target.code).await.is_empty());
    f.transfer(&source.code, &target.code, &["a-authorized"])
        .await
        .unwrap();
    assert_eq!(
        f.store
            .progress_batch("a-authorized")
            .await
            .unwrap()
            .unwrap(),
        original
    );
    f.service
        .remove_paddon_items(&target.code, &strings(&["a-authorized"]), &f.worker)
        .await
        .unwrap();
    f.service
        .remove_paddon_items(&source.code, &strings(&["z-unauthorized"]), &f.worker)
        .await
        .unwrap();
    for free in [false, true] {
        f.service
            .update_paddon_management_settings(free, &f.manager)
            .await
            .unwrap();
        assert_eq!(
            f.scoped_add(&target.code, &["a-authorized", "z-unauthorized"])
                .await,
            Err(ProductionMapError::PaddonTransferForbidden)
        );
        assert!(
            f.members(&target.code).await.is_empty(),
            "a later forbidden origin rolls back earlier attachments"
        );
    }
    f.close().await;
}

#[tokio::test]
async fn paddon_cross_apparatus_keeps_visibility_owner_rules_and_one_active_membership() {
    let f = Fixture::new().await;
    let other = QueueActionActor {
        ref_: "other-worker".into(),
        ..f.worker.clone()
    };
    let source = f.paddon(CUT1, &other).await;
    let target = f.paddon(CUT5, &f.worker).await;
    let hidden_target = f.paddon(CUT5, &other).await;
    let alternate = f.paddon(CUT5, &f.worker).await;
    let original = f.roll("shared-output", CUT1).await;
    f.service
        .add_paddon_items(&source.code, &strings(&["shared-output"]), &other)
        .await
        .unwrap();
    assert_eq!(
        f.transfer(&source.code, &target.code, &["shared-output"])
            .await,
        Err(ProductionMapError::PaddonNotFound)
    );
    assert_eq!(
        f.scoped_add(&hidden_target.code, &["shared-output"]).await,
        Err(ProductionMapError::PaddonNotFound)
    );
    f.service
        .update_paddon_settings(None, Some(true), &f.manager)
        .await
        .unwrap();
    assert_eq!(
        f.transfer(&source.code, &target.code, &["shared-output"])
            .await,
        Err(ProductionMapError::PaddonTransferForbidden)
    );
    f.service
        .update_paddon_settings(Some(true), Some(false), &f.manager)
        .await
        .unwrap();
    assert_eq!(
        f.scoped_add(&target.code, &["shared-output"]).await,
        Err(ProductionMapError::PaddonNotFound)
    );
    let snapshot = f
        .service
        .paddon_snapshot_for_apparatuses(&target.code, &f.worker, &f.allowed)
        .await
        .unwrap();
    assert!(
        snapshot
            .available_items
            .iter()
            .all(|b| b.batch_id != "shared-output")
    );
    f.service
        .update_paddon_settings(None, Some(true), &f.manager)
        .await
        .unwrap();
    f.scoped_add(&target.code, &["shared-output"])
        .await
        .unwrap();
    assert!(f.members(&source.code).await.is_empty());
    f.transfer(&target.code, &source.code, &["shared-output"])
        .await
        .unwrap();
    let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            f.transfer(&source.code, &target.code, &["shared-output"]),
            f.transfer(&source.code, &alternate.code, &["shared-output"])
        )
    })
    .await
    .expect("cross-apparatus transfers must not deadlock");
    assert_ne!(
        first.is_ok(),
        second.is_ok(),
        "one named-source transfer must win"
    );
    let active: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_paddon_items WHERE progress_batch_id='shared-output' AND removed_at IS NULL")
        .fetch_one(&f.pool).await.unwrap();
    assert_eq!(active, 1);
    assert_eq!(
        f.store
            .progress_batch("shared-output")
            .await
            .unwrap()
            .unwrap(),
        original
    );
    f.close().await;
}

#[tokio::test]
async fn paddon_cross_apparatus_never_bypasses_seals_receipts_or_roll_eligibility() {
    let f = Fixture::new().await;
    let source = f.paddon(CUT1, &f.worker).await;
    let target = f.paddon(CUT5, &f.worker).await;
    f.roll("sealed-output", CUT1).await;
    f.service
        .add_paddon_items(&source.code, &strings(&["sealed-output"]), &f.worker)
        .await
        .unwrap();
    for free in [false, true] {
        f.service
            .update_paddon_management_settings(free, &f.manager)
            .await
            .unwrap();
        for code in [&source.code, &target.code] {
            sqlx::query("UPDATE mini_paddons SET locked_at=now() WHERE code=$1")
                .bind(code)
                .execute(&f.pool)
                .await
                .unwrap();
            assert_eq!(
                f.transfer(&source.code, &target.code, &["sealed-output"])
                    .await,
                Err(ProductionMapError::PaddonLocked)
            );
            assert!(
                f.scoped_add(&target.code, &["sealed-output"])
                    .await
                    .is_err()
            );
            sqlx::query("UPDATE mini_paddons SET locked_at=NULL,receipt_json='{}' WHERE code=$1")
                .bind(code)
                .execute(&f.pool)
                .await
                .unwrap();
            assert_eq!(
                f.transfer(&source.code, &target.code, &["sealed-output"])
                    .await,
                Err(ProductionMapError::PaddonAlreadyReceived)
            );
            assert!(
                f.scoped_add(&target.code, &["sealed-output"])
                    .await
                    .is_err()
            );
            sqlx::query("UPDATE mini_paddons SET receipt_json=NULL WHERE code=$1")
                .bind(code)
                .execute(&f.pool)
                .await
                .unwrap();
        }
        assert_eq!(f.members(&source.code).await, ["sealed-output"]);
        assert!(f.members(&target.code).await.is_empty());
    }
    f.service
        .remove_paddon_items(&source.code, &strings(&["sealed-output"]), &f.worker)
        .await
        .unwrap();
    for free in [false, true] {
        f.service
            .update_paddon_management_settings(free, &f.manager)
            .await
            .unwrap();
        for (status, payload) in [
            ("in_use", serde_json::json!({})),
            ("waiting", serde_json::json!({"received_warehouse":"WH"})),
            (
                "waiting",
                serde_json::json!({"finished_goods_stock_id":"stock-id"}),
            ),
        ] {
            sqlx::query("UPDATE mini_progress_batches SET wip_status=$1,payload_json=$2 WHERE batch_id='sealed-output'")
                .bind(status).bind(payload).execute(&f.pool).await.unwrap();
            assert_eq!(
                f.scoped_add(&target.code, &["sealed-output"]).await,
                Err(ProductionMapError::ProgressBatchNotAccepted)
            );
            assert!(f.members(&target.code).await.is_empty());
        }
    }
    f.close().await;
}
