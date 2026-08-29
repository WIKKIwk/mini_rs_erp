use std::sync::Arc;

use crate::core::production_map::{
    ApparatusQueuePreviousWipMode, OpeningWipBatchInput, OpeningWipBatchStatus,
    OpeningWipCreateInput, OpeningWipQuantityBasis, ProductionMapDefinition, ProductionMapError,
    ProductionMapService, QueueActionActor, QueueProgressInput, queue_state,
};
use crate::db::postgres::{apply_foundation_migration, postgres_test_database_options};
use crate::db::postgres_production_map::PostgresProductionMapStore;

use super::seed_standard_canonical_apparatus;

const PECHAT_ID: &str = "apparatus:default:bosma_7";
const LAMINATION_ID: &str = "apparatus:default:asset-007";

#[tokio::test]
async fn postgres_opening_wip_from_print_persists_and_starts_lamination() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_opening_wip_source";
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");
    admin_pool.close().await;

    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, db_name))
        .await
        .expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migrations");
    seed_standard_canonical_apparatus(&pool).await;

    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new_for_test(store);
    let order_id = "zakaz-postgres-opening-wip-source";
    service
        .upsert_map(two_stage_map(order_id))
        .await
        .expect("save two-stage map");

    let opening = service
        .create_opening_wip(
            OpeningWipCreateInput {
                idempotency_key: "postgres-opening-wip-source-request".to_string(),
                order_id: order_id.to_string(),
                entry_apparatus: String::new(),
                source_operation: String::new(),
                source_apparatus: PECHAT_ID.to_string(),
                source_stage_node_id: "print".to_string(),
                current_location: String::new(),
                note: "PostgreSQL source-contract regression".to_string(),
                batches: vec![OpeningWipBatchInput {
                    quantity_basis: OpeningWipQuantityBasis::Measured,
                    finished_goods_meter: Some(200.0),
                    finished_goods_kg: Some(56.0),
                    bobina_kg: Some(2.0),
                    diameter: None,
                }],
            },
            actor("admin", "admin:opening-wip-postgres"),
        )
        .await
        .expect("create source Opening WIP in PostgreSQL");

    assert_eq!(opening.intake.entry_apparatus, PECHAT_ID);
    assert_eq!(opening.intake.source_apparatus, PECHAT_ID);
    assert_eq!(opening.intake.source_operation, "print");
    assert!(opening.intake.current_location.is_empty());
    assert!(opening.intake.resume_apparatus.is_empty());
    assert_eq!(opening.intake.resume_stage_node_id, "print");
    assert_eq!(opening.batches.len(), 1);

    let persisted: (String, String, Option<String>, String) = sqlx::query_as(
        "SELECT source_apparatus, current_location, resume_apparatus, resume_stage_node_id
         FROM mini_opening_wip_intakes
         WHERE intake_id = $1",
    )
    .bind(&opening.intake.intake_id)
    .fetch_one(&pool)
    .await
    .expect("read persisted Opening WIP contract");
    assert_eq!(persisted.0, PECHAT_ID);
    assert!(persisted.1.is_empty());
    assert_eq!(persisted.2, None);
    assert_eq!(persisted.3, "print");

    let disposable = service
        .create_opening_wip(
            OpeningWipCreateInput {
                idempotency_key: "postgres-opening-wip-delete-request".to_string(),
                order_id: order_id.to_string(),
                entry_apparatus: String::new(),
                source_operation: String::new(),
                source_apparatus: PECHAT_ID.to_string(),
                source_stage_node_id: "print".to_string(),
                current_location: String::new(),
                note: "Unused Opening WIP delete regression".to_string(),
                batches: vec![OpeningWipBatchInput {
                    quantity_basis: OpeningWipQuantityBasis::Measured,
                    finished_goods_meter: Some(50.0),
                    finished_goods_kg: Some(14.0),
                    bobina_kg: Some(1.0),
                    diameter: None,
                }],
            },
            actor("admin", "admin:opening-wip-postgres"),
        )
        .await
        .expect("create disposable Opening WIP");
    let deleted = service
        .delete_opening_wip_batch(
            &disposable.batches[0].batch_id,
            actor("admin", "admin:opening-wip-postgres"),
        )
        .await
        .expect("delete unused Opening WIP");
    assert_eq!(deleted.batch.wip_status, OpeningWipBatchStatus::Void);
    let delete_audit: (String, Option<f64>, String) = sqlx::query_as(
        "SELECT wip_status, EXTRACT(EPOCH FROM voided_at)::DOUBLE PRECISION, voided_by_ref
         FROM mini_opening_wip_batches
         WHERE batch_id = $1",
    )
    .bind(&disposable.batches[0].batch_id)
    .fetch_one(&pool)
    .await
    .expect("read Opening WIP delete audit");
    assert_eq!(delete_audit.0, "void");
    assert!(delete_audit.1.is_some());
    assert_eq!(delete_audit.2, "admin:opening-wip-postgres");

    let controls = service
        .queue_action_controls()
        .await
        .expect("load PostgreSQL queue controls");
    let lamination_control = controls
        .get(LAMINATION_ID)
        .and_then(|orders| orders.get(order_id))
        .expect("lamination queue control");
    assert_eq!(
        lamination_control.interaction.opening_wip_mode,
        ApparatusQueuePreviousWipMode::ScanRequired,
    );
    assert!(
        lamination_control
            .allowed_actions
            .contains(&queue_state::ApparatusQueueAction::Start)
    );

    let assigned = [LAMINATION_ID.to_string()];
    assert_eq!(
        service
            .apply_apparatus_queue_action_with_progress(
                LAMINATION_ID,
                order_id,
                queue_state::ApparatusQueueAction::Start,
                &assigned,
                actor("aparatchi", "worker:lamination-postgres"),
                QueueProgressInput::default(),
            )
            .await,
        Err(ProductionMapError::ProgressQrRequired),
    );

    service
        .apply_apparatus_queue_action_with_progress(
            LAMINATION_ID,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &assigned,
            actor("aparatchi", "worker:lamination-postgres"),
            QueueProgressInput {
                qr_payload: opening.batches[0].qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("scan Opening WIP and start lamination");

    let in_use = service
        .opening_wip_batch(&opening.batches[0].batch_id, "")
        .await
        .expect("reload scanned Opening WIP");
    assert_eq!(in_use.batch.wip_status, OpeningWipBatchStatus::InUse);
    assert_eq!(in_use.batch.used_by_apparatus, LAMINATION_ID);
    assert_eq!(
        service
            .delete_opening_wip_batch(
                &opening.batches[0].batch_id,
                actor("admin", "admin:opening-wip-postgres"),
            )
            .await,
        Err(ProductionMapError::OpeningWipDeleteLocked),
    );

    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("cleanup test db");
    admin_pool.close().await;
}

fn two_stage_map(order_id: &str) -> ProductionMapDefinition {
    serde_json::from_value(serde_json::json!({
        "id": order_id,
        "product_code": "OPENING-WIP-PG",
        "title": "Opening WIP PostgreSQL",
        "code": "PG-OPENING-WIP",
        "order_number": "PG-OPENING-WIP",
        "nodes": [
            { "id": "start", "kind": "start", "title": "Start" },
            {
                "id": "print",
                "kind": "apparatus",
                "title": "7 ta rangli bosma aparat",
                "apparatus_id": PECHAT_ID
            },
            {
                "id": "lamination",
                "kind": "apparatus",
                "title": "Laminatsiya 1",
                "apparatus_id": LAMINATION_ID
            },
            { "id": "end", "kind": "end", "title": "End" }
        ],
        "edges": [
            { "from": "start", "to": "print" },
            { "from": "print", "to": "lamination" },
            { "from": "lamination", "to": "end" }
        ]
    }))
    .expect("two-stage map fixture")
}

fn actor(role: &str, ref_: &str) -> QueueActionActor {
    QueueActionActor {
        role: role.to_string(),
        ref_: ref_.to_string(),
        display_name: ref_.to_string(),
    }
}
