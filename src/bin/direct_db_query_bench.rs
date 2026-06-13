use std::path::PathBuf;
use std::time::{Duration, Instant};

use sqlx::MySqlPool;
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};

const DEFAULT_SITE_CONFIG: &str =
    "/Volumes/Samsung990P/local.git/erpnext_n1/erp/sites/erpfresh.localhost/site_config.json";
const ITERATIONS: usize = 300;
const WARMUPS: usize = 30;

#[derive(Debug, serde::Deserialize)]
struct SiteConfig {
    db_name: String,
    db_password: String,
}

#[derive(Debug)]
struct BenchContext {
    barcode: String,
    supplier: String,
    customer: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let site_config = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SITE_CONFIG));
    let raw = std::fs::read_to_string(&site_config)?;
    let site: SiteConfig = serde_json::from_str(&raw)?;

    let options = MySqlConnectOptions::new()
        .host("127.0.0.1")
        .port(3306)
        .username(&site.db_name)
        .password(&site.db_password)
        .database(&site.db_name);
    let pool = MySqlPoolOptions::new()
        .min_connections(1)
        .max_connections(4)
        .connect_with(options)
        .await?;

    let context = load_context(&pool).await?;
    println!(
        "context: barcode={} supplier={} customer={}",
        context.barcode, context.supplier, context.customer
    );

    assert_same_count(
        &pool,
        "stock_barcode",
        STOCK_BARCODE_CURRENT_SQL,
        STOCK_BARCODE_SARGABLE_SQL,
        &[&context.barcode],
    )
    .await?;
    assert_same_count(
        &pool,
        "delivery_pending",
        DELIVERY_PENDING_CURRENT_SQL,
        DELIVERY_PENDING_SARGABLE_SQL,
        &[],
    )
    .await?;
    assert_same_count(
        &pool,
        "delivery_confirmed",
        DELIVERY_CONFIRMED_CURRENT_SQL,
        DELIVERY_CONFIRMED_SARGABLE_SQL,
        &[],
    )
    .await?;

    bench_query(
        &pool,
        "stock_barcode_current",
        STOCK_BARCODE_CURRENT_SQL,
        &[&context.barcode],
    )
    .await?;
    bench_query(
        &pool,
        "stock_barcode_sargable",
        STOCK_BARCODE_SARGABLE_SQL,
        &[&context.barcode],
    )
    .await?;
    bench_query(
        &pool,
        "supplier_items_page",
        SUPPLIER_ITEMS_PAGE_SQL,
        &[&context.supplier],
    )
    .await?;
    bench_query(
        &pool,
        "customer_items_page",
        CUSTOMER_ITEMS_PAGE_SQL,
        &[&context.customer],
    )
    .await?;
    bench_query(
        &pool,
        "customer_items_item_ordered",
        CUSTOMER_ITEMS_ITEM_ORDERED_SQL,
        &[&context.customer],
    )
    .await?;
    bench_query(
        &pool,
        "delivery_pending_current",
        DELIVERY_PENDING_CURRENT_SQL,
        &[],
    )
    .await?;
    bench_query(
        &pool,
        "delivery_pending_sargable",
        DELIVERY_PENDING_SARGABLE_SQL,
        &[],
    )
    .await?;
    bench_query(
        &pool,
        "delivery_confirmed_current",
        DELIVERY_CONFIRMED_CURRENT_SQL,
        &[],
    )
    .await?;
    bench_query(
        &pool,
        "delivery_confirmed_sargable",
        DELIVERY_CONFIRMED_SARGABLE_SQL,
        &[],
    )
    .await?;

    Ok(())
}

async fn load_context(pool: &MySqlPool) -> Result<BenchContext, sqlx::Error> {
    let barcode = sqlx::query_scalar::<_, String>(
        r#"
            SELECT sed.barcode
            FROM `tabStock Entry Detail` sed
            WHERE COALESCE(sed.barcode, '') != ''
            GROUP BY sed.barcode
            ORDER BY COUNT(*) DESC, sed.barcode ASC
            LIMIT 1
        "#,
    )
    .fetch_one(pool)
    .await?;
    let supplier = sqlx::query_scalar::<_, String>(
        r#"
            SELECT isup.supplier
            FROM `tabItem Supplier` isup
            GROUP BY isup.supplier
            ORDER BY COUNT(*) DESC, isup.supplier ASC
            LIMIT 1
        "#,
    )
    .fetch_one(pool)
    .await?;
    let customer = sqlx::query_scalar::<_, String>(
        r#"
            SELECT icd.customer_name
            FROM `tabItem Customer Detail` icd
            GROUP BY icd.customer_name
            ORDER BY COUNT(*) DESC, icd.customer_name ASC
            LIMIT 1
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(BenchContext {
        barcode,
        supplier,
        customer,
    })
}

async fn assert_same_count(
    pool: &MySqlPool,
    name: &str,
    left_sql: &str,
    right_sql: &str,
    binds: &[&str],
) -> Result<(), sqlx::Error> {
    let left = query_count(pool, left_sql, binds).await?;
    let right = query_count(pool, right_sql, binds).await?;
    assert_eq!(left, right, "{name} count mismatch");
    println!("equality: {name} rows={left}");
    Ok(())
}

async fn bench_query(
    pool: &MySqlPool,
    name: &str,
    sql: &str,
    binds: &[&str],
) -> Result<(), sqlx::Error> {
    for _ in 0..WARMUPS {
        query_count(pool, sql, binds).await?;
    }

    let mut samples = Vec::with_capacity(ITERATIONS);
    let mut rows = 0usize;
    for _ in 0..ITERATIONS {
        let started = Instant::now();
        rows = query_count(pool, sql, binds).await?;
        samples.push(started.elapsed());
    }
    samples.sort_unstable();

    let total: Duration = samples.iter().copied().sum();
    let avg = total.as_secs_f64() * 1000.0 / samples.len() as f64;
    let median = millis(samples[samples.len() / 2]);
    let p95 = millis(samples[(samples.len() * 95 / 100).min(samples.len() - 1)]);
    let min = millis(samples[0]);
    println!(
        "{name}: rows={rows} avg={avg:.3}ms median={median:.3}ms p95={p95:.3}ms min={min:.3}ms"
    );
    Ok(())
}

async fn query_count(pool: &MySqlPool, sql: &str, binds: &[&str]) -> Result<usize, sqlx::Error> {
    let mut query = sqlx::query(sql);
    for value in binds {
        query = query.bind(*value);
    }
    Ok(query.fetch_all(pool).await?.len())
}

fn millis(value: Duration) -> f64 {
    value.as_secs_f64() * 1000.0
}

const STOCK_BARCODE_CURRENT_SQL: &str = r#"
    SELECT
        se.name AS stock_entry_name,
        sed.name AS detail_name,
        sed.barcode
    FROM `tabStock Entry Detail` sed
    INNER JOIN `tabStock Entry` se ON se.name = sed.parent
    LEFT JOIN tabItem i ON i.name = sed.item_code
    WHERE COALESCE(sed.barcode, '') = ?
    ORDER BY se.modified DESC, se.creation DESC, se.name DESC, sed.idx ASC
    LIMIT 20
"#;

const STOCK_BARCODE_SARGABLE_SQL: &str = r#"
    SELECT
        se.name AS stock_entry_name,
        sed.name AS detail_name,
        sed.barcode
    FROM `tabStock Entry Detail` sed
    INNER JOIN `tabStock Entry` se ON se.name = sed.parent
    LEFT JOIN tabItem i ON i.name = sed.item_code
    WHERE sed.barcode = ?
    ORDER BY se.modified DESC, se.creation DESC, se.name DESC, sed.idx ASC
    LIMIT 20
"#;

const SUPPLIER_ITEMS_PAGE_SQL: &str = r#"
    SELECT DISTINCT
        i.item_code,
        COALESCE(NULLIF(i.item_name, ''), i.item_code) AS item_name,
        COALESCE(NULLIF(i.stock_uom, ''), 'Nos') AS stock_uom
    FROM `tabItem Supplier` isup
    INNER JOIN tabItem i ON i.name = isup.parent
    WHERE isup.supplier = ?
      AND i.disabled = 0
    ORDER BY i.item_name ASC
    LIMIT 50 OFFSET 0
"#;

const CUSTOMER_ITEMS_PAGE_SQL: &str = r#"
    SELECT DISTINCT
        i.item_code,
        COALESCE(NULLIF(i.item_name, ''), i.item_code) AS item_name,
        COALESCE(NULLIF(i.stock_uom, ''), 'Nos') AS stock_uom
    FROM `tabItem Customer Detail` icd
    INNER JOIN tabItem i ON i.name = icd.parent
    WHERE icd.customer_name = ?
      AND i.disabled = 0
    ORDER BY i.item_name ASC
    LIMIT 50 OFFSET 0
"#;

const CUSTOMER_ITEMS_ITEM_ORDERED_SQL: &str = r#"
    SELECT DISTINCT
        i.item_code,
        COALESCE(NULLIF(i.item_name, ''), i.item_code) AS item_name,
        COALESCE(NULLIF(i.stock_uom, ''), 'Nos') AS stock_uom
    FROM tabItem i FORCE INDEX (item_name)
    INNER JOIN `tabItem Customer Detail` icd ON icd.parent = i.name AND icd.customer_name = ?
    WHERE i.disabled = 0
    ORDER BY i.item_name ASC
    LIMIT 50 OFFSET 0
"#;

const DELIVERY_PENDING_CURRENT_SQL: &str = r#"
    SELECT
        dn.name AS id,
        dni.item_code,
        dn.modified
    FROM `tabDelivery Note` dn
    LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    WHERE dn.docstatus = 1
      AND COALESCE(dn.accord_flow_state, 0) = 1
      AND COALESCE(dn.accord_customer_state, 0) NOT IN (2, 3, 4)
    ORDER BY dn.modified DESC
    LIMIT 20
"#;

const DELIVERY_PENDING_SARGABLE_SQL: &str = r#"
    SELECT
        dn.name AS id,
        dni.item_code,
        dn.modified
    FROM `tabDelivery Note` dn
    LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    WHERE dn.docstatus = 1
      AND dn.accord_flow_state = 1
      AND dn.accord_customer_state NOT IN (2, 3, 4)
    ORDER BY dn.modified DESC
    LIMIT 20
"#;

const DELIVERY_CONFIRMED_CURRENT_SQL: &str = r#"
    SELECT
        dn.name AS id,
        dni.item_code,
        dn.modified
    FROM `tabDelivery Note` dn
    LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    WHERE dn.docstatus = 1
      AND COALESCE(dn.accord_flow_state, 0) = 1
      AND COALESCE(dn.accord_customer_state, 0) = 3
    ORDER BY dn.modified DESC
"#;

const DELIVERY_CONFIRMED_SARGABLE_SQL: &str = r#"
    SELECT
        dn.name AS id,
        dni.item_code,
        dn.modified
    FROM `tabDelivery Note` dn
    LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    WHERE dn.docstatus = 1
      AND dn.accord_flow_state = 1
      AND dn.accord_customer_state = 3
    ORDER BY dn.modified DESC
"#;
