use sqlx::{FromRow, PgPool};

use crate::core::production_map::{LaminatsiyaAstatkaReport, ProductionMapError};

#[derive(Debug, FromRow)]
struct LaminatsiyaAstatkaReportRow {
    report_id: String,
    order_id: String,
    apparatus: String,
    from_at_unix: i64,
    to_at_unix: i64,
    lamination_print_leftover_rolls: f64,
    lamination_film_leftover_rolls: f64,
    total_waste: f64,
    worker_role: String,
    worker_ref: String,
    worker_display_name: String,
    description: String,
    created_at_unix: i64,
}

pub(super) async fn load_laminatsiya_astatka_reports_for_order(
    pool: &PgPool,
    order_id: &str,
) -> Result<Vec<LaminatsiyaAstatkaReport>, ProductionMapError> {
    let rows = sqlx::query_as::<_, LaminatsiyaAstatkaReportRow>(
        r#"SELECT
             report_id,
             order_id,
             apparatus,
             EXTRACT(EPOCH FROM from_at)::bigint AS from_at_unix,
             EXTRACT(EPOCH FROM to_at)::bigint AS to_at_unix,
             lamination_print_leftover_rolls::double precision AS lamination_print_leftover_rolls,
             lamination_film_leftover_rolls::double precision AS lamination_film_leftover_rolls,
             total_waste::double precision AS total_waste,
             worker_role,
             worker_ref,
             worker_display_name,
             description,
             EXTRACT(EPOCH FROM created_at)::bigint AS created_at_unix
           FROM mini_laminatsiya_astatka_reports
           WHERE order_id = $1
           ORDER BY to_at ASC, created_at ASC, report_id ASC"#,
    )
    .bind(order_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    Ok(rows
        .into_iter()
        .map(|row| LaminatsiyaAstatkaReport {
            report_id: row.report_id,
            order_id: row.order_id,
            apparatus: row.apparatus,
            from_at_unix: row.from_at_unix,
            to_at_unix: row.to_at_unix,
            lamination_print_leftover_rolls: row.lamination_print_leftover_rolls,
            lamination_film_leftover_rolls: row.lamination_film_leftover_rolls,
            total_waste: row.total_waste,
            worker_role: row.worker_role,
            worker_ref: row.worker_ref,
            worker_display_name: row.worker_display_name,
            description: row.description,
            created_at_unix: row.created_at_unix,
        })
        .collect())
}

pub(super) async fn put_laminatsiya_astatka_report(
    pool: &PgPool,
    report: &LaminatsiyaAstatkaReport,
) -> Result<(), ProductionMapError> {
    sqlx::query(
        r#"INSERT INTO mini_laminatsiya_astatka_reports (
             report_id,
             order_id,
             apparatus,
             from_at,
             to_at,
             lamination_print_leftover_rolls,
             lamination_film_leftover_rolls,
             total_waste,
             worker_role,
             worker_ref,
             worker_display_name,
             description,
             created_at
         )
         VALUES ($1, $2, $3, to_timestamp($4), to_timestamp($5), $6, $7, $8,
                 $9, $10, $11, $12, to_timestamp($13))
         ON CONFLICT (report_id) DO UPDATE SET
             order_id = EXCLUDED.order_id,
             apparatus = EXCLUDED.apparatus,
             from_at = EXCLUDED.from_at,
             to_at = EXCLUDED.to_at,
             lamination_print_leftover_rolls = EXCLUDED.lamination_print_leftover_rolls,
             lamination_film_leftover_rolls = EXCLUDED.lamination_film_leftover_rolls,
             total_waste = EXCLUDED.total_waste,
             worker_role = EXCLUDED.worker_role,
             worker_ref = EXCLUDED.worker_ref,
             worker_display_name = EXCLUDED.worker_display_name,
             description = EXCLUDED.description,
             created_at = EXCLUDED.created_at"#,
    )
    .bind(report.report_id.trim())
    .bind(report.order_id.trim())
    .bind(report.apparatus.trim())
    .bind(report.from_at_unix as f64)
    .bind(report.to_at_unix as f64)
    .bind(report.lamination_print_leftover_rolls)
    .bind(report.lamination_film_leftover_rolls)
    .bind(report.total_waste)
    .bind(report.worker_role.trim())
    .bind(report.worker_ref.trim())
    .bind(report.worker_display_name.trim())
    .bind(report.description.trim())
    .bind(report.created_at_unix as f64)
    .execute(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}
