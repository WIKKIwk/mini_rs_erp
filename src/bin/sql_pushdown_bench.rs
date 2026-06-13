use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
use sqlx::{MySqlPool, Row};

const DEFAULT_SITE_CONFIG: &str =
    "/Volumes/Samsung990P/local.git/erpnext_n1/erp/sites/erpfresh.localhost/site_config.json";
const ITERATIONS: usize = 300;

#[derive(Debug, serde::Deserialize)]
struct SiteConfig {
    db_name: String,
    db_password: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Summary {
    pending: i64,
    confirmed: i64,
    returned: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
struct ReceiptRow {
    name: String,
    supplier: String,
    supplier_name: String,
    doc_status: i32,
    status: String,
    total_qty: f64,
    posting_date: String,
    supplier_delivery_note: String,
    remarks: String,
    currency: String,
    item_code: String,
    item_name: String,
    uom: String,
    amount: f64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
struct DeliveryRow {
    name: String,
    customer: String,
    customer_name: String,
    doc_status: i32,
    modified: String,
    qty: f64,
    returned_qty: f64,
    item_code: String,
    item_name: String,
    uom: String,
    accord_flow_state: i32,
    accord_customer_state: i32,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Record {
    id: String,
    supplier_ref: String,
    supplier_name: String,
    uom: String,
    sent_qty: f64,
    accepted_qty: f64,
    status: String,
    created_label: String,
}

#[derive(Debug, Clone, Default)]
struct Breakdown {
    supplier_ref: String,
    supplier_name: String,
    uom: String,
    receipt_count: i64,
    total_sent_qty: f64,
    total_accepted_qty: f64,
    total_returned_qty: f64,
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

    compare_outputs(&pool).await?;
    run_benchmarks(&pool).await?;
    Ok(())
}

async fn compare_outputs(pool: &MySqlPool) -> Result<(), sqlx::Error> {
    let raw_summary = raw_summary(pool).await?;
    let pushed_summary = pushed_summary(pool).await?;
    assert_eq!(raw_summary, pushed_summary, "summary mismatch");

    let raw_pending = raw_pending(pool).await?;
    let pushed_pending = pushed_pending(pool).await?;
    assert_eq!(
        record_ids(&raw_pending),
        record_ids(&pushed_pending),
        "pending id/order mismatch"
    );

    for kind in ["pending", "confirmed", "returned"] {
        let raw_breakdown = normalize_breakdown(raw_status_breakdown(pool, kind).await?);
        let pushed_breakdown = normalize_breakdown(pushed_status_breakdown(pool, kind).await?);
        assert_breakdown_eq(kind, &raw_breakdown, &pushed_breakdown);
    }

    println!("equality: ok");
    println!(
        "summary: pending={} confirmed={} returned={}",
        raw_summary.pending, raw_summary.confirmed, raw_summary.returned
    );
    println!("pending_records: {}", raw_pending.len());
    Ok(())
}

async fn run_benchmarks(pool: &MySqlPool) -> Result<(), sqlx::Error> {
    bench("summary_raw_rows_rust_count", ITERATIONS, || async {
        raw_summary(pool).await.map(|_| ())
    })
    .await?;
    bench("summary_sql_pushdown", ITERATIONS, || async {
        pushed_summary(pool).await.map(|_| ())
    })
    .await?;
    bench("pending_raw_rows_rust_filter", ITERATIONS, || async {
        raw_pending(pool).await.map(|_| ())
    })
    .await?;
    bench("pending_sql_pushdown", ITERATIONS, || async {
        pushed_pending(pool).await.map(|_| ())
    })
    .await?;

    for kind in ["pending", "confirmed", "returned"] {
        bench(
            &format!("breakdown_raw_rows_rust_group:{kind}"),
            ITERATIONS,
            || async { raw_status_breakdown(pool, kind).await.map(|_| ()) },
        )
        .await?;
        bench(
            &format!("breakdown_sql_pushdown:{kind}"),
            ITERATIONS,
            || async { pushed_status_breakdown(pool, kind).await.map(|_| ()) },
        )
        .await?;
    }
    Ok(())
}

async fn bench<F, Fut>(name: &str, iterations: usize, mut op: F) -> Result<(), sqlx::Error>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), sqlx::Error>>,
{
    for _ in 0..20 {
        op().await?;
    }

    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        op().await?;
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let total: Duration = samples.iter().copied().sum();
    let avg = total.as_secs_f64() * 1000.0 / samples.len() as f64;
    let median = millis(samples[samples.len() / 2]);
    let p95 = millis(samples[(samples.len() * 95 / 100).min(samples.len() - 1)]);
    let min = millis(samples[0]);
    println!("{name}: avg={avg:.3}ms median={median:.3}ms p95={p95:.3}ms min={min:.3}ms");
    Ok(())
}

fn millis(value: Duration) -> f64 {
    value.as_secs_f64() * 1000.0
}

async fn raw_summary(pool: &MySqlPool) -> Result<Summary, sqlx::Error> {
    let receipts = fetch_receipts(pool, PURCHASE_RECEIPT_ROWS_SQL).await?;
    let deliveries = fetch_deliveries(pool, DELIVERY_NOTE_ROWS_SQL).await?;
    let mut summary = Summary::default();

    for row in &receipts {
        let (status, include) = receipt_status(row);
        if include {
            count_status(&mut summary, &status);
        }
    }
    for row in &deliveries {
        if delivery_visible(row) {
            count_status(&mut summary, &delivery_status(row));
        }
    }
    Ok(summary)
}

async fn pushed_summary(pool: &MySqlPool) -> Result<Summary, sqlx::Error> {
    let row = sqlx::query(PUSHED_SUMMARY_SQL).fetch_one(pool).await?;
    Ok(Summary {
        pending: row.try_get::<i64, _>("pending_count")?,
        confirmed: row.try_get::<i64, _>("confirmed_count")?,
        returned: row.try_get::<i64, _>("returned_count")?,
    })
}

async fn raw_pending(pool: &MySqlPool) -> Result<Vec<Record>, sqlx::Error> {
    let receipts = fetch_receipts(pool, PENDING_PURCHASE_RECEIPT_ROWS_SQL).await?;
    let deliveries = fetch_deliveries(pool, PENDING_DELIVERY_NOTE_ROWS_SQL).await?;
    let mut result = Vec::new();

    for row in &receipts {
        let (_, include) = receipt_status(row);
        if !include {
            continue;
        }
        let record = receipt_record(row);
        if record.status == "pending" || record.status == "draft" {
            result.push(record);
        }
    }
    for row in &deliveries {
        if delivery_visible(row) && delivery_status(row) == "pending" {
            result.push(delivery_record(row));
        }
    }
    sort_records(&mut result);
    Ok(result)
}

async fn pushed_pending(pool: &MySqlPool) -> Result<Vec<Record>, sqlx::Error> {
    fetch_records(pool, PUSHED_PENDING_SQL).await
}

async fn raw_status_breakdown(pool: &MySqlPool, kind: &str) -> Result<Vec<Breakdown>, sqlx::Error> {
    let receipts = fetch_receipts(pool, PURCHASE_RECEIPT_ROWS_SQL).await?;
    let deliveries = fetch_deliveries(pool, DELIVERY_NOTE_ROWS_SQL).await?;
    let mut grouped = BTreeMap::<String, Breakdown>::new();

    for row in &receipts {
        add_breakdown(&mut grouped, receipt_record(row), kind);
    }
    for row in &deliveries {
        add_breakdown(&mut grouped, delivery_record(row), kind);
    }
    Ok(sort_breakdown(grouped.into_values().collect()))
}

async fn pushed_status_breakdown(
    pool: &MySqlPool,
    kind: &str,
) -> Result<Vec<Breakdown>, sqlx::Error> {
    let sql = match kind {
        "pending" => PUSHED_BREAKDOWN_PENDING_SQL,
        "confirmed" => PUSHED_BREAKDOWN_CONFIRMED_SQL,
        "returned" => PUSHED_BREAKDOWN_RETURNED_SQL,
        _ => PUSHED_BREAKDOWN_EMPTY_SQL,
    };
    let rows = sqlx::query(sql).fetch_all(pool).await?;
    rows.into_iter()
        .map(|row| {
            Ok(Breakdown {
                supplier_ref: row.try_get::<String, _>("supplier_ref")?,
                supplier_name: row.try_get::<String, _>("supplier_name")?,
                uom: row.try_get::<String, _>("uom")?,
                receipt_count: row.try_get::<i64, _>("receipt_count")?,
                total_sent_qty: row.try_get::<f64, _>("total_sent_qty")?,
                total_accepted_qty: row.try_get::<f64, _>("total_accepted_qty")?,
                total_returned_qty: row.try_get::<f64, _>("total_returned_qty")?,
            })
        })
        .collect()
}

async fn fetch_receipts(pool: &MySqlPool, sql: &str) -> Result<Vec<ReceiptRow>, sqlx::Error> {
    let rows = sqlx::query(sql).fetch_all(pool).await?;
    rows.into_iter()
        .map(|row| {
            Ok(ReceiptRow {
                name: row.try_get("name")?,
                supplier: row.try_get("supplier")?,
                supplier_name: row.try_get("supplier_name")?,
                doc_status: row.try_get("doc_status")?,
                status: row.try_get("status")?,
                total_qty: row.try_get("total_qty")?,
                posting_date: row.try_get("posting_date")?,
                supplier_delivery_note: row.try_get("supplier_delivery_note")?,
                remarks: row.try_get("remarks")?,
                currency: row.try_get("currency")?,
                item_code: row.try_get("item_code")?,
                item_name: row.try_get("item_name")?,
                uom: row.try_get("uom")?,
                amount: row.try_get("amount")?,
            })
        })
        .collect()
}

async fn fetch_deliveries(pool: &MySqlPool, sql: &str) -> Result<Vec<DeliveryRow>, sqlx::Error> {
    let rows = sqlx::query(sql).fetch_all(pool).await?;
    rows.into_iter()
        .map(|row| {
            Ok(DeliveryRow {
                name: row.try_get("name")?,
                customer: row.try_get("customer")?,
                customer_name: row.try_get("customer_name")?,
                doc_status: row.try_get("doc_status")?,
                modified: row.try_get("modified")?,
                qty: row.try_get("qty")?,
                returned_qty: row.try_get("returned_qty")?,
                item_code: row.try_get("item_code")?,
                item_name: row.try_get("item_name")?,
                uom: row.try_get("uom")?,
                accord_flow_state: row.try_get("accord_flow_state")?,
                accord_customer_state: row.try_get("accord_customer_state")?,
            })
        })
        .collect()
}

async fn fetch_records(pool: &MySqlPool, sql: &str) -> Result<Vec<Record>, sqlx::Error> {
    let rows = sqlx::query(sql).fetch_all(pool).await?;
    rows.into_iter()
        .map(|row| {
            Ok(Record {
                id: row.try_get("id")?,
                supplier_ref: row.try_get("supplier_ref")?,
                supplier_name: row.try_get("supplier_name")?,
                uom: row.try_get("uom")?,
                sent_qty: row.try_get("sent_qty")?,
                accepted_qty: row.try_get("accepted_qty")?,
                status: row.try_get("status")?,
                created_label: row.try_get("created_label")?,
            })
        })
        .collect()
}

fn count_status(summary: &mut Summary, status: &str) {
    match status {
        "pending" | "draft" => summary.pending += 1,
        "accepted" => summary.confirmed += 1,
        "partial" | "rejected" | "cancelled" => summary.returned += 1,
        _ => {}
    }
}

fn receipt_status(row: &ReceiptRow) -> (String, bool) {
    let mut sent_qty = row.total_qty;
    if let Some(marker_qty) = parse_marker_qty(&row.supplier_delivery_note)
        && marker_qty > sent_qty
    {
        sent_qty = marker_qty;
    }
    let status = if row.doc_status == 2 || row.status.trim().eq_ignore_ascii_case("Cancelled") {
        "cancelled"
    } else if row.doc_status == 1 {
        if row.total_qty <= 0.0 {
            "rejected"
        } else if sent_qty > 0.0 && row.total_qty < sent_qty {
            "partial"
        } else {
            "accepted"
        }
    } else if row.status.trim().eq_ignore_ascii_case("Draft") {
        "draft"
    } else {
        "pending"
    };

    let unannounced_state = extract_unannounced_state(&row.remarks);
    if row.doc_status == 0 && unannounced_state == "pending" {
        return (status.to_string(), false);
    }
    if status == "accepted" && unannounced_state == "approved" {
        return (status.to_string(), false);
    }
    (status.to_string(), true)
}

fn receipt_record(row: &ReceiptRow) -> Record {
    let (status, _) = receipt_status(row);
    let mut sent_qty = row.total_qty;
    if let Some(marker_qty) = parse_marker_qty(&row.supplier_delivery_note)
        && marker_qty > sent_qty
    {
        sent_qty = marker_qty;
    }
    let accepted_qty = match status.as_str() {
        "accepted" | "partial" => row.total_qty,
        _ => 0.0,
    };
    Record {
        id: row.name.trim().to_string(),
        supplier_ref: row.supplier.trim().to_string(),
        supplier_name: row.supplier_name.trim().to_string(),
        uom: row.uom.trim().to_string(),
        sent_qty,
        accepted_qty,
        status,
        created_label: row.posting_date.trim().to_string(),
    }
}

fn delivery_record(row: &DeliveryRow) -> Record {
    let status = delivery_status(row);
    let accepted_qty = match status.as_str() {
        "accepted" => row.qty,
        "partial" => {
            let returned_qty = if row.returned_qty <= 0.0 {
                row.qty.max(0.0)
            } else {
                row.returned_qty
            };
            (row.qty - returned_qty).max(0.0)
        }
        _ => 0.0,
    };
    Record {
        id: row.name.trim().to_string(),
        supplier_ref: row.customer.trim().to_string(),
        supplier_name: row.customer_name.trim().to_string(),
        uom: row.uom.trim().to_string(),
        sent_qty: row.qty,
        accepted_qty,
        status,
        created_label: row.modified.trim().to_string(),
    }
}

fn delivery_visible(row: &DeliveryRow) -> bool {
    row.doc_status == 1 && row.accord_flow_state == 1
}

fn delivery_status(row: &DeliveryRow) -> String {
    if row.doc_status != 1 {
        return "draft".to_string();
    }
    if row.accord_flow_state != 1 {
        return "pending".to_string();
    }
    match row.accord_customer_state {
        2 => "rejected",
        3 => "accepted",
        4 => "partial",
        _ => "pending",
    }
    .to_string()
}

fn add_breakdown(grouped: &mut BTreeMap<String, Breakdown>, record: Record, kind: &str) {
    if !matches_kind(&record.status, kind) {
        return;
    }
    let key = if record.supplier_ref.trim().is_empty() {
        record.supplier_name.trim().to_string()
    } else {
        record.supplier_ref.trim().to_string()
    };
    let entry = grouped.entry(key).or_insert_with(|| Breakdown {
        supplier_ref: record.supplier_ref.clone(),
        supplier_name: record.supplier_name.clone(),
        uom: record.uom.clone(),
        ..Breakdown::default()
    });
    entry.receipt_count += 1;
    entry.total_sent_qty += record.sent_qty;
    entry.total_accepted_qty += record.accepted_qty;
    entry.total_returned_qty += (record.sent_qty - record.accepted_qty).max(0.0);
    if entry.uom.trim().is_empty() {
        entry.uom = record.uom;
    }
}

fn matches_kind(status: &str, kind: &str) -> bool {
    match kind {
        "pending" => status == "pending" || status == "draft",
        "confirmed" => status == "accepted",
        "returned" => status == "partial" || status == "rejected" || status == "cancelled",
        _ => false,
    }
}

fn sort_breakdown(mut items: Vec<Breakdown>) -> Vec<Breakdown> {
    items.sort_by(|left, right| {
        right.receipt_count.cmp(&left.receipt_count).then_with(|| {
            left.supplier_name
                .to_lowercase()
                .cmp(&right.supplier_name.to_lowercase())
        })
    });
    items
}

fn normalize_breakdown(items: Vec<Breakdown>) -> Vec<Breakdown> {
    items
        .into_iter()
        .map(|mut item| {
            item.total_sent_qty = round4(item.total_sent_qty);
            item.total_accepted_qty = round4(item.total_accepted_qty);
            item.total_returned_qty = round4(item.total_returned_qty);
            item
        })
        .collect()
}

fn assert_breakdown_eq(kind: &str, left: &[Breakdown], right: &[Breakdown]) {
    assert_eq!(left.len(), right.len(), "breakdown length mismatch: {kind}");
    for (left, right) in left.iter().zip(right.iter()) {
        assert_eq!(
            left.supplier_ref, right.supplier_ref,
            "supplier_ref: {kind}"
        );
        assert_eq!(
            left.supplier_name, right.supplier_name,
            "supplier_name: {kind}"
        );
        assert_eq!(left.receipt_count, right.receipt_count, "count: {kind}");
        assert_eq!(left.total_sent_qty, right.total_sent_qty, "sent: {kind}");
        assert_eq!(
            left.total_accepted_qty, right.total_accepted_qty,
            "accepted: {kind}"
        );
        assert_eq!(
            left.total_returned_qty, right.total_returned_qty,
            "returned: {kind}"
        );
    }
}

fn sort_records(items: &mut [Record]) {
    items.sort_by(|left, right| right.created_label.cmp(&left.created_label));
}

fn record_ids(items: &[Record]) -> Vec<&str> {
    items.iter().map(|item| item.id.as_str()).collect()
}

fn parse_marker_qty(marker: &str) -> Option<f64> {
    let trimmed = marker.trim();
    if !trimmed.starts_with("TG:") {
        return None;
    }
    trimmed
        .split(':')
        .next_back()
        .and_then(|value| value.trim().parse::<f64>().ok())
}

fn extract_unannounced_state(remarks: &str) -> String {
    remarks
        .replace("\r\n", "\n")
        .lines()
        .map(str::trim)
        .find_map(|line| {
            line.strip_prefix("Accord Werka Aytilmagan:")
                .map(|value| value.trim().to_lowercase())
        })
        .unwrap_or_default()
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

const PURCHASE_RECEIPT_ROWS_SQL: &str = r#"
    SELECT
        pr.name AS name,
        pr.supplier AS supplier,
        COALESCE(pr.supplier_name, '') AS supplier_name,
        pr.docstatus AS doc_status,
        COALESCE(pr.status, '') AS status,
        CAST(COALESCE(pr.total_qty, 0) AS DOUBLE) AS total_qty,
        COALESCE(CAST(pr.posting_date AS CHAR), '') AS posting_date,
        COALESCE(pr.supplier_delivery_note, '') AS supplier_delivery_note,
        COALESCE(pr.remarks, '') AS remarks,
        COALESCE(pr.currency, '') AS currency,
        COALESCE(pri.item_code, '') AS item_code,
        COALESCE(pri.item_name, '') AS item_name,
        COALESCE(pri.uom, '') AS uom,
        CAST(COALESCE(pri.amount, 0) AS DOUBLE) AS amount
    FROM `tabPurchase Receipt` pr
    LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
    WHERE pr.supplier_delivery_note LIKE 'TG:%'
    ORDER BY pr.name DESC
"#;

const DELIVERY_NOTE_ROWS_SQL: &str = r#"
    SELECT
        dn.name AS name,
        dn.customer AS customer,
        COALESCE(dn.customer_name, '') AS customer_name,
        dn.docstatus AS doc_status,
        COALESCE(CAST(dn.modified AS CHAR), '') AS modified,
        CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS qty,
        CAST(COALESCE(dni.returned_qty, 0) AS DOUBLE) AS returned_qty,
        COALESCE(dni.item_code, '') AS item_code,
        COALESCE(dni.item_name, '') AS item_name,
        COALESCE(dni.uom, '') AS uom,
        COALESCE(dn.accord_flow_state, 0) AS accord_flow_state,
        COALESCE(dn.accord_customer_state, 0) AS accord_customer_state
    FROM `tabDelivery Note` dn
    LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    ORDER BY dn.name DESC
"#;

const PENDING_PURCHASE_RECEIPT_ROWS_SQL: &str = r#"
    SELECT
        pr.name AS name,
        pr.supplier AS supplier,
        COALESCE(pr.supplier_name, '') AS supplier_name,
        pr.docstatus AS doc_status,
        COALESCE(pr.status, '') AS status,
        CAST(COALESCE(pr.total_qty, 0) AS DOUBLE) AS total_qty,
        COALESCE(CAST(pr.posting_date AS CHAR), '') AS posting_date,
        COALESCE(pr.supplier_delivery_note, '') AS supplier_delivery_note,
        COALESCE(pr.remarks, '') AS remarks,
        COALESCE(pr.currency, '') AS currency,
        COALESCE(pri.item_code, '') AS item_code,
        COALESCE(pri.item_name, '') AS item_name,
        COALESCE(pri.uom, '') AS uom,
        CAST(COALESCE(pri.amount, 0) AS DOUBLE) AS amount
    FROM `tabPurchase Receipt` pr
    LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
    WHERE pr.supplier_delivery_note LIKE 'TG:%'
      AND pr.docstatus = 0
    ORDER BY pr.name DESC
"#;

const PENDING_DELIVERY_NOTE_ROWS_SQL: &str = r#"
    SELECT
        dn.name AS name,
        dn.customer AS customer,
        COALESCE(dn.customer_name, '') AS customer_name,
        dn.docstatus AS doc_status,
        COALESCE(CAST(dn.modified AS CHAR), '') AS modified,
        CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS qty,
        CAST(COALESCE(dni.returned_qty, 0) AS DOUBLE) AS returned_qty,
        COALESCE(dni.item_code, '') AS item_code,
        COALESCE(dni.item_name, '') AS item_name,
        COALESCE(dni.uom, '') AS uom,
        COALESCE(dn.accord_flow_state, 0) AS accord_flow_state,
        COALESCE(dn.accord_customer_state, 0) AS accord_customer_state
    FROM `tabDelivery Note` dn
    LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    WHERE dn.docstatus = 1
      AND COALESCE(dn.accord_flow_state, 0) = 1
      AND COALESCE(dn.accord_customer_state, 0) NOT IN (2, 3, 4)
    ORDER BY dn.name DESC
"#;

const PUSHED_SUMMARY_SQL: &str = r#"
    SELECT
        CAST(SUM(status IN ('pending', 'draft')) AS SIGNED) AS pending_count,
        CAST(SUM(status = 'accepted') AS SIGNED) AS confirmed_count,
        CAST(SUM(status IN ('partial', 'rejected', 'cancelled')) AS SIGNED) AS returned_count
    FROM (
        SELECT
            CASE
                WHEN pr.docstatus = 2 OR TRIM(COALESCE(pr.status, '')) = 'Cancelled' THEN 'cancelled'
                WHEN pr.docstatus = 1 THEN
                    CASE
                        WHEN COALESCE(pr.total_qty, 0) <= 0 THEN 'rejected'
                        WHEN GREATEST(COALESCE(pr.total_qty, 0), CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))) > 0
                             AND COALESCE(pr.total_qty, 0) < GREATEST(COALESCE(pr.total_qty, 0), CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))) THEN 'partial'
                        ELSE 'accepted'
                    END
                WHEN TRIM(COALESCE(pr.status, '')) = 'Draft' THEN 'draft'
                ELSE 'pending'
            END AS status
        FROM `tabPurchase Receipt` pr
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
          AND NOT (pr.docstatus = 0 AND COALESCE(pr.remarks, '') LIKE '%Accord Werka Aytilmagan: pending%')
          AND NOT (pr.docstatus = 1 AND COALESCE(pr.remarks, '') LIKE '%Accord Werka Aytilmagan: approved%')
        UNION ALL
        SELECT
            CASE COALESCE(dn.accord_customer_state, 0)
                WHEN 2 THEN 'rejected'
                WHEN 3 THEN 'accepted'
                WHEN 4 THEN 'partial'
                ELSE 'pending'
            END AS status
        FROM `tabDelivery Note` dn
        WHERE dn.docstatus = 1
          AND COALESCE(dn.accord_flow_state, 0) = 1
    ) statuses
"#;

const PUSHED_PENDING_SQL: &str = r#"
    SELECT *
    FROM (
        SELECT
            pr.name AS id,
            pr.supplier AS supplier_ref,
            COALESCE(pr.supplier_name, '') AS supplier_name,
            COALESCE(pri.uom, '') AS uom,
            CAST(GREATEST(COALESCE(pr.total_qty, 0), CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))) AS DOUBLE) AS sent_qty,
            CAST(0 AS DOUBLE) AS accepted_qty,
            CASE WHEN TRIM(COALESCE(pr.status, '')) = 'Draft' THEN 'draft' ELSE 'pending' END AS status,
            COALESCE(CAST(pr.posting_date AS CHAR), '') AS created_label
        FROM `tabPurchase Receipt` pr
        LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
          AND pr.docstatus = 0
          AND COALESCE(pr.remarks, '') NOT LIKE '%Accord Werka Aytilmagan: pending%'
        UNION ALL
        SELECT
            dn.name AS id,
            dn.customer AS supplier_ref,
            COALESCE(dn.customer_name, '') AS supplier_name,
            COALESCE(dni.uom, '') AS uom,
            CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS sent_qty,
            CAST(0 AS DOUBLE) AS accepted_qty,
            'pending' AS status,
            COALESCE(CAST(dn.modified AS CHAR), '') AS created_label
        FROM `tabDelivery Note` dn
        LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
        WHERE dn.docstatus = 1
          AND COALESCE(dn.accord_flow_state, 0) = 1
          AND COALESCE(dn.accord_customer_state, 0) NOT IN (2, 3, 4)
    ) pending_rows
    ORDER BY created_label DESC
"#;

const PUSHED_BREAKDOWN_PENDING_SQL: &str = r#"
    SELECT supplier_ref, supplier_name, MIN(uom) AS uom, COUNT(*) AS receipt_count,
           CAST(SUM(sent_qty) AS DOUBLE) AS total_sent_qty,
           CAST(SUM(accepted_qty) AS DOUBLE) AS total_accepted_qty,
           CAST(SUM(GREATEST(sent_qty - accepted_qty, 0)) AS DOUBLE) AS total_returned_qty
    FROM (
        SELECT pr.supplier AS supplier_ref, COALESCE(pr.supplier_name, '') AS supplier_name, COALESCE(pri.uom, '') AS uom,
               CAST(GREATEST(COALESCE(pr.total_qty, 0), CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))) AS DOUBLE) AS sent_qty,
               CAST(0 AS DOUBLE) AS accepted_qty,
               CASE WHEN TRIM(COALESCE(pr.status, '')) = 'Draft' THEN 'draft' ELSE 'pending' END AS status
        FROM `tabPurchase Receipt` pr
        LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
          AND pr.docstatus = 0
        UNION ALL
        SELECT dn.customer AS supplier_ref, COALESCE(dn.customer_name, '') AS supplier_name, COALESCE(dni.uom, '') AS uom,
               CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS sent_qty,
               CAST(0 AS DOUBLE) AS accepted_qty,
               CASE
                   WHEN dn.docstatus != 1 THEN 'draft'
                   WHEN COALESCE(dn.accord_flow_state, 0) != 1 THEN 'pending'
                   WHEN COALESCE(dn.accord_customer_state, 0) = 2 THEN 'rejected'
                   WHEN COALESCE(dn.accord_customer_state, 0) = 3 THEN 'accepted'
                   WHEN COALESCE(dn.accord_customer_state, 0) = 4 THEN 'partial'
                   ELSE 'pending'
               END AS status
        FROM `tabDelivery Note` dn
        LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    ) records
    WHERE status IN ('pending', 'draft')
    GROUP BY supplier_ref, supplier_name
    ORDER BY receipt_count DESC, LOWER(supplier_name) ASC
"#;

const PUSHED_BREAKDOWN_CONFIRMED_SQL: &str = r#"
    SELECT supplier_ref, supplier_name, MIN(uom) AS uom, COUNT(*) AS receipt_count,
           CAST(SUM(sent_qty) AS DOUBLE) AS total_sent_qty,
           CAST(SUM(accepted_qty) AS DOUBLE) AS total_accepted_qty,
           CAST(SUM(GREATEST(sent_qty - accepted_qty, 0)) AS DOUBLE) AS total_returned_qty
    FROM (
        SELECT pr.supplier AS supplier_ref, COALESCE(pr.supplier_name, '') AS supplier_name, COALESCE(pri.uom, '') AS uom,
               CAST(GREATEST(COALESCE(pr.total_qty, 0), CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))) AS DOUBLE) AS sent_qty,
               CAST(COALESCE(pr.total_qty, 0) AS DOUBLE) AS accepted_qty,
               CASE
                   WHEN pr.docstatus = 2 OR TRIM(COALESCE(pr.status, '')) = 'Cancelled' THEN 'cancelled'
                   WHEN pr.docstatus = 1 THEN
                       CASE
                           WHEN COALESCE(pr.total_qty, 0) <= 0 THEN 'rejected'
                           WHEN GREATEST(COALESCE(pr.total_qty, 0), CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))) > 0
                                AND COALESCE(pr.total_qty, 0) < GREATEST(COALESCE(pr.total_qty, 0), CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))) THEN 'partial'
                           ELSE 'accepted'
                       END
                   WHEN TRIM(COALESCE(pr.status, '')) = 'Draft' THEN 'draft'
                   ELSE 'pending'
               END AS status
        FROM `tabPurchase Receipt` pr
        LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
        UNION ALL
        SELECT dn.customer AS supplier_ref, COALESCE(dn.customer_name, '') AS supplier_name, COALESCE(dni.uom, '') AS uom,
               CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS sent_qty,
               CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS accepted_qty,
               CASE
                   WHEN dn.docstatus != 1 THEN 'draft'
                   WHEN COALESCE(dn.accord_flow_state, 0) != 1 THEN 'pending'
                   WHEN COALESCE(dn.accord_customer_state, 0) = 2 THEN 'rejected'
                   WHEN COALESCE(dn.accord_customer_state, 0) = 3 THEN 'accepted'
                   WHEN COALESCE(dn.accord_customer_state, 0) = 4 THEN 'partial'
                   ELSE 'pending'
               END AS status
        FROM `tabDelivery Note` dn
        LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    ) records
    WHERE status = 'accepted'
    GROUP BY supplier_ref, supplier_name
    ORDER BY receipt_count DESC, LOWER(supplier_name) ASC
"#;

const PUSHED_BREAKDOWN_RETURNED_SQL: &str = r#"
    SELECT supplier_ref, supplier_name, MIN(uom) AS uom, COUNT(*) AS receipt_count,
           CAST(SUM(sent_qty) AS DOUBLE) AS total_sent_qty,
           CAST(SUM(accepted_qty) AS DOUBLE) AS total_accepted_qty,
           CAST(SUM(GREATEST(sent_qty - accepted_qty, 0)) AS DOUBLE) AS total_returned_qty
    FROM (
        SELECT pr.supplier AS supplier_ref, COALESCE(pr.supplier_name, '') AS supplier_name, COALESCE(pri.uom, '') AS uom,
               CAST(GREATEST(COALESCE(pr.total_qty, 0), CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))) AS DOUBLE) AS sent_qty,
               CASE
                   WHEN pr.docstatus = 1 AND COALESCE(pr.total_qty, 0) > 0 THEN CAST(COALESCE(pr.total_qty, 0) AS DOUBLE)
                   ELSE CAST(0 AS DOUBLE)
               END AS accepted_qty,
               CASE
                   WHEN pr.docstatus = 2 OR TRIM(COALESCE(pr.status, '')) = 'Cancelled' THEN 'cancelled'
                   WHEN pr.docstatus = 1 THEN
                       CASE
                           WHEN COALESCE(pr.total_qty, 0) <= 0 THEN 'rejected'
                           WHEN GREATEST(COALESCE(pr.total_qty, 0), CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))) > 0
                                AND COALESCE(pr.total_qty, 0) < GREATEST(COALESCE(pr.total_qty, 0), CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))) THEN 'partial'
                           ELSE 'accepted'
                       END
                   WHEN TRIM(COALESCE(pr.status, '')) = 'Draft' THEN 'draft'
                   ELSE 'pending'
               END AS status
        FROM `tabPurchase Receipt` pr
        LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
        UNION ALL
        SELECT dn.customer AS supplier_ref, COALESCE(dn.customer_name, '') AS supplier_name, COALESCE(dni.uom, '') AS uom,
               CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS sent_qty,
               CASE
                   WHEN COALESCE(dn.accord_customer_state, 0) = 4 THEN CAST(GREATEST(COALESCE(dn.total_qty, 0) - CASE WHEN COALESCE(dni.returned_qty, 0) <= 0 THEN COALESCE(dn.total_qty, 0) ELSE COALESCE(dni.returned_qty, 0) END, 0) AS DOUBLE)
                   ELSE CAST(0 AS DOUBLE)
               END AS accepted_qty,
               CASE
                   WHEN dn.docstatus != 1 THEN 'draft'
                   WHEN COALESCE(dn.accord_flow_state, 0) != 1 THEN 'pending'
                   WHEN COALESCE(dn.accord_customer_state, 0) = 2 THEN 'rejected'
                   WHEN COALESCE(dn.accord_customer_state, 0) = 3 THEN 'accepted'
                   WHEN COALESCE(dn.accord_customer_state, 0) = 4 THEN 'partial'
                   ELSE 'pending'
               END AS status
        FROM `tabDelivery Note` dn
        LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    ) records
    WHERE status IN ('partial', 'rejected', 'cancelled')
    GROUP BY supplier_ref, supplier_name
    ORDER BY receipt_count DESC, LOWER(supplier_name) ASC
"#;

const PUSHED_BREAKDOWN_EMPTY_SQL: &str = "SELECT '' AS supplier_ref, '' AS supplier_name, '' AS uom, 0 AS receipt_count, 0.0 AS total_sent_qty, 0.0 AS total_accepted_qty, 0.0 AS total_returned_qty WHERE 1=0";
