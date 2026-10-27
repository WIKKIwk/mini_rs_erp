use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};
use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;

use crate::core::production_map::{OrderProgressBatch, OrderRunSession, ProductionMapError};
use crate::core::qolip::mileage::{cycle_id, meter_units};
use crate::core::qolip::{QolipError, QolipMileage, QolipMileageCycle};
use crate::core::quantity::erp_quantity_from_units;

const CAPTURE: &str = "qolip_mileage_cycles";
const PRODUCER: &str = "qolip_mileage_apparatus_id";

async fn lock_code(tx: &mut Transaction<'_, Postgres>, code: &str) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("qolip:{}", code.trim().to_lowercase()))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(crate) async fn begin_cycle_tx(
    tx: &mut Transaction<'_, Postgres>,
    rework_id: &str,
    qolip_code: &str,
    returned_at: OffsetDateTime,
) -> Result<(), QolipError> {
    if rework_id.trim().is_empty() || qolip_code.trim().is_empty() {
        return Err(QolipError::StoreFailed);
    }
    lock_code(tx, qolip_code)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    let existing: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM mini_qolip_mileage_cycles
         WHERE rework_id=$1 AND qolip_code_key=lower(btrim($2)))",
    )
    .bind(rework_id.trim())
    .bind(qolip_code)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    if existing {
        return Ok(());
    }
    let returned = returned_at.unix_timestamp_nanos() as f64 / 1_000_000_000.0;
    sqlx::query(
        "UPDATE mini_qolip_mileage_cycles SET ended_at=to_timestamp($2::float8)
         WHERE qolip_code_key=lower(btrim($1)) AND ended_at IS NULL",
    )
    .bind(qolip_code)
    .bind(returned)
    .execute(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO mini_qolip_mileage_cycles(id,qolip_code,rework_id,started_at)
         VALUES($1,$2,$3,to_timestamp($4::float8))",
    )
    .bind(cycle_id(rework_id, qolip_code))
    .bind(qolip_code.trim())
    .bind(rework_id.trim())
    .bind(returned)
    .execute(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    Ok(())
}

pub(super) async fn read(pool: &PgPool, code: &str) -> Result<QolipMileage, QolipError> {
    let rows = sqlx::query_as::<_, (String, String, Option<String>, f64, String)>(
        "SELECT id,
            to_char(started_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
            to_char(ended_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
            meter::float8,rework_id
         FROM mini_qolip_mileage_cycles WHERE qolip_code_key=lower(btrim($1))
         ORDER BY started_at,id",
    )
    .bind(code)
    .fetch_all(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    let cycles = rows
        .into_iter()
        .map(
            |(id, started_at, ended_at, meter, rework_id)| QolipMileageCycle {
                id,
                started_at,
                ended_at,
                meter,
                rework_id,
            },
        )
        .collect::<Vec<_>>();
    Ok(QolipMileage {
        qolip_code: code.trim().to_string(),
        tracking_enabled: !cycles.is_empty(),
        current_meter: cycles
            .iter()
            .find(|cycle| cycle.ended_at.is_none())
            .map(|cycle| cycle.meter),
        cycles,
    })
}

fn actual_codes(payload: &Value) -> BTreeSet<String> {
    let mut codes = payload
        .get("qolip_codes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .chain(payload.get("qolip_code").and_then(Value::as_str))
        .map(|code| code.trim().to_lowercase())
        .filter(|code| !code.is_empty())
        .collect::<BTreeSet<_>>();
    codes.retain(|code| code.len() <= 512);
    codes
}

/// Capture at the physical tooling claim, rather than at a later WIP reprint.
/// A null cycle records that mileage was unknown/off at that claim.
pub(crate) async fn capture_session_cycles_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &mut OrderRunSession,
) -> Result<(), ProductionMapError> {
    let owner = session
        .payload_json
        .get("qolip_lock_owner")
        .and_then(Value::as_bool)
        == Some(true);
    if actual_codes(&session.payload_json).is_empty() {
        return Ok(());
    }
    let previous = sqlx::query_as::<_, (Value, f64)>(
        "SELECT payload_json,EXTRACT(EPOCH FROM created_at)::float8
         FROM mini_order_run_sessions WHERE session_id=$1 FOR UPDATE",
    )
    .bind(&session.session_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let reacquired = owner
        && previous.as_ref().is_some_and(|(payload, _)| {
            payload.get("qolip_lock_owner").and_then(Value::as_bool) != Some(true)
        });
    if !reacquired
        && let Some((payload, _)) = &previous
        && payload.get(CAPTURE).is_some_and(Value::is_object)
    {
        session.payload_json[CAPTURE] = payload[CAPTURE].clone();
        session.payload_json[PRODUCER] = payload[PRODUCER].clone();
        return Ok(());
    }
    // Inherited plate lineage on lamination/cutting never proves physical use.
    if !owner {
        return Ok(());
    }
    let is_print: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM mini_apparatus
         WHERE id=$1 AND execution_profile_json->>'operation'='print')",
    )
    .bind(session.apparatus.trim())
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if !is_print {
        return Ok(());
    }
    // For an old session first touched after deployment, use its original DB
    // creation time. Do not activate old work against a later peredelka return.
    let captured_at = if reacquired || previous.is_none() {
        OffsetDateTime::now_utc().unix_timestamp_nanos() as f64 / 1_000_000_000.0
    } else {
        previous.as_ref().map(|(_, at)| *at).unwrap_or_default()
    };
    let mut captured = Map::new();
    for code in actual_codes(&session.payload_json) {
        lock_code(tx, &code)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let cycle: Option<String> = sqlx::query_scalar(
            "SELECT id FROM mini_qolip_mileage_cycles WHERE qolip_code_key=$1
             AND started_at<=to_timestamp($2::float8)
             AND (ended_at IS NULL OR ended_at>to_timestamp($2::float8))
             ORDER BY started_at DESC,id LIMIT 1",
        )
        .bind(&code)
        .bind(captured_at)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        captured.insert(code, cycle.map(Value::String).unwrap_or(Value::Null));
    }
    session.payload_json[CAPTURE] = Value::Object(captured);
    session.payload_json[PRODUCER] = Value::String(session.apparatus.clone());
    Ok(())
}

fn output_meter(batch: &OrderProgressBatch) -> Option<f64> {
    let meter = batch.finished_goods_meter.or_else(|| {
        matches!(
            batch.uom.trim().to_lowercase().as_str(),
            "m" | "meter" | "metr"
        )
        .then_some(batch.produced_qty)
    })?;
    meter_units(meter)
        .ok()
        .filter(|units| *units > 0)
        .map(erp_quantity_from_units)
}

fn production_source(batch: &OrderProgressBatch) -> bool {
    !batch.order_id.starts_with("training-")
        && !batch.order_id.starts_with("demo-")
        && batch.payload_json.get("training").and_then(Value::as_bool) != Some(true)
        && batch
            .payload_json
            .get("origin")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        && batch.started_at_unix > 0
        && batch.completed_at_unix >= batch.started_at_unix
}

/// Call only when this actual output is first inserted. Every normal later WIP
/// upsert (including splitting/remainders) is stock handling, not new plate use.
pub(crate) async fn record_new_output_tx(
    tx: &mut Transaction<'_, Postgres>,
    batch: &OrderProgressBatch,
) -> Result<(), ProductionMapError> {
    if !production_source(batch) {
        return Ok(());
    }
    let Some(meter) = output_meter(batch) else {
        return Ok(());
    };
    let source = sqlx::query_scalar::<_, Value>(
        "SELECT session.payload_json FROM mini_order_run_sessions session
         JOIN mini_apparatus apparatus ON apparatus.id=session.canonical_apparatus_id
         WHERE session.session_id=$1 AND session.order_id=$2
           AND session.canonical_apparatus_id=$3
           AND apparatus.execution_profile_json->>'operation'='print'
           AND session.payload_json->>'qolip_mileage_apparatus_id'=$3",
    )
    .bind(&batch.session_id)
    .bind(&batch.order_id)
    .bind(&batch.apparatus)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let Some(captured) = source
        .as_ref()
        .and_then(|payload| payload.get(CAPTURE))
        .and_then(Value::as_object)
    else {
        return Ok(());
    };
    for (code, cycle) in captured.iter().collect::<BTreeMap<_, _>>() {
        lock_code(tx, code)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let cycle_id = cycle.as_str();
        if let Some(id) = cycle_id {
            // WIP production bounds have second precision. The claimed cycle
            // is immutable; tolerate only that existing timestamp granularity.
            let in_interval: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM mini_qolip_mileage_cycles WHERE id=$1
                 AND qolip_code_key=$2 AND floor(EXTRACT(EPOCH FROM started_at))::float8<=$3::float8
                 AND (ended_at IS NULL OR ceil(EXTRACT(EPOCH FROM ended_at))::float8>=$4::float8))",
            )
            .bind(id)
            .bind(code)
            .bind(batch.started_at_unix as f64)
            .bind(batch.completed_at_unix as f64)
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
            if !in_interval {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
        }
        let inserted = sqlx::query_scalar::<_, Option<String>>(
            "INSERT INTO mini_qolip_mileage_contributions
             (batch_id,qolip_code_key,cycle_id,session_id,canonical_apparatus_id,
              production_started_at,production_completed_at,meter)
             VALUES($1,$2,$3,$4,$5,to_timestamp($6::float8),to_timestamp($7::float8),
                    ($8::float8)::numeric(18,6))
             ON CONFLICT(batch_id,qolip_code_key) DO NOTHING RETURNING cycle_id",
        )
        .bind(&batch.batch_id)
        .bind(code)
        .bind(cycle_id)
        .bind(&batch.session_id)
        .bind(&batch.apparatus)
        .bind(batch.started_at_unix as f64)
        .bind(batch.completed_at_unix as f64)
        .bind(meter)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if let Some(Some(id)) = inserted {
            sqlx::query("UPDATE mini_qolip_mileage_cycles SET meter=meter+($2::float8)::numeric(18,6) WHERE id=$1")
                .bind(id).bind(meter).execute(&mut **tx).await
                .map_err(|_| ProductionMapError::StoreFailed)?;
        }
    }
    Ok(())
}

/// Only an explicit meter correction changes the original contribution; its
/// old cycle (including null/off) remains immutable after later returns.
pub(crate) async fn lock_output_sources_tx(
    tx: &mut Transaction<'_, Postgres>,
    batch_id: &str,
) -> Result<(), ProductionMapError> {
    let codes = sqlx::query_scalar::<_, String>(
        "SELECT qolip_code_key FROM mini_qolip_mileage_contributions
         WHERE batch_id=$1 ORDER BY qolip_code_key",
    )
    .bind(batch_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    for code in codes {
        lock_code(tx, &code)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(())
}

pub(crate) async fn correct_output_tx(
    tx: &mut Transaction<'_, Postgres>,
    batch: &OrderProgressBatch,
) -> Result<(), ProductionMapError> {
    let sources = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT qolip_code_key,cycle_id FROM mini_qolip_mileage_contributions
         WHERE batch_id=$1 ORDER BY qolip_code_key",
    )
    .bind(&batch.batch_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let Some(meter) = output_meter(batch) else {
        return if sources.iter().any(|(_, cycle)| cycle.is_some()) {
            Err(ProductionMapError::ProgressInputInvalid)
        } else {
            Ok(())
        };
    };
    for (code, cycle_id) in sources {
        lock_code(tx, &code)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        // The surrounding batch revision CAS serializes corrections. Re-read
        // the source under its row lock to keep the delta atomic with returns.
        let previous: f64 = sqlx::query_scalar(
            "SELECT meter::float8 FROM mini_qolip_mileage_contributions
             WHERE batch_id=$1 AND qolip_code_key=$2 FOR UPDATE",
        )
        .bind(&batch.batch_id)
        .bind(&code)
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if let Some(id) = cycle_id {
            sqlx::query("UPDATE mini_qolip_mileage_cycles SET meter=meter-($2::float8)::numeric(18,6)+($3::float8)::numeric(18,6) WHERE id=$1")
                .bind(id).bind(previous).bind(meter).execute(&mut **tx).await
                .map_err(|_| ProductionMapError::StoreFailed)?;
        }
        sqlx::query("UPDATE mini_qolip_mileage_contributions SET meter=($3::float8)::numeric(18,6) WHERE batch_id=$1 AND qolip_code_key=$2")
            .bind(&batch.batch_id).bind(code).bind(meter).execute(&mut **tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(())
}
