use super::*;
use crate::db::postgres::apply_foundation_migration;
use sqlx::postgres::PgConnectOptions;

#[tokio::test]
async fn preparation_history_pages_scoped_movements_and_groups_transfer_receipts() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".into());
    let admin = PgPool::connect(&url).await.unwrap();
    let db = format!("mini_rs_erp_test_history_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(url.parse::<PgConnectOptions>().unwrap().database(&db))
        .await
        .unwrap();
    apply_foundation_migration(&pool).await.unwrap();

    sqlx::raw_sql(
        r#"
INSERT INTO mini_system_users(id, role, name, phone) VALUES
  ('master', 'tayyorlov_masteri', 'Master', '901234567'),
  ('other', 'tayyorlov_masteri', 'Other', '901234568');
INSERT INTO mini_warehouses(id, name) VALUES
  ('source', 'Source'), ('prep', 'Preparation'), ('outside', 'Outside'), ('hidden', 'Hidden');
INSERT INTO mini_warehouse_assignments(assignment_kind, warehouse, warehouse_name, principal_role, principal_ref) VALUES
  ('warehouse', 'Preparation', 'Preparation', 'tayyorlov_masteri', 'master'),
  ('warehouse', 'Source', 'Source', 'tayyorlov_masteri', 'master'),
  ('warehouse', 'Hidden', 'Hidden', 'tayyorlov_masteri', 'other');

INSERT INTO mini_item_groups(name, parent_item_group, is_group)
VALUES ('Seriyo', 'All Item Groups', true) ON CONFLICT DO NOTHING;
INSERT INTO mini_items(code, name, uom, item_group) VALUES ('HIST-PE', 'Polyethylene', 'Kg', 'Seriyo');
INSERT INTO mini_preparation_materials(item_code, owner_ref, name_key) VALUES ('HIST-PE', 'master', 'polyethylene');
INSERT INTO mini_raw_material_stock(id, warehouse, item_code, item_name, barcode, qty, uom, status)
VALUES ('pending-stock', 'Preparation', 'HIST-PE', 'Polyethylene', 'PE-PENDING', 25, 'kg', 'available');

INSERT INTO mini_inventory_transfers(id, idempotency_key, source_warehouse_id, source_warehouse,
    destination_warehouse_id, destination_warehouse, status, requested_by_role, requested_by_ref,
    created_at, received_at, rejected_at)
VALUES
  ('confirmed', 'confirmed', 'source', 'Source', 'prep', 'Preparation', 'received', 'material_taminotchi', 'supplier',
   '2026-01-03 00:00Z', '2026-01-03 01:00Z', NULL),
  ('legacy', 'legacy', 'outside', 'Outside', 'prep', 'Preparation', 'received', 'material_taminotchi', 'supplier',
   '2026-01-02 00:00Z', '2026-01-02 01:00Z', NULL),
  ('rejected', 'rejected', 'prep', 'Preparation', 'outside', 'Outside', 'rejected', 'material_taminotchi', 'supplier',
   '2026-01-01 00:00Z', NULL, '2026-01-01 01:00Z'),
  ('pending', 'pending', 'outside', 'Outside', 'prep', 'Preparation', 'received', 'material_taminotchi', 'supplier',
   '2026-01-05 00:00Z', '2026-01-05 01:00Z', NULL),
  ('open', 'open', 'outside', 'Outside', 'prep', 'Preparation', 'requested', 'material_taminotchi', 'supplier',
   '2026-01-06 00:00Z', NULL, NULL),
  ('hidden', 'hidden', 'outside', 'Outside', 'hidden', 'Hidden', 'received', 'material_taminotchi', 'supplier',
   '2026-01-06 00:00Z', '2026-01-06 01:00Z', NULL);
INSERT INTO mini_inventory_transfer_lines(transfer_id, asset_kind, asset_ref, item_code, item_name, qty, uom, source_physical_location_id)
SELECT id, 'raw_material', id || '-stock', 'HIST-PE', 'Polyethylene', 25, 'kg', 'source-place' FROM mini_inventory_transfers;

INSERT INTO mini_preparation_operations(id, owner_ref, request_id, kind, order_id, request_json, response_json, created_at)
VALUES
  ('link-1', 'master', 'link-001', 'receipt', NULL, '{"kind":"warehouse_transfer","transfer_id":"confirmed"}',
   '{"kind":"receipt","warehouse":"Preparation","name":"Linked 1","kg":"25"}', '2026-01-04 00:00Z'),
  ('link-2', 'master', 'link-002', 'receipt', NULL, '{"kind":"warehouse_transfer","transfer_id":"confirmed"}',
   '{"kind":"receipt","warehouse":"Preparation","name":"Linked 2","kg":"25"}', '2026-01-04 00:00Z'),
  ('old-link', 'master', 'old-link', 'receipt', NULL, '{"kind":"paddon_transfer","stock_id":"legacy-stock"}',
   '{"kind":"receipt","warehouse":"Preparation","name":"Legacy","kg":"25"}', '2026-01-02 01:00Z'),
  ('manual', 'master', 'manual-001', 'receipt', NULL, '{"kind":"receipt"}',
   '{"kind":"receipt","warehouse":"Preparation","name":"Direct","kg":"12"}', '2026-01-03 00:00Z'),
  ('reversal', 'master', 'reversal-001', 'receipt_reversal', NULL, '{"kind":"receipt_reversal"}',
   '{"kind":"receipt_reversal","warehouse":"Preparation","name":"Direct","kg":"12"}', '2026-01-03 10:00Z'),
  ('used', 'master', 'consumption-01', 'consumption', 'history-order', '{"kind":"consumption"}',
   '{"kind":"consumption","warehouse":"Preparation","order_code":"001"}', '2026-01-04 00:00Z'),
  ('other', 'other', 'other-001', 'receipt', NULL, '{"kind":"receipt"}',
   '{"kind":"receipt","warehouse":"Preparation","name":"Other owner","kg":"12"}', '2026-01-06 00:00Z');
INSERT INTO mini_preparation_operations(id, owner_ref, request_id, kind, request_json, response_json, created_at)
SELECT 'older-' || n, 'master', 'older-req-' || n, 'receipt', '{"kind":"receipt"}',
       '{"kind":"receipt","warehouse":"Preparation","name":"Older","kg":"1"}', '2025-12-31 00:00Z'
FROM generate_series(1, 105) n;
UPDATE mini_warehouses SET name = 'Renamed Preparation' WHERE id = 'prep';
UPDATE mini_warehouse_assignments SET warehouse_name = 'Renamed Preparation', warehouse = 'Renamed Preparation'
WHERE warehouse_name = 'Preparation';
UPDATE mini_raw_material_stock SET warehouse = 'Renamed Preparation' WHERE id = 'pending-stock';
"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let store = PostgresPreparationStore::new(pool.clone());
    let mut query = PreparationHistoryQuery {
        limit: Some(2),
        ..Default::default()
    };
    let first = store.history("master", &query).await.unwrap();
    let entries = first["entries"].as_array().unwrap();
    assert_eq!(entries[0]["key"], "transfer:confirmed");
    assert_eq!(entries[0]["transfer"]["lines"][0]["qty"], 25.0);
    assert_eq!(
        entries[0]["preparation_received_at"],
        "2026-01-04T00:00:00+00:00"
    );
    assert_eq!(
        entries[0]["preparation_receipts"].as_array().unwrap().len(),
        2
    );
    assert_eq!(entries[0]["preparation_receipts"][0]["kg"], "25");
    assert_eq!(entries[1]["key"], "operation:used");

    query.before_time = first["next_cursor"]["time"].as_str().map(str::to_owned);
    query.before_key = first["next_cursor"]["key"].as_str().map(str::to_owned);
    query.limit = Some(100);
    let second = store.history("master", &query).await.unwrap();
    let entries = second["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 100);
    assert_eq!(entries[0]["key"], "operation:reversal");
    assert_eq!(entries[1]["operation"]["warehouse"], "Renamed Preparation");
    assert_eq!(entries[2]["key"], "transfer:legacy");
    assert_eq!(entries[3]["key"], "transfer:rejected");

    query.before_time = second["next_cursor"]["time"].as_str().map(str::to_owned);
    query.before_key = second["next_cursor"]["key"].as_str().map(str::to_owned);
    let third = store.history("master", &query).await.unwrap();
    assert_eq!(third["entries"].as_array().unwrap().len(), 9);
    assert!(third["next_cursor"].is_null());

    let receipt_filter = PreparationHistoryQuery {
        warehouse_id: "prep".into(),
        kind: HistoryFilter::Receipt,
        limit: Some(4),
        ..Default::default()
    };
    let receipts = store.history("master", &receipt_filter).await.unwrap();
    let keys: Vec<_> = receipts["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "transfer:confirmed",
            "operation:reversal",
            "operation:manual",
            "transfer:legacy"
        ]
    );

    let source_transfer_filter = PreparationHistoryQuery {
        warehouse_id: "source".into(),
        kind: HistoryFilter::Transfer,
        ..Default::default()
    };
    let source_transfers = store
        .history("master", &source_transfer_filter)
        .await
        .unwrap();
    assert_eq!(source_transfers["entries"].as_array().unwrap().len(), 1);
    assert_eq!(source_transfers["entries"][0]["key"], "transfer:confirmed");

    let hidden_filter = PreparationHistoryQuery {
        warehouse_id: "hidden".into(),
        ..Default::default()
    };
    let hidden = store.history("master", &hidden_filter).await.unwrap();
    assert_eq!(hidden["entries"], json!([]));

    let sarf_filter = PreparationHistoryQuery {
        kind: HistoryFilter::Consumption,
        ..Default::default()
    };
    let sarf = store.history("master", &sarf_filter).await.unwrap();
    assert_eq!(sarf["entries"].as_array().unwrap().len(), 1);
    assert_eq!(sarf["entries"][0]["key"], "operation:used");

    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
}
