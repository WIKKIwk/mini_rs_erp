use std::str::FromStr;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use mini_rs_erp::app::AppState;
use mini_rs_erp::config::AppConfig;
use mini_rs_erp::core::apparatus_standard::{
    ApparatusCapacity, ApparatusDisplay, ApparatusLifecycle, ApparatusOperationalPolicies,
    CanonicalApparatusDraft, CanonicalCommandMetadata, CapacityAvailability, EquipmentCapability,
    EquipmentCapabilityCode, EquipmentClassId, EquipmentHierarchyScope, ExecutionOperation,
    ExecutionProfile, HierarchyLevelId, LifecycleState, MaterialExecutionPolicy, PhysicalAssetId,
    ProcessTechnology, QueueDiscipline, ToolingExecutionPolicy, TrainingProfile, VirtualTaskPolicy,
};
use mini_rs_erp::core::auth::models::{Principal, PrincipalRole};
use mini_rs_erp::core::backup_doctor::{BackupDoctor, BackupDoctorConfig};
use mini_rs_erp::core::session::manager::SessionManager;
use mini_rs_erp::db::postgres::apply_foundation_migration;
use mini_rs_erp::db::postgres_order_reset::PostgresOrderResetStore;
use mini_rs_erp::http::router::build_router;
use serde_json::{Value, json};
use sqlx::postgres::PgConnectOptions;
use sqlx::{ConnectOptions, PgPool};
use tempfile::TempDir;
use tower::ServiceExt;

const ORDER_ID: &str = "zakaz-order-reset-e2e";
const ITEM_CODE: &str = "E2E-ORDER-ITEM";
const RAW_BARCODE: &str = "E2E-RAW-1";
const QOLIP_CODE: &str = "E2E-QOLIP-1";
const QOLIP_LOCATION_ID: &str = "e2e-qolip-location";
const SESSION_ID: &str = "e2e-order-session";
const PROGRESS_BATCH_ID: &str = "e2e-progress-batch";

#[tokio::test]
async fn order_reset_restores_a_real_database_to_the_pre_order_snapshot() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let database_name = format!("mini_rs_erp_order_reset_e2e_{}", std::process::id());
    let admin_pool = PgPool::connect(&admin_url).await.expect("admin database");

    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{database_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop stale e2e database");
    sqlx::query(&format!(r#"CREATE DATABASE "{database_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create e2e database");
    admin_pool.close().await;

    let test_options = PgConnectOptions::from_str(&admin_url)
        .expect("parse admin database url")
        .database(&database_name);
    let mut test_url = test_options.to_url_lossy();
    test_url.set_query(None);
    let test_url = test_url.to_string();
    let pool = PgPool::connect_with(test_options)
        .await
        .expect("e2e database");
    apply_foundation_migration(&pool)
        .await
        .expect("apply full migration set");

    seed_pre_order_state(&pool).await;
    let mut state = test_state(pool.clone());
    let apparatus_id = seed_canonical_apparatus(
        &state,
        "E2E apparatus",
        "physical-asset:e2e:apparatus",
        "work-unit:e2e:apparatus",
        "command:e2e:apparatus",
    )
    .await;
    let transfer_apparatus_id = seed_canonical_apparatus(
        &state,
        "E2E apparatus 2",
        "physical-asset:e2e:apparatus-2",
        "work-unit:e2e:apparatus-2",
        "command:e2e:apparatus-2",
    )
    .await;
    let before = snapshot(&pool).await;
    seed_order_lifecycle(&pool, &apparatus_id, &transfer_apparatus_id).await;
    let during_order = snapshot(&pool).await;
    assert_ne!(
        during_order, before,
        "lifecycle must change the baseline state"
    );

    let backup_dir = tempfile::tempdir().expect("backup directory");
    let backup_doctor = real_backup_doctor(&test_url, &admin_url, &backup_dir);
    state.sessions = SessionManager::memory(Some(3600));
    state.backup_doctor = backup_doctor;
    state.order_reset = Some(PostgresOrderResetStore::new(pool.clone()));
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Admin,
            display_name: "E2E admin".to_string(),
            legal_name: "E2E admin".to_string(),
            ref_: "e2e-admin".to_string(),
            phone: "+998000000000".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("create admin session");
    let router = build_router(state);

    let response = router
        .oneshot(reset_request(&token))
        .await
        .expect("order reset response");
    let status = response.status();
    let body = response_json(response).await;
    assert_eq!(status, StatusCode::OK, "order reset response: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["scope"], "orders");
    assert_eq!(body["backup"]["verified"], true);
    assert!(!body["backup"]["id"].as_str().unwrap_or_default().is_empty());
    assert!(body["backup"]["size_bytes"].as_u64().unwrap_or_default() > 0);
    assert_eq!(body["result"]["orders_deleted"], 1);
    assert_eq!(body["result"]["production_maps_deleted"], 1);

    let after = snapshot(&pool).await;
    assert_eq!(after, before);

    pool.close().await;
    let admin_pool = PgPool::connect(&admin_url)
        .await
        .expect("admin database cleanup connection");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{database_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop e2e database");
    admin_pool.close().await;
}

fn test_state(pool: PgPool) -> AppState {
    AppState::from_postgres(
        AppConfig {
            bind_addr: "127.0.0.1:0".parse().expect("bind address"),
            default_target_warehouse: "E2E Warehouse".to_string(),
            http_timeout: Duration::from_secs(15),
            session_store_path: "target/order-reset-e2e-sessions.json".into(),
            profile_store_path: "target/order-reset-e2e-profile.json".into(),
            push_token_store_path: "target/order-reset-e2e-push.json".into(),
            session_ttl_seconds: Some(3600),
            supplier_prefix: "10".to_string(),
            werka_prefix: "20".to_string(),
            werka_code: "".to_string(),
            werka_name: "Werka".to_string(),
            werka_phone: "+99888862440".to_string(),
            material_taminotchi_code: String::new(),
            material_taminotchi_name: "Material taminotchi".to_string(),
            material_taminotchi_phone: String::new(),
            admin_phone: "+998880000000".to_string(),
            admin_name: "Admin".to_string(),
            admin_code: "19621978".to_string(),
        },
        pool,
    )
}

fn real_backup_doctor(test_url: &str, admin_url: &str, backup_dir: &TempDir) -> BackupDoctor {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    BackupDoctor::new(BackupDoctorConfig {
        backup_root: backup_dir.path().to_path_buf(),
        script_path: root.join("tools/db/backup_postgres.sh"),
        restore_script_path: root.join("tools/db/restore_postgres.sh"),
        database_url: Some(test_url.to_string()),
        migration_database_url: Some(test_url.to_string()),
        admin_database_url: Some(admin_url.to_string()),
        auto_migrate_after_restore: false,
        auto_enabled: false,
        schedule_hour: 2,
        schedule_minute: 0,
        utc_offset_minutes: 300,
        health_max_age_hours: 30,
        max_runtime: Duration::from_secs(120),
        min_available_mb: 0,
        retention_enabled: false,
    })
}

async fn seed_canonical_apparatus(
    state: &AppState,
    display_name: &str,
    physical_asset_id: &str,
    work_unit_id: &str,
    command_id: &str,
) -> String {
    let committed = state
        .apparatus
        .create(
            CanonicalApparatusDraft {
                display: ApparatusDisplay {
                    display_name: display_name.to_string(),
                    description: "Order reset integration fixture".to_string(),
                    catalog_order: 1,
                },
                equipment_class_id: EquipmentClassId::new("equipment-class:e2e:package")
                    .expect("equipment class id"),
                physical_asset_id: PhysicalAssetId::new(physical_asset_id)
                    .expect("physical asset id"),
                hierarchy: EquipmentHierarchyScope {
                    enterprise_id: HierarchyLevelId::new("enterprise:e2e").expect("enterprise id"),
                    site_id: HierarchyLevelId::new("site:e2e").expect("site id"),
                    area_id: HierarchyLevelId::new("area:e2e").expect("area id"),
                    work_center_id: HierarchyLevelId::new("work-center:e2e")
                        .expect("work center id"),
                    work_unit_id: HierarchyLevelId::new(work_unit_id).expect("work unit id"),
                },
                capabilities: vec![EquipmentCapability {
                    code: EquipmentCapabilityCode::Package,
                    level: 1,
                }],
                execution_profile: ExecutionProfile {
                    operation: ExecutionOperation::Package,
                    technology: ProcessTechnology::BagMaking,
                    color_station_count: None,
                    min_web_width_mm: None,
                    max_web_width_mm: None,
                    virtual_tasks: VirtualTaskPolicy::Disabled,
                    capability_compatible_reroute: true,
                },
                policies: ApparatusOperationalPolicies {
                    queue: QueueDiscipline::StrictSequence,
                    material: MaterialExecutionPolicy::NotRequired {
                        item_group_ids: Vec::new(),
                    },
                    tooling: ToolingExecutionPolicy::NotRequired,
                },
                capacity: ApparatusCapacity {
                    capacity_slots: 1,
                    setup_minutes: 0,
                    cleanup_minutes: 0,
                    efficiency_percent: 100,
                    finite_capacity: true,
                    availability: CapacityAvailability::Always,
                },
                placement: None,
                training: TrainingProfile {
                    enabled: false,
                    queue_enabled: false,
                    material_tracking_enabled: false,
                },
                lifecycle: ApparatusLifecycle {
                    state: LifecycleState::Active,
                    retirement_reason: None,
                },
            },
            CanonicalCommandMetadata::new("user:e2e-admin", command_id),
        )
        .await
        .expect("baseline canonical apparatus");
    committed.revision.apparatus_id.to_string()
}

async fn seed_pre_order_state(pool: &PgPool) {
    let mut tx = pool.begin().await.expect("baseline transaction");
    sqlx::query(
        "INSERT INTO mini_item_groups (name, parent_item_group, is_group, payload_json)
         VALUES ('E2E Materials', 'All Item Groups', true, '{}'::jsonb)
         ON CONFLICT (name) DO NOTHING",
    )
    .execute(&mut *tx)
    .await
    .expect("baseline item group");
    sqlx::query(
        "INSERT INTO mini_items (code, name, uom, item_group, payload_json)
         VALUES ($1, 'E2E raw material', 'Kg', 'E2E Materials', '{}'::jsonb)
         ON CONFLICT (code) DO NOTHING",
    )
    .bind(ITEM_CODE)
    .execute(&mut *tx)
    .await
    .expect("baseline item");
    sqlx::query(
        "INSERT INTO mini_warehouses (id, name, company, is_group, parent_warehouse)
         VALUES ('e2e-warehouse', 'E2E Warehouse', '', false, '')
         ON CONFLICT (id) DO NOTHING",
    )
    .execute(&mut *tx)
    .await
    .expect("baseline warehouse");
    sqlx::query(
        "INSERT INTO mini_gscale_receipts
            (name, status, item_code, warehouse, qty, uom, barcode)
         VALUES ('E2E-RECEIPT', 'submitted', $1, 'E2E Warehouse', 10, 'kg', $2)
         ON CONFLICT (name) DO NOTHING",
    )
    .bind(ITEM_CODE)
    .bind(RAW_BARCODE)
    .execute(&mut *tx)
    .await
    .expect("baseline receipt");
    sqlx::query(
        "INSERT INTO mini_raw_material_stock
            (id, warehouse, item_code, item_name, barcode, qty, uom, status,
             reserved_order_id, source_receipt_id, payload_json)
         VALUES ('raw:e2e-raw-1', 'E2E Warehouse', $1, 'E2E raw material', $2,
                 10, 'kg', 'available', '', 'E2E-RECEIPT', '{}'::jsonb)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(ITEM_CODE)
    .bind(RAW_BARCODE)
    .execute(&mut *tx)
    .await
    .expect("baseline raw stock");
    sqlx::query(
        "INSERT INTO mini_qolip_product_specs
            (item_code, item_name, item_group, qolip_code, size,
             created_by_role, created_by_ref, created_by_name)
         VALUES ($1, 'E2E product', 'E2E Materials', $2, 10, 'system', 'e2e', 'E2E')
         ON CONFLICT DO NOTHING",
    )
    .bind(ITEM_CODE)
    .bind(QOLIP_CODE)
    .execute(&mut *tx)
    .await
    .expect("baseline qolip spec");
    sqlx::query(
        "INSERT INTO mini_qolip_locations
            (id, block, warehouse, item_code, item_name, qolip_code, size, quantity,
             row_letter, column_number, location_label,
             created_by_role, created_by_ref, created_by_name, payload_json)
         VALUES ($1, 'E2E Block', 'E2E Qolip', $2, 'E2E product', $3, 10, 2,
                 'A', 1, 'A1', 'system', 'e2e', 'E2E', '{}'::jsonb)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(QOLIP_LOCATION_ID)
    .bind(ITEM_CODE)
    .bind(QOLIP_CODE)
    .execute(&mut *tx)
    .await
    .expect("baseline qolip location");
    sqlx::query("ALTER SEQUENCE mini_production_order_number_seq RESTART WITH 1")
        .execute(&mut *tx)
        .await
        .expect("baseline order sequence");
    tx.commit().await.expect("commit baseline state");
}

async fn seed_order_lifecycle(pool: &PgPool, apparatus_id: &str, transfer_apparatus_id: &str) {
    let mut tx = pool.begin().await.expect("lifecycle transaction");
    sqlx::query(
        "INSERT INTO mini_orders
            (id, code, order_number, customer_ref, customer_name, product_code,
             product_name, status, product_form, kg, width_mm, roll_count)
         VALUES ($1, 'E2E-ORDER', '0001', '', 'E2E customer', $2,
                 'E2E product', 'in_progress', 'rulon', 10, 1000, 1)",
    )
    .bind(ORDER_ID)
    .bind(ITEM_CODE)
    .execute(&mut *tx)
    .await
    .expect("order");
    sqlx::query(
        "INSERT INTO mini_production_maps
            (id, order_id, product_code, title, code, order_number, roll_count, width_mm, map_json)
         VALUES ($1, $1, $2, 'E2E product', 'E2E-ORDER', '0001', 1, 1000,
                 $3)",
    )
    .bind(ORDER_ID)
    .bind(ITEM_CODE)
    .bind(json!({
        "id": ORDER_ID,
        "product_code": ITEM_CODE,
        "title": "E2E product",
        "order_number": "0001",
        "nodes": [],
        "edges": []
    }))
    .execute(&mut *tx)
    .await
    .expect("production map");
    sqlx::query(
        "INSERT INTO mini_production_map_nodes
            (map_id, node_id, kind, title, payload_json)
         VALUES ($1, 'start', 'start', 'Start', '{}'::jsonb),
                ($1, 'end', 'end', 'End', '{}'::jsonb)",
    )
    .bind(ORDER_ID)
    .execute(&mut *tx)
    .await
    .expect("map nodes");
    sqlx::query(
        "INSERT INTO mini_order_products
            (id, order_id, item_code, product_name, layers_json)
         VALUES ('e2e-order-product', $1, $2, 'E2E product', '[]'::jsonb)",
    )
    .bind(ORDER_ID)
    .bind(ITEM_CODE)
    .execute(&mut *tx)
    .await
    .expect("order product");
    sqlx::query(
        "INSERT INTO mini_queue_sequences (apparatus, canonical_apparatus_id, order_ids)
         VALUES ('E2E apparatus', $2, jsonb_build_array($1))",
    )
    .bind(ORDER_ID)
    .bind(apparatus_id)
    .execute(&mut *tx)
    .await
    .expect("queue sequence");
    sqlx::query(
        "INSERT INTO mini_queue_states (apparatus, canonical_apparatus_id, order_id, state)
         VALUES ('E2E apparatus', $2, $1, 'in_progress')",
    )
    .bind(ORDER_ID)
    .bind(apparatus_id)
    .execute(&mut *tx)
    .await
    .expect("queue state");
    sqlx::query(
        "INSERT INTO mini_queue_action_events
            (event_id, apparatus, canonical_apparatus_id, order_id, action, from_state, to_state, policy,
             assigned_apparatus, actor_role, actor_ref, actor_display_name)
         VALUES ('e2e-queue-event', 'E2E apparatus', $2, $1, 'start', 'pending',
                 'in_progress', 'free_pick', '[]'::jsonb, 'admin', 'e2e-admin', 'E2E')",
    )
    .bind(ORDER_ID)
    .bind(apparatus_id)
    .execute(&mut *tx)
    .await
    .expect("queue event");
    sqlx::query(
        "INSERT INTO mini_order_run_sessions
            (session_id, apparatus, canonical_apparatus_id, order_id, status, worker_role, worker_ref,
             worker_display_name, payload_json)
         VALUES ($1, 'E2E apparatus', $3, $2, 'active', 'aparatchi', 'e2e-worker',
                 'E2E worker', $4)",
    )
    .bind(SESSION_ID)
    .bind(ORDER_ID)
    .bind(apparatus_id)
    .bind(json!({"qolip_code": QOLIP_CODE, "qolip_codes": [QOLIP_CODE]}))
    .execute(&mut *tx)
    .await
    .expect("order session");
    sqlx::query(
        "INSERT INTO mini_order_progress_events
            (event_id, session_id, batch_id, apparatus, canonical_apparatus_id, order_id, action,
             produced_qty, uom, worker_role, worker_ref, worker_display_name)
         VALUES ('e2e-progress-event', $1, $2, 'E2E apparatus', $4, $3, 'complete',
                 5, 'kg', 'aparatchi', 'e2e-worker', 'E2E worker')",
    )
    .bind(SESSION_ID)
    .bind(PROGRESS_BATCH_ID)
    .bind(ORDER_ID)
    .bind(apparatus_id)
    .execute(&mut *tx)
    .await
    .expect("progress event");
    sqlx::query(
        "INSERT INTO mini_progress_batches
            (batch_id, session_id, apparatus, canonical_apparatus_id, order_id, action, status, produced_qty,
             uom, qr_payload, label_item_code, label_item_name, wip_status,
             current_apparatus, canonical_current_apparatus_id, current_location)
         VALUES ($1, $2, 'E2E apparatus', $5, $3, 'complete', 'completed', 5, 'kg',
                 'E2E-QR-1', $4, 'E2E product', 'processed', 'E2E apparatus', $5, 'E2E')",
    )
    .bind(PROGRESS_BATCH_ID)
    .bind(SESSION_ID)
    .bind(ORDER_ID)
    .bind(ITEM_CODE)
    .bind(apparatus_id)
    .execute(&mut *tx)
    .await
    .expect("progress batch");
    sqlx::query(
        "INSERT INTO mini_progress_batch_corrections
            (batch_id, previous_revision, new_revision, reason, actor_role, actor_ref,
             actor_display_name, old_values, new_values)
         VALUES ($1, 1, 2, 'E2E correction', 'admin', 'e2e-admin', 'E2E', '{}', '{}')",
    )
    .bind(PROGRESS_BATCH_ID)
    .execute(&mut *tx)
    .await
    .expect("progress correction");
    sqlx::query(
        "INSERT INTO mini_paddons (id, code, note, created_by_ref, created_by_display_name)
         VALUES ('e2e-paddon', '12345', 'E2E', 'e2e-admin', 'E2E')",
    )
    .execute(&mut *tx)
    .await
    .expect("paddon");
    sqlx::query(
        "INSERT INTO mini_paddon_items
            (id, paddon_id, progress_batch_id, added_by_ref, added_by_display_name)
         VALUES ('e2e-paddon-item', 'e2e-paddon', $1, 'e2e-admin', 'E2E')",
    )
    .bind(PROGRESS_BATCH_ID)
    .execute(&mut *tx)
    .await
    .expect("paddon item");
    sqlx::query(
        "INSERT INTO mini_raw_material_assignments
            (barcode, order_id, apparatus, canonical_apparatus_id, item_code, item_group, payload_json)
         VALUES ($1, $2, 'E2E apparatus', $4, $3, 'E2E Materials', '{}')",
    )
    .bind(RAW_BARCODE)
    .bind(ORDER_ID)
    .bind(ITEM_CODE)
    .bind(apparatus_id)
    .execute(&mut *tx)
    .await
    .expect("raw assignment");
    sqlx::query(
        "UPDATE mini_raw_material_stock
         SET status = 'consumed', reserved_order_id = $2,
             payload_json = jsonb_build_object(
                 'in_use_order_id', $2,
                 'consumed_order_id', $2,
                 'reserved_order_id', $2
             )
         WHERE barcode = $1",
    )
    .bind(RAW_BARCODE)
    .bind(ORDER_ID)
    .execute(&mut *tx)
    .await
    .expect("consume raw stock");
    sqlx::query(
        "INSERT INTO mini_raw_material_events
            (event_id, idempotency_key, event_type, warehouse, barcode, item_code,
             item_name, qty_delta, uom, stock_status_before, stock_status_after,
             order_id, apparatus, canonical_apparatus_id, actor_role, actor_ref, actor_display_name,
             source_type, source_id, payload_json)
         VALUES ('e2e-raw-event', 'e2e-raw-event-key', 'consumption_posted',
                 'E2E Warehouse', $1, $2, 'E2E raw material', -10, 'kg',
                 'in_use', 'consumed', $3, 'E2E apparatus', $4, 'system', 'system',
                 'System', 'consumption', $3, '{}')",
    )
    .bind(RAW_BARCODE)
    .bind(ITEM_CODE)
    .bind(ORDER_ID)
    .bind(apparatus_id)
    .execute(&mut *tx)
    .await
    .expect("raw event");
    sqlx::query("UPDATE mini_qolip_locations SET quantity = 1 WHERE id = $1")
        .bind(QOLIP_LOCATION_ID)
        .execute(&mut *tx)
        .await
        .expect("consume qolip location");
    sqlx::query(
        "INSERT INTO mini_qolip_checkouts
            (id, location_id, block, warehouse, item_code, item_name, qolip_code, size,
             quantity, row_letter, column_number, location_label, issued_to_ref,
             issued_to_name, status, issued_by_role, issued_by_ref, issued_by_name,
             payload_json)
         VALUES ('e2e-qolip-checkout', $1, 'E2E Block', 'E2E Qolip', $2,
                 'E2E product', $3, 10, 1, 'A', 1, 'A1', 'e2e-worker',
                 'E2E worker', 'open', 'admin', 'e2e-admin', 'E2E',
                 $4)",
    )
    .bind(QOLIP_LOCATION_ID)
    .bind(ITEM_CODE)
    .bind(QOLIP_CODE)
    .bind(json!({"order_id": ORDER_ID, "qolip_code": QOLIP_CODE}))
    .execute(&mut *tx)
    .await
    .expect("qolip checkout");
    sqlx::query(
        "INSERT INTO mini_qolip_order_notes
            (order_id, principal_role, principal_ref, principal_name, item_code,
             item_name, qolip_codes, status)
         VALUES ($1, 'aparatchi', 'e2e-worker', 'E2E worker', $2,
                 'E2E product', ARRAY[$3]::text[], 'given')",
    )
    .bind(ORDER_ID)
    .bind(ITEM_CODE)
    .bind(QOLIP_CODE)
    .execute(&mut *tx)
    .await
    .expect("qolip order note");
    sqlx::query(
        "INSERT INTO mini_returned_paint_images
            (image_id, order_id, apparatus, canonical_apparatus_id, owner_ref, image_name, image_mime,
             image_size_bytes, body)
         VALUES ('e2e-paint-image', $1, 'E2E apparatus', $3, 'e2e-worker',
                 'e2e.jpg', 'image/jpeg', 1, $2)",
    )
    .bind(ORDER_ID)
    .bind(vec![1_u8])
    .bind(apparatus_id)
    .execute(&mut *tx)
    .await
    .expect("returned paint image");
    sqlx::query(
        r#"INSERT INTO mini_returned_paint_requests
            (id, order_id, order_code, order_name, apparatus, canonical_apparatus_id, sender_role, sender_ref,
             sender_display_name, items_json, status, image_id,
             rasxot_mix_total, astatka_mix_total, rasxot_alcohol, astatka_alcohol,
             final_used_alcohol, rasxot_pure_paint, astatka_pure_paint, final_used_paint)
         VALUES ('e2e-paint-request', $1, 'E2E-ORDER', 'E2E product', 'E2E apparatus', $2,
                 'aparatchi', 'e2e-worker', 'E2E worker',
                 '[{"category":"colors","usage":"rasxot","values":{"mix":1}}]',
                 'completed', 'e2e-paint-image', 1, 0, 0.3, 0, 0.3, 0.7, 0, 0.7)"#,
    )
    .bind(ORDER_ID)
    .bind(apparatus_id)
    .execute(&mut *tx)
    .await
    .expect("returned paint request");
    sqlx::query(
        "INSERT INTO mini_laminatsiya_astatka_reports
            (report_id, order_id, apparatus, canonical_apparatus_id, from_at, to_at,
             lamination_print_leftover_rolls, lamination_film_leftover_rolls, total_waste)
         VALUES ('e2e-laminatsiya-report', $1, 'E2E apparatus', $2, now(), now(), 1, 1, 1)",
    )
    .bind(ORDER_ID)
    .bind(apparatus_id)
    .execute(&mut *tx)
    .await
    .expect("laminatsiya report");
    sqlx::query(
        "INSERT INTO mini_rezka_astatka_reports
            (report_id, order_id, apparatus, canonical_apparatus_id, from_at, to_at, total_waste,
             rezka_bosma_waste, rezka_lamination_waste, rezka_edge_waste)
         VALUES ('e2e-rezka-report', $1, 'E2E apparatus', $2, now(), now(), 1, 1, 0, 0)",
    )
    .bind(ORDER_ID)
    .bind(apparatus_id)
    .execute(&mut *tx)
    .await
    .expect("rezka report");
    sqlx::query(
        "INSERT INTO mini_apparatus_order_transfers
            (transfer_id, idempotency_key, order_id, from_apparatus, to_apparatus,
             canonical_from_apparatus_id, canonical_to_apparatus_id,
             reason, actor_role, session_id, progress_batch_id, payload_json)
         VALUES ('e2e-transfer', 'e2e-transfer-key', $1, 'E2E apparatus',
                 'E2E apparatus 2', $4, $5, 'E2E', 'admin', $2, $3, '{}')",
    )
    .bind(ORDER_ID)
    .bind(SESSION_ID)
    .bind(PROGRESS_BATCH_ID)
    .bind(apparatus_id)
    .bind(transfer_apparatus_id)
    .execute(&mut *tx)
    .await
    .expect("apparatus transfer");
    sqlx::query(
        "INSERT INTO mini_apparatus_schedule_reservations
            (reservation_id, idempotency_key, order_id, canonical_apparatus_id,
             apparatus_id, apparatus,
             starts_at, ends_at, requested_duration_minutes, reserved_duration_minutes,
             status, capability_requirements, actor_json)
         VALUES ('e2e-reservation', 'e2e-reservation-key', $1, $2, $2,
                 'E2E apparatus', now(), now() + interval '1 hour', 60, 60,
                 'planned', '[]', '{}')",
    )
    .bind(ORDER_ID)
    .bind(apparatus_id)
    .execute(&mut *tx)
    .await
    .expect("schedule reservation");
    sqlx::query(
        "INSERT INTO mini_finished_goods_stock
            (id, warehouse, order_id, item_code, item_name, qty, uom, status)
         VALUES ('e2e-finished-good', 'E2E Warehouse', $1, $2, 'E2E product', 5, 'kg', 'available')",
    )
    .bind(ORDER_ID)
    .bind(ITEM_CODE)
    .execute(&mut *tx)
    .await
    .expect("finished goods");
    sqlx::query(
        "INSERT INTO mini_order_control_states
            (order_id, state, actor_role, actor_ref, actor_display_name)
         VALUES ($1, 'active', 'admin', 'e2e-admin', 'E2E')",
    )
    .bind(ORDER_ID)
    .execute(&mut *tx)
    .await
    .expect("order control state");
    sqlx::query(
        "INSERT INTO mini_engine_events
            (event_id, domain, action, entity_id, actor_key, idempotency_key, payload_json)
         VALUES ('e2e-engine-event', 'orders', 'create', $1, 'e2e-admin',
                 'e2e-engine-key', '{}')",
    )
    .bind(ORDER_ID)
    .execute(&mut *tx)
    .await
    .expect("engine event");
    sqlx::query(
        "INSERT INTO mini_idempotency_keys (key, domain, action, entity_id)
         VALUES ('e2e-idempotency-key', 'orders', 'create', $1)",
    )
    .bind(ORDER_ID)
    .execute(&mut *tx)
    .await
    .expect("idempotency key");
    tx.commit().await.expect("commit order lifecycle");
}

async fn snapshot(pool: &PgPool) -> Value {
    let raw = sqlx::query_as::<_, (String, String, f64, String)>(
        "SELECT status, reserved_order_id, qty::float8, payload_json::text
         FROM mini_raw_material_stock WHERE barcode = $1",
    )
    .bind(RAW_BARCODE)
    .fetch_one(pool)
    .await
    .expect("raw stock snapshot");
    let qolip = sqlx::query_as::<_, (i32, String, Option<i32>)>(
        "SELECT quantity, row_letter, column_number
         FROM mini_qolip_locations WHERE id = $1",
    )
    .bind(QOLIP_LOCATION_ID)
    .fetch_one(pool)
    .await
    .expect("qolip snapshot");
    let receipt = sqlx::query_as::<_, (String, f64)>(
        "SELECT status, qty::float8 FROM mini_gscale_receipts WHERE name = 'E2E-RECEIPT'",
    )
    .fetch_one(pool)
    .await
    .expect("receipt snapshot");
    let sequence = sqlx::query_as::<_, (i64, bool)>(
        "SELECT last_value, is_called FROM mini_production_order_number_seq",
    )
    .fetch_one(pool)
    .await
    .expect("order sequence snapshot");

    let mut counts = serde_json::Map::new();
    for (key, table) in [
        ("orders", "mini_orders"),
        ("maps", "mini_production_maps"),
        ("order_products", "mini_order_products"),
        ("queue_states", "mini_queue_states"),
        ("queue_events", "mini_queue_action_events"),
        ("sessions", "mini_order_run_sessions"),
        ("progress_events", "mini_order_progress_events"),
        ("progress_batches", "mini_progress_batches"),
        ("progress_corrections", "mini_progress_batch_corrections"),
        ("raw_assignments", "mini_raw_material_assignments"),
        ("raw_events", "mini_raw_material_events"),
        ("qolip_notes", "mini_qolip_order_notes"),
        ("qolip_checkouts", "mini_qolip_checkouts"),
        ("returned_requests", "mini_returned_paint_requests"),
        ("returned_images", "mini_returned_paint_images"),
        ("laminatsiya_reports", "mini_laminatsiya_astatka_reports"),
        ("rezka_reports", "mini_rezka_astatka_reports"),
        ("transfers", "mini_apparatus_order_transfers"),
        ("reservations", "mini_apparatus_schedule_reservations"),
        ("finished_goods", "mini_finished_goods_stock"),
        ("paddons", "mini_paddons"),
        ("paddon_items", "mini_paddon_items"),
        ("engine_events", "mini_engine_events"),
        ("idempotency_keys", "mini_idempotency_keys"),
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(pool)
            .await
            .expect("table snapshot");
        counts.insert(key.to_string(), json!(count));
    }
    let queue_sequence: Value = sqlx::query_scalar(
        "SELECT order_ids FROM mini_queue_sequences WHERE apparatus = 'E2E apparatus'",
    )
    .fetch_optional(pool)
    .await
    .expect("queue sequence snapshot")
    .unwrap_or_else(|| json!([]));
    counts.insert(
        "qolip_locations".to_string(),
        json!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM mini_qolip_locations")
                .fetch_one(pool)
                .await
                .expect("qolip locations count")
        ),
    );

    json!({
        "counts": counts,
        "raw": {
            "status": raw.0,
            "reserved_order_id": raw.1,
            "qty": raw.2,
            "payload": raw.3,
        },
        "qolip": {
            "quantity": qolip.0,
            "row": qolip.1,
            "column": qolip.2,
        },
        "receipt": {"status": receipt.0, "qty": receipt.1},
        "sequence": {"last_value": sequence.0, "is_called": sequence.1},
        "queue_sequence": queue_sequence,
    })
}

fn reset_request(token: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/mobile/admin/emergency-reset/orders")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"confirmation":"RESET ORDERS"}"#))
        .expect("reset request")
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("response json")
}
