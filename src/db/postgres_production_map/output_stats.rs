use chrono::DateTime;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::core::production_map::output_stats::{
    MAX_POINTS, OutputStatsError, OutputStatsReport, OutputStatsRequest, utc_string,
};

pub(super) async fn load(
    pool: &PgPool,
    request: &OutputStatsRequest,
    scope: Option<&[String]>,
) -> Result<OutputStatsReport, OutputStatsError> {
    let mut tx = pool.begin().await.map_err(database_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    sqlx::query("SET LOCAL statement_timeout = '8s'")
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    let (micros, apparatus): (i64, Value) = sqlx::query_as(include_str!("output_stats_scope.sql"))
        .bind(scope.map(<[String]>::to_vec))
        .bind(request.apparatus.as_deref())
        .bind(utc_string(request.from))
        .bind(utc_string(request.to))
        .fetch_one(&mut *tx)
        .await
        .map_err(database_error)?;
    let as_of = DateTime::from_timestamp_micros(micros).ok_or(OutputStatsError::StoreFailed)?;
    let apparatus = apparatus.as_array().ok_or(OutputStatsError::StoreFailed)?;
    if request.apparatus.is_some() && apparatus.is_empty() {
        return Err(OutputStatsError::Invalid("output_stats_unknown_apparatus"));
    }
    let points = request.boundaries(as_of)?;
    // Check the entire requested range even if part of it is still in the future.
    let requested_points = request.boundaries(request.to)?.len();
    if apparatus.len() > MAX_POINTS || apparatus.len().saturating_mul(requested_points) > MAX_POINTS
    {
        return Err(OutputStatsError::Invalid("output_stats_too_many_points"));
    }
    let ids: Vec<_> = apparatus
        .iter()
        .map(|a| {
            a["id"]
                .as_str()
                .map(str::to_owned)
                .ok_or(OutputStatsError::StoreFailed)
        })
        .collect::<Result<_, _>>()?;
    let metadata = json!({
        "ok": true, "from": utc_string(request.from), "to": utc_string(request.to),
        "timezone": request.timezone.name(), "bucket": request.bucket,
        "as_of": utc_string(as_of), "quantity_basis": "latest_revision",
    });
    let body: String = sqlx::query_scalar(include_str!("output_stats.sql"))
        .bind(utc_string(request.from))
        .bind(utc_string(request.to))
        .bind(utc_string(as_of))
        .bind(ids)
        .bind(serde_json::to_value(&points).map_err(|_| OutputStatsError::StoreFailed)?)
        .bind(Value::Array(apparatus.clone()))
        .bind(metadata)
        .fetch_one(&mut *tx)
        .await
        .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    Ok(OutputStatsReport(body))
}

fn database_error(error: sqlx::Error) -> OutputStatsError {
    tracing::error!(%error, "production output report failed");
    OutputStatsError::StoreFailed
}

#[cfg(test)]
mod tests;
