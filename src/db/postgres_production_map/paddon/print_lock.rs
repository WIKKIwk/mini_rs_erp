use sqlx::PgPool;

use crate::core::production_map::{
    PaddonCreateInput, PaddonPrintConfirmation, PaddonSummary, ProductionMapError,
    QueueActionActor, paddon_unlock_actor_allowed,
};

#[derive(sqlx::FromRow)]
struct UnlockAccess {
    locked: bool,
    locked_by_ref: String,
    received: bool,
}

pub(super) async fn can_unlock(
    pool: &PgPool,
    code: &str,
    actor: &QueueActionActor,
) -> Result<bool, ProductionMapError> {
    let access = sqlx::query_as::<_, UnlockAccess>(
        "SELECT locked_at IS NOT NULL AS locked, locked_by_ref, receipt_json IS NOT NULL AS received FROM mini_paddons WHERE code=$1",
    ).bind(code).fetch_optional(pool).await.map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::PaddonNotFound)?;
    if !access.locked || access.received {
        return Ok(false);
    }
    let enabled = super::paddon_management::load(pool)
        .await?
        .free_movement_enabled;
    Ok(paddon_unlock_actor_allowed(
        &access.locked_by_ref,
        enabled,
        actor,
    ))
}

pub(super) async fn unlock(
    pool: &PgPool,
    code: &str,
    actor: &QueueActionActor,
) -> Result<PaddonSummary, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    // Use the membership-edit lock order; disabling the mode waits for this unlock.
    let enabled = super::paddon_management::lock_enabled(&mut tx).await?;
    let access = sqlx::query_as::<_, UnlockAccess>(
        "SELECT locked_at IS NOT NULL AS locked, locked_by_ref, receipt_json IS NOT NULL AS received FROM mini_paddons WHERE code=$1 FOR UPDATE",
    ).bind(code).fetch_optional(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::PaddonNotFound)?;
    if access.received {
        return Err(ProductionMapError::PaddonAlreadyReceived);
    }
    if access.locked {
        if !paddon_unlock_actor_allowed(&access.locked_by_ref, enabled, actor) {
            return Err(ProductionMapError::PaddonUnlockForbidden);
        }
        sqlx::query("UPDATE mini_paddons SET locked_at=NULL, locked_by_ref='', locked_by_display_name='', updated_at=now() WHERE code=$1")
            .bind(code).execute(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        // The next successful print starts a new successor-confirmation cycle.
        // Existing successor paddons and active selections are preserved.
        sqlx::query("DELETE FROM mini_paddon_print_successors WHERE source_code=$1")
            .bind(code)
            .execute(&mut *tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    super::paddon_helpers::load_paddon_summary(pool, code)
        .await?
        .ok_or(ProductionMapError::PaddonNotFound)
}

pub(super) async fn confirm(
    pool: &PgPool,
    code: &str,
    actor: &QueueActionActor,
) -> Result<PaddonPrintConfirmation, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    // Membership edits and cutting outputs take the same pallet row lock.
    let locked = sqlx::query_scalar::<_, bool>(
        "SELECT locked_at IS NOT NULL FROM mini_paddons WHERE code=$1 FOR UPDATE",
    )
    .bind(code)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::PaddonNotFound)?;
    let apparatuses = sqlx::query_scalar::<_, String>(
        "SELECT apparatus_id FROM mini_active_rezka_paddons WHERE paddon_code=$1 AND actor_role=$2 AND actor_ref=$3 ORDER BY apparatus_id",
    ).bind(code).bind(actor.role.trim()).bind(actor.ref_.trim()).fetch_all(&mut *tx).await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    if !locked {
        sqlx::query("UPDATE mini_paddons SET locked_at=now(), locked_by_ref=$2, locked_by_display_name=$3, updated_at=now() WHERE code=$1")
            .bind(code).bind(actor.ref_.trim()).bind(actor.display_name.trim())
            .execute(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    }
    // Clear every worker/device selection of the sealed physical package.
    sqlx::query("UPDATE mini_active_rezka_paddons SET paddon_code=NULL, updated_at=now() WHERE paddon_code=$1")
        .bind(code).execute(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let paddon = super::paddon_helpers::load_paddon_summary(pool, code)
        .await?
        .ok_or(ProductionMapError::PaddonNotFound)?;
    Ok(PaddonPrintConfirmation {
        paddon,
        newly_locked: !locked,
        apparatuses,
    })
}

pub(super) async fn successor(
    pool: &PgPool,
    source_code: &str,
    apparatus: &str,
    actor: &QueueActionActor,
) -> Result<PaddonSummary, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    super::active_paddon::lock_scope(&mut tx, apparatus, actor).await?;
    let locked = sqlx::query_scalar::<_, bool>(
        "SELECT locked_at IS NOT NULL FROM mini_paddons WHERE code=$1 FOR UPDATE",
    )
    .bind(source_code)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::PaddonNotFound)?;
    if !locked {
        return Err(ProductionMapError::PaddonInvalidInput);
    }
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT paddon_code FROM mini_paddon_print_successors WHERE source_code=$1 AND actor_role=$2 AND actor_ref=$3 AND apparatus_id=$4",
    ).bind(source_code).bind(actor.role.trim()).bind(actor.ref_.trim()).bind(apparatus)
        .fetch_optional(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let code = if let Some(code) = existing {
        let unavailable = sqlx::query_scalar::<_, bool>(
            "SELECT locked_at IS NOT NULL OR receipt_json IS NOT NULL FROM mini_paddons WHERE code=$1 FOR UPDATE",
        ).bind(&code).fetch_one(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if unavailable {
            return Err(ProductionMapError::PaddonLocked);
        }
        code
    } else {
        let code = super::paddon_helpers::create_paddon_tx(
            &mut tx,
            PaddonCreateInput {
                location: apparatus.to_string(),
                note: String::new(),
                actor_ref: actor.ref_.trim().to_string(),
                actor_display_name: actor.display_name.trim().to_string(),
            },
        )
        .await?;
        sqlx::query("INSERT INTO mini_paddon_print_successors(source_code, actor_role, actor_ref, apparatus_id, paddon_code) VALUES ($1,$2,$3,$4,$5)")
            .bind(source_code).bind(actor.role.trim()).bind(actor.ref_.trim()).bind(apparatus).bind(&code)
            .execute(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        code
    };
    sqlx::query("INSERT INTO mini_active_rezka_paddons(actor_role, actor_ref, apparatus_id, paddon_code) VALUES ($1,$2,$3,$4) ON CONFLICT(actor_role, actor_ref, apparatus_id) DO UPDATE SET paddon_code=EXCLUDED.paddon_code, updated_at=now()")
        .bind(actor.role.trim()).bind(actor.ref_.trim()).bind(apparatus).bind(&code)
        .execute(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    super::paddon_helpers::load_paddon_summary(pool, &code)
        .await?
        .ok_or(ProductionMapError::PaddonNotFound)
}
