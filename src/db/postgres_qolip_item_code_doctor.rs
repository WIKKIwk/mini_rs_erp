use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sqlx::{PgPool, Postgres, Row, Transaction};

const DEFAULT_INTERVAL_SECONDS: u64 = 300;
const MIN_INTERVAL_SECONDS: u64 = 30;
const MAX_INTERVAL_SECONDS: u64 = 24 * 60 * 60;
const DOCTOR_LOCK_KEY: i64 = 7_228_113_866_947_301_421;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QolipItemCodeDoctorReport {
    pub repaired_products: usize,
    pub product_specs_updated: u64,
    pub locations_updated: u64,
    pub open_checkouts_updated: u64,
    pub order_notes_updated: u64,
}

#[derive(Clone)]
pub struct PostgresQolipItemCodeDoctor {
    inner: Arc<PostgresQolipItemCodeDoctorInner>,
}

struct PostgresQolipItemCodeDoctorInner {
    pool: PgPool,
    enabled: bool,
    interval: Duration,
    scheduler_started: AtomicBool,
}

#[derive(Debug)]
struct RepairCandidate {
    source_item_code: String,
    canonical_item_code: String,
    item_name: String,
    item_group: String,
    first_qolip_code: String,
}

impl PostgresQolipItemCodeDoctor {
    pub fn from_env(pool: PgPool) -> Self {
        let enabled = std::env::var("MINI_ERP_QOLIP_ITEM_CODE_DOCTOR_ENABLED")
            .ok()
            .map(|value| {
                let value = value.trim();
                value != "0"
                    && !value.eq_ignore_ascii_case("false")
                    && !value.eq_ignore_ascii_case("no")
                    && !value.eq_ignore_ascii_case("off")
            })
            .unwrap_or(true);
        let interval_seconds = std::env::var("MINI_ERP_QOLIP_ITEM_CODE_DOCTOR_INTERVAL_SECONDS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_INTERVAL_SECONDS)
            .clamp(MIN_INTERVAL_SECONDS, MAX_INTERVAL_SECONDS);
        Self::new(pool, enabled, Duration::from_secs(interval_seconds))
    }

    pub fn new(pool: PgPool, enabled: bool, interval: Duration) -> Self {
        Self {
            inner: Arc::new(PostgresQolipItemCodeDoctorInner {
                pool,
                enabled,
                interval,
                scheduler_started: AtomicBool::new(false),
            }),
        }
    }

    pub fn start_scheduler(&self) {
        if !self.inner.enabled || self.inner.scheduler_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            self.inner.scheduler_started.store(false, Ordering::Release);
            tracing::warn!("qolip item-code doctor scheduler could not find a Tokio runtime");
            return;
        };
        let doctor = self.clone();
        runtime.spawn(async move {
            loop {
                doctor.run_and_log().await;
                tokio::time::sleep(doctor.inner.interval).await;
            }
        });
    }

    pub async fn run_once(&self) -> Result<QolipItemCodeDoctorReport, sqlx::Error> {
        let mut transaction = self.inner.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(DOCTOR_LOCK_KEY)
            .execute(&mut *transaction)
            .await?;

        let candidates = load_repair_candidates(&mut transaction).await?;
        let mut report = QolipItemCodeDoctorReport::default();
        for candidate in candidates {
            repair_candidate(&mut transaction, &candidate, &mut report).await?;
        }
        transaction.commit().await?;
        Ok(report)
    }

    async fn run_and_log(&self) {
        match self.run_once().await {
            Ok(report) if report.repaired_products > 0 => tracing::info!(
                repaired_products = report.repaired_products,
                product_specs_updated = report.product_specs_updated,
                locations_updated = report.locations_updated,
                open_checkouts_updated = report.open_checkouts_updated,
                order_notes_updated = report.order_notes_updated,
                "qolip item-code doctor repaired catalog identity mismatches"
            ),
            Ok(_) => tracing::trace!("qolip item-code doctor found no repair candidates"),
            Err(error) => tracing::warn!(%error, "qolip item-code doctor run failed"),
        }
    }
}

async fn load_repair_candidates(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<Vec<RepairCandidate>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        WITH referenced_codes AS (
            SELECT lower(btrim(product_code)) AS code_key
            FROM mini_orders
            WHERE btrim(product_code) <> ''
            UNION
            SELECT lower(btrim(product_code)) AS code_key
            FROM mini_production_maps
            WHERE order_id IS NOT NULL
              AND btrim(product_code) <> ''
        ),
        referenced_items AS (
            SELECT item.code, item.name, item.item_group
            FROM mini_items item
            JOIN referenced_codes reference
              ON reference.code_key = lower(btrim(item.code))
        ),
        canonical_items AS (
            SELECT referenced.*,
                   count(*) OVER (
                       PARTITION BY lower(btrim(referenced.name)),
                                    lower(btrim(referenced.item_group))
                   ) AS referenced_item_count
            FROM referenced_items referenced
        ),
        raw_candidates AS (
            SELECT
                source.code AS source_item_code,
                canonical.code AS canonical_item_code,
                canonical.name AS item_name,
                canonical.item_group,
                COALESCE(
                    NULLIF(btrim(source.payload_json->>'qolip_first_code'), ''),
                    (
                        SELECT spec.qolip_code
                        FROM mini_qolip_product_specs spec
                        WHERE lower(btrim(spec.item_code)) = lower(btrim(source.code))
                        ORDER BY spec.created_at ASC, lower(spec.qolip_code)
                        LIMIT 1
                    ),
                    (
                        SELECT location.qolip_code
                        FROM mini_qolip_locations location
                        WHERE lower(btrim(location.item_code)) = lower(btrim(source.code))
                        ORDER BY location.created_at ASC, lower(location.qolip_code)
                        LIMIT 1
                    ),
                    (
                        SELECT checkout.qolip_code
                        FROM mini_qolip_checkouts checkout
                        WHERE lower(btrim(checkout.item_code)) = lower(btrim(source.code))
                          AND lower(checkout.status) = 'open'
                        ORDER BY checkout.created_at ASC, lower(checkout.qolip_code)
                        LIMIT 1
                    ),
                    ''
                ) AS first_qolip_code
            FROM canonical_items canonical
            JOIN mini_items source
              ON lower(btrim(source.name)) = lower(btrim(canonical.name))
             AND lower(btrim(source.item_group)) = lower(btrim(canonical.item_group))
             AND lower(btrim(source.code)) <> lower(btrim(canonical.code))
            WHERE canonical.referenced_item_count = 1
              AND lower(btrim(canonical.code)) NOT LIKE 'tg-%'
              AND lower(btrim(source.code)) LIKE 'tg-%'
              AND NOT EXISTS (
                  SELECT 1
                  FROM referenced_codes reference
                  WHERE reference.code_key = lower(btrim(source.code))
              )
              AND EXISTS (
                  SELECT 1
                  FROM mini_customer_items canonical_customer
                  JOIN mini_customer_items source_customer
                    ON source_customer.customer_ref = canonical_customer.customer_ref
                  WHERE lower(btrim(canonical_customer.item_code)) = lower(btrim(canonical.code))
                    AND lower(btrim(source_customer.item_code)) = lower(btrim(source.code))
              )
              AND (
                  EXISTS (
                      SELECT 1 FROM mini_qolip_product_specs spec
                      WHERE lower(btrim(spec.item_code)) = lower(btrim(source.code))
                  )
                  OR EXISTS (
                      SELECT 1 FROM mini_qolip_locations location
                      WHERE lower(btrim(location.item_code)) = lower(btrim(source.code))
                  )
                  OR EXISTS (
                      SELECT 1 FROM mini_qolip_checkouts checkout
                      WHERE lower(btrim(checkout.item_code)) = lower(btrim(source.code))
                        AND lower(checkout.status) = 'open'
                  )
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM mini_qolip_product_specs spec
                  WHERE lower(btrim(spec.item_code)) = lower(btrim(source.code))
                    AND (
                        lower(btrim(spec.item_name)) <> lower(btrim(canonical.name))
                        OR lower(btrim(spec.item_group)) <> lower(btrim(canonical.item_group))
                    )
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM mini_qolip_locations location
                  WHERE lower(btrim(location.item_code)) = lower(btrim(source.code))
                    AND lower(btrim(location.item_name)) <> lower(btrim(canonical.name))
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM mini_qolip_checkouts checkout
                  WHERE lower(btrim(checkout.item_code)) = lower(btrim(source.code))
                    AND lower(checkout.status) = 'open'
                    AND lower(btrim(checkout.item_name)) <> lower(btrim(canonical.name))
              )
        ),
        ranked_candidates AS (
            SELECT candidate.*,
                   count(*) OVER (
                       PARTITION BY lower(btrim(candidate.canonical_item_code))
                   ) AS source_item_count
            FROM raw_candidates candidate
        )
        SELECT source_item_code, canonical_item_code, item_name, item_group, first_qolip_code
        FROM ranked_candidates
        WHERE source_item_count = 1
        ORDER BY lower(canonical_item_code), lower(source_item_code)
        "#,
    )
    .fetch_all(&mut **transaction)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| RepairCandidate {
            source_item_code: row.get("source_item_code"),
            canonical_item_code: row.get("canonical_item_code"),
            item_name: row.get("item_name"),
            item_group: row.get("item_group"),
            first_qolip_code: row.get("first_qolip_code"),
        })
        .collect())
}

async fn repair_candidate(
    transaction: &mut Transaction<'_, Postgres>,
    candidate: &RepairCandidate,
    report: &mut QolipItemCodeDoctorReport,
) -> Result<(), sqlx::Error> {
    let _locked_items = sqlx::query_scalar::<_, String>(
        "SELECT code
         FROM mini_items
         WHERE lower(btrim(code)) = ANY($1)
         ORDER BY lower(btrim(code))
         FOR UPDATE",
    )
    .bind(vec![
        candidate.source_item_code.trim().to_lowercase(),
        candidate.canonical_item_code.trim().to_lowercase(),
    ])
    .fetch_all(&mut **transaction)
    .await?;

    let product_specs_updated = sqlx::query(
        "UPDATE mini_qolip_product_specs
         SET item_code = $2,
             item_name = $3,
             item_group = $4,
             payload_json = jsonb_set(
                 jsonb_set(
                     jsonb_set(COALESCE(payload_json, '{}'::jsonb), '{item_code}', to_jsonb($2::text), true),
                     '{item_name}', to_jsonb($3::text), true
                 ),
                 '{item_group}', to_jsonb($4::text), true
             ),
             updated_at = now()
         WHERE lower(btrim(item_code)) = lower(btrim($1))
           AND lower(btrim(item_name)) = lower(btrim($3))
           AND lower(btrim(item_group)) = lower(btrim($4))",
    )
    .bind(&candidate.source_item_code)
    .bind(&candidate.canonical_item_code)
    .bind(&candidate.item_name)
    .bind(&candidate.item_group)
    .execute(&mut **transaction)
    .await?
    .rows_affected();

    let locations_updated = update_current_qolip_projection(
        transaction,
        "mini_qolip_locations",
        &candidate.source_item_code,
        &candidate.canonical_item_code,
        &candidate.item_name,
        false,
    )
    .await?;
    let open_checkouts_updated = update_current_qolip_projection(
        transaction,
        "mini_qolip_checkouts",
        &candidate.source_item_code,
        &candidate.canonical_item_code,
        &candidate.item_name,
        true,
    )
    .await?;

    let order_notes_updated = sqlx::query(
        "UPDATE mini_qolip_order_notes note
         SET item_code = $2,
             item_name = $3,
             updated_at = now()
         FROM mini_orders orders
         WHERE note.order_id = orders.id
           AND lower(btrim(note.item_code)) = lower(btrim($1))
           AND lower(btrim(orders.product_code)) = lower(btrim($2))
           AND lower(btrim(orders.product_name)) = lower(btrim($3))",
    )
    .bind(&candidate.source_item_code)
    .bind(&candidate.canonical_item_code)
    .bind(&candidate.item_name)
    .execute(&mut **transaction)
    .await?
    .rows_affected();

    let total_updated =
        product_specs_updated + locations_updated + open_checkouts_updated + order_notes_updated;
    if total_updated == 0 {
        return Ok(());
    }

    if !candidate.first_qolip_code.trim().is_empty() {
        sqlx::query(
            "UPDATE mini_items
             SET payload_json = jsonb_set(
                     COALESCE(payload_json, '{}'::jsonb),
                     '{qolip_first_code}',
                     to_jsonb($2::text),
                     true
                 ),
                 updated_at = now()
             WHERE lower(btrim(code)) = lower(btrim($1))
               AND COALESCE(btrim(payload_json->>'qolip_first_code'), '') = ''",
        )
        .bind(&candidate.canonical_item_code)
        .bind(candidate.first_qolip_code.trim())
        .execute(&mut **transaction)
        .await?;
    }
    sqlx::query(
        "UPDATE mini_items
         SET payload_json = COALESCE(payload_json, '{}'::jsonb) - 'qolip_first_code',
             updated_at = now()
         WHERE lower(btrim(code)) = lower(btrim($1))
           AND payload_json ? 'qolip_first_code'",
    )
    .bind(&candidate.source_item_code)
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        "INSERT INTO mini_qolip_item_code_repairs (
             source_item_code, canonical_item_code, item_name, item_group,
             product_specs_updated, locations_updated, open_checkouts_updated,
             order_notes_updated
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(&candidate.source_item_code)
    .bind(&candidate.canonical_item_code)
    .bind(&candidate.item_name)
    .bind(&candidate.item_group)
    .bind(product_specs_updated as i64)
    .bind(locations_updated as i64)
    .bind(open_checkouts_updated as i64)
    .bind(order_notes_updated as i64)
    .execute(&mut **transaction)
    .await?;

    report.repaired_products += 1;
    report.product_specs_updated += product_specs_updated;
    report.locations_updated += locations_updated;
    report.open_checkouts_updated += open_checkouts_updated;
    report.order_notes_updated += order_notes_updated;
    Ok(())
}

async fn update_current_qolip_projection(
    transaction: &mut Transaction<'_, Postgres>,
    table: &str,
    source_item_code: &str,
    canonical_item_code: &str,
    item_name: &str,
    open_only: bool,
) -> Result<u64, sqlx::Error> {
    debug_assert!(matches!(
        table,
        "mini_qolip_locations" | "mini_qolip_checkouts"
    ));
    let status_filter = if open_only {
        " AND lower(status) = 'open'"
    } else {
        ""
    };
    let statement = format!(
        "UPDATE {table}
         SET item_code = $2,
             item_name = $3,
             payload_json = jsonb_set(
                 jsonb_set(COALESCE(payload_json, '{{}}'::jsonb), '{{item_code}}', to_jsonb($2::text), true),
                 '{{item_name}}', to_jsonb($3::text), true
             ),
             updated_at = now()
         WHERE lower(btrim(item_code)) = lower(btrim($1))
           AND lower(btrim(item_name)) = lower(btrim($3)){status_filter}"
    );
    Ok(sqlx::query(&statement)
        .bind(source_item_code)
        .bind(canonical_item_code)
        .bind(item_name)
        .execute(&mut **transaction)
        .await?
        .rows_affected())
}

#[cfg(test)]
mod tests;
