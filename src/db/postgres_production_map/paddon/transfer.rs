use sqlx::PgPool;

use crate::core::production_map::{PaddonTransferInput, ProductionMapError, QueueActionActor};

#[derive(sqlx::FromRow)]
struct TransferPaddon {
    id: String,
    code: String,
    apparatus_id: String,
    created_by_ref: String,
    locked: bool,
    received: bool,
}

pub(super) async fn transfer(
    pool: &PgPool,
    input: &PaddonTransferInput,
    actor: &QueueActionActor,
    allowed: Option<&[String]>,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    // Keep visibility changes serialized with this write, just like free movement edits.
    let (visible, free_movement): (bool, bool) = sqlx::query_as(
        "SELECT worker_visibility_enabled, free_movement_enabled FROM mini_paddon_management_settings WHERE singleton FOR SHARE",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    // All membership writes/print/receipt operations lock pallets before rolls.
    // Stable ordering also makes simultaneous transfers in either direction safe.
    let paddons = sqlx::query_as::<_, TransferPaddon>(
        "SELECT id, code, apparatus_id, created_by_ref, locked_at IS NOT NULL AS locked,
                receipt_json IS NOT NULL AS received
         FROM mini_paddons WHERE code=ANY($1) ORDER BY id FOR UPDATE",
    )
    .bind([&input.source_code, &input.target_code])
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if paddons.len() != 2
        || paddons.iter().any(|p| {
            actor.role == "aparatchi" && !visible && p.created_by_ref.trim() != actor.ref_.trim()
        })
    {
        return Err(ProductionMapError::PaddonNotFound);
    }
    if paddons.iter().any(|p| p.received) {
        return Err(ProductionMapError::PaddonAlreadyReceived);
    }
    // Free movement never bypasses a physical pallet's seal.
    if paddons.iter().any(|p| p.locked) {
        return Err(ProductionMapError::PaddonLocked);
    }
    if actor.role == "aparatchi"
        && !free_movement
        && paddons
            .iter()
            .any(|p| p.created_by_ref.trim() != actor.ref_.trim())
    {
        return Err(ProductionMapError::PaddonTransferForbidden);
    }
    let source = paddons
        .iter()
        .find(|p| p.code == input.source_code)
        .ok_or(ProductionMapError::PaddonNotFound)?;
    let target = paddons
        .iter()
        .find(|p| p.code == input.target_code)
        .ok_or(ProductionMapError::PaddonNotFound)?;
    if let Some(allowed) = allowed {
        for p in &paddons {
            super::paddon_movement::ensure_apparatus_access(actor, &p.apparatus_id, &target.apparatus_id, Some(allowed))?;
        }
    } else if actor.role == "aparatchi" && source.apparatus_id != target.apparatus_id {
        return Err(ProductionMapError::PaddonTransferForbidden);
    }
    let mut batch_ids = input.progress_batch_ids.clone();
    batch_ids.sort();
    let mut memberships = Vec::with_capacity(batch_ids.len());
    for batch_id in &batch_ids {
        super::paddon_movement::lock_movable_roll(&mut tx, batch_id).await?;
        if allowed.is_some() && actor.role == "aparatchi" {
            let origin: Option<String> = sqlx::query_scalar("SELECT canonical_apparatus_id FROM mini_progress_batches WHERE batch_id=$1")
                .bind(batch_id).fetch_one(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
            super::paddon_movement::ensure_apparatus_access(actor, origin.as_deref().unwrap_or_default(), &target.apparatus_id, allowed)?;
        }
        // Verify the named source after taking the locks; stale requests may not
        // pull a roll out of a third pallet or attach a previously removed roll.
        let item_id = sqlx::query_scalar::<_, String>(
            "SELECT id FROM mini_paddon_items
             WHERE paddon_id=$1 AND progress_batch_id=$2 AND removed_at IS NULL FOR UPDATE",
        )
        .bind(&source.id)
        .bind(batch_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::PaddonItemNotAssigned)?;
        memberships.push(item_id);
    }
    for (batch_id, item_id) in batch_ids.iter().zip(memberships) {
        sqlx::query("UPDATE mini_paddon_items SET removed_at=now(), removed_by_ref=$2, removed_by_display_name=$3 WHERE id=$1")
            .bind(item_id).bind(actor.ref_.trim()).bind(actor.display_name.trim())
            .execute(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        sqlx::query("INSERT INTO mini_paddon_items(id,paddon_id,progress_batch_id,added_by_ref,added_by_display_name) VALUES($1,$2,$3,$4,$5)")
            .bind(super::paddon_helpers::new_item_id()).bind(&target.id).bind(batch_id)
            .bind(actor.ref_.trim()).bind(actor.display_name.trim())
            .execute(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    }
    sqlx::query("UPDATE mini_paddons SET updated_at=now() WHERE id=ANY($1)")
        .bind([&source.id, &target.id])
        .execute(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}
