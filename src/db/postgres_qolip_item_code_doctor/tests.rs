use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use super::PostgresQolipItemCodeDoctor;
use crate::db::postgres::apply_foundation_migration;

#[tokio::test]
async fn doctor_repairs_only_unambiguous_qolip_item_code_mismatches() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".to_string());
    let database_name = format!("mini_rs_erp_qolip_item_code_doctor_{}", std::process::id());
    let admin_options = PgConnectOptions::from_str(&admin_url).expect("admin database url");
    let admin_pool = PgPoolOptions::new()
        .connect_with(admin_options.clone())
        .await
        .expect("admin database");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{database_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop stale doctor test database");
    sqlx::query(&format!(r#"CREATE DATABASE "{database_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create doctor test database");

    let pool = PgPoolOptions::new()
        .connect_with(admin_options.clone().database(&database_name))
        .await
        .expect("doctor test database");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migrations");
    seed_test_data(&pool).await;

    let doctor = PostgresQolipItemCodeDoctor::new(pool.clone(), true, Duration::from_secs(300));
    let report = doctor.run_once().await.expect("doctor repair");
    assert_eq!(report.repaired_products, 1);
    assert_eq!(report.product_specs_updated, 2);
    assert_eq!(report.locations_updated, 1);
    assert_eq!(report.open_checkouts_updated, 1);
    assert_eq!(report.order_notes_updated, 1);

    for table in [
        "mini_qolip_product_specs",
        "mini_qolip_locations",
        "mini_qolip_checkouts",
        "mini_qolip_order_notes",
    ] {
        let code: String = sqlx::query_scalar(&format!(
            "SELECT item_code FROM {table} WHERE lower(item_name) = lower('Magnus') LIMIT 1"
        ))
        .fetch_one(&pool)
        .await
        .expect("repaired qolip projection");
        assert_eq!(code, "AMB", "{table} must use the ERP item code");
    }

    let canonical_first_qolip: String = sqlx::query_scalar(
        "SELECT COALESCE(payload_json->>'qolip_first_code', '')
         FROM mini_items WHERE code = 'AMB'",
    )
    .fetch_one(&pool)
    .await
    .expect("canonical first qolip code");
    assert_eq!(canonical_first_qolip, "5913-1");
    let alias_first_qolip: String = sqlx::query_scalar(
        "SELECT COALESCE(payload_json->>'qolip_first_code', '')
         FROM mini_items WHERE code = 'TG-a6b78c3080f65879'",
    )
    .fetch_one(&pool)
    .await
    .expect("alias first qolip code");
    assert!(alias_first_qolip.is_empty());

    let audit: (String, String, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT source_item_code, canonical_item_code, product_specs_updated,
                locations_updated, open_checkouts_updated, order_notes_updated
         FROM mini_qolip_item_code_repairs",
    )
    .fetch_one(&pool)
    .await
    .expect("doctor audit row");
    assert_eq!(
        audit,
        (
            "TG-a6b78c3080f65879".to_string(),
            "AMB".to_string(),
            2,
            1,
            1,
            1
        )
    );

    let ambiguous_codes: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT item_code
         FROM mini_qolip_product_specs
         WHERE item_name = 'Ambiguous product'
         ORDER BY item_code",
    )
    .fetch_all(&pool)
    .await
    .expect("ambiguous qolip codes");
    assert_eq!(ambiguous_codes, vec!["TG-AMBIGUOUS-A", "TG-AMBIGUOUS-B"]);

    let second_report = doctor.run_once().await.expect("idempotent doctor repair");
    assert_eq!(second_report.repaired_products, 0);
    let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_qolip_item_code_repairs")
        .fetch_one(&pool)
        .await
        .expect("audit count");
    assert_eq!(audit_count, 1);

    pool.close().await;
    sqlx::query(&format!(r#"DROP DATABASE "{database_name}" WITH (FORCE)"#))
        .execute(&admin_pool)
        .await
        .expect("drop doctor test database");
    admin_pool.close().await;
}

async fn seed_test_data(pool: &sqlx::PgPool) {
    sqlx::raw_sql(
        r#"
        INSERT INTO mini_item_groups (name, parent_item_group, is_group, payload_json)
        VALUES
            ('All Item Groups', NULL, true, '{}'::jsonb),
            ('tayyor mahsulot', 'All Item Groups', true, '{}'::jsonb)
        ON CONFLICT (name) DO NOTHING;

        INSERT INTO mini_customers (ref, name, payload_json)
        VALUES ('CUST-14', 'AMB customer', '{}'::jsonb)
        ON CONFLICT (ref) DO NOTHING;

        INSERT INTO mini_items (code, name, item_group, payload_json)
        VALUES
            ('AMB', 'Magnus', 'tayyor mahsulot', '{"code":"AMB","name":"Magnus"}'::jsonb),
            ('TG-a6b78c3080f65879', 'Magnus', 'tayyor mahsulot', '{"qolip_first_code":"5913-1"}'::jsonb),
            ('AMBIGUOUS', 'Ambiguous product', 'tayyor mahsulot', '{"code":"AMBIGUOUS"}'::jsonb),
            ('TG-AMBIGUOUS-A', 'Ambiguous product', 'tayyor mahsulot', '{}'::jsonb),
            ('TG-AMBIGUOUS-B', 'Ambiguous product', 'tayyor mahsulot', '{}'::jsonb);

        INSERT INTO mini_customer_items (customer_ref, item_code)
        VALUES
            ('CUST-14', 'AMB'),
            ('CUST-14', 'TG-a6b78c3080f65879'),
            ('CUST-14', 'AMBIGUOUS'),
            ('CUST-14', 'TG-AMBIGUOUS-A'),
            ('CUST-14', 'TG-AMBIGUOUS-B');

        INSERT INTO mini_orders (
            id, code, order_number, customer_ref, customer_name,
            product_code, product_name, status, kg
        )
        VALUES
            ('order-magnus', 'ORDER-MAGNUS', '0001', 'CUST-14', 'AMB customer',
             'AMB', 'Magnus', 'draft', 100),
            ('order-ambiguous', 'ORDER-AMBIGUOUS', '0002', 'CUST-14', 'AMB customer',
             'AMBIGUOUS', 'Ambiguous product', 'draft', 100);

        INSERT INTO mini_qolip_product_specs (
            item_code, item_name, item_group, qolip_code, size, payload_json
        )
        VALUES
            ('TG-a6b78c3080f65879', 'Magnus', 'tayyor mahsulot', '5913-1', 700,
             '{"item_code":"TG-a6b78c3080f65879","item_name":"Magnus","item_group":"tayyor mahsulot"}'::jsonb),
            ('TG-a6b78c3080f65879', 'Magnus', 'tayyor mahsulot', '5913-2', 700,
             '{"item_code":"TG-a6b78c3080f65879","item_name":"Magnus","item_group":"tayyor mahsulot"}'::jsonb),
            ('TG-AMBIGUOUS-A', 'Ambiguous product', 'tayyor mahsulot', 'AMB-A-1', 100, '{}'::jsonb),
            ('TG-AMBIGUOUS-B', 'Ambiguous product', 'tayyor mahsulot', 'AMB-B-1', 100, '{}'::jsonb);

        INSERT INTO mini_qolip_locations (
            id, block, item_code, item_name, qolip_code, size, quantity, payload_json
        )
        VALUES (
            'location-magnus', 'A', 'TG-a6b78c3080f65879', 'Magnus', '5913-1', 700, 1,
            '{"item_code":"TG-a6b78c3080f65879","item_name":"Magnus"}'::jsonb
        );

        INSERT INTO mini_qolip_checkouts (
            id, location_id, block, item_code, item_name, qolip_code, size, quantity,
            issued_to_ref, issued_to_name, status, payload_json
        )
        VALUES (
            'checkout-magnus', 'location-magnus', 'A', 'TG-a6b78c3080f65879',
            'Magnus', '5913-1', 700, 1, 'worker-1', 'Worker', 'open',
            '{"item_code":"TG-a6b78c3080f65879","item_name":"Magnus"}'::jsonb
        );

        INSERT INTO mini_qolip_order_notes (
            order_id, principal_role, principal_ref, principal_name,
            item_code, item_name, qolip_codes, status
        )
        VALUES (
            'order-magnus', 'qolipchi', 'qolipchi-1', 'Qolipchi',
            'TG-a6b78c3080f65879', 'Magnus', ARRAY['5913-1', '5913-2'], 'given'
        );
        "#,
    )
    .execute(pool)
    .await
    .expect("seed doctor test data");
}
