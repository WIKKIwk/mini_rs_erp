use sqlx::{Postgres, Transaction};

use crate::core::production_map::{
    OrderProgressBatch, ProductionMapError, QueueActionProgressWrite,
};

fn outputs(write: &QueueActionProgressWrite) -> &[OrderProgressBatch] {
    if write.progress_batches.is_empty() {
        write.progress_batch.as_slice()
    } else {
        &write.progress_batches
    }
}

pub(super) async fn lock_for_output(
    tx: &mut Transaction<'_, Postgres>,
    write: &QueueActionProgressWrite,
) -> Result<Option<String>, ProductionMapError> {
    let Some(code) = write
        .event
        .payload_json
        .get("output_paddon_code")
        .and_then(serde_json::Value::as_str)
        .filter(|code| !code.trim().is_empty())
    else {
        return Ok(None);
    };
    if outputs(write).is_empty() {
        return Ok(None);
    }
    // Pallets have no lifecycle column. A warehouse receipt, when supported
    // by this database, seals the package. Row JSON also supports databases
    // that have not introduced receipt metadata yet.
    let (id, receipt) = sqlx::query_as::<_, (String, Option<serde_json::Value>)>(
        "SELECT p.id, to_jsonb(p)->'receipt_json' FROM mini_paddons p WHERE p.code = $1 FOR UPDATE",
    )
    .bind(code.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::PaddonNotFound)?;
    if receipt.is_some_and(|value| !value.is_null()) {
        return Err(ProductionMapError::PaddonInvalidInput);
    }
    Ok(Some(id))
}

pub(super) async fn assign_outputs(
    tx: &mut Transaction<'_, Postgres>,
    paddon_id: &str,
    write: &QueueActionProgressWrite,
) -> Result<(), ProductionMapError> {
    for batch in outputs(write) {
        let existing = sqlx::query_scalar::<_, String>(
            "SELECT paddon_id FROM mini_paddon_items WHERE progress_batch_id = $1 AND removed_at IS NULL FOR UPDATE",
        ).bind(&batch.batch_id).fetch_optional(&mut **tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        if let Some(existing) = existing {
            if existing != paddon_id {
                return Err(ProductionMapError::PaddonItemAlreadyAssigned);
            }
            continue;
        }
        sqlx::query(
            "INSERT INTO mini_paddon_items (id, paddon_id, progress_batch_id, added_by_ref, added_by_display_name)
             VALUES ($1, $2, $3, $4, $5)",
        ).bind(format!("paddon-output:{}:{}", write.event.event_id, batch.batch_id))
            .bind(paddon_id).bind(&batch.batch_id)
            .bind(&write.event.actor.ref_).bind(&write.event.actor.display_name)
            .execute(&mut **tx).await.map_err(|error| match error {
                sqlx::Error::Database(error) if error.constraint() == Some("idx_mini_paddon_items_active_batch") =>
                    ProductionMapError::PaddonItemAlreadyAssigned,
                _ => ProductionMapError::StoreFailed,
            })?;
    }
    sqlx::query("UPDATE mini_paddons SET updated_at = now() WHERE id = $1")
        .bind(paddon_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}
