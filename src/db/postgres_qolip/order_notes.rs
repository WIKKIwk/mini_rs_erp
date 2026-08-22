use std::collections::BTreeSet;

use sqlx::PgPool;

use crate::core::auth::models::Principal;
use crate::core::qolip::normalize::role_code;
use crate::core::qolip::{QolipError, QolipOrderNote};

use super::rows::QolipOrderNoteRow;

pub(super) async fn load_order_notes(
    pool: &PgPool,
    principal: &Principal,
) -> Result<Vec<QolipOrderNote>, QolipError> {
    let rows = sqlx::query_as::<_, QolipOrderNoteRow>(
        r#"
        SELECT order_id, item_code, item_name, qolip_codes, status,
               to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"') AS updated_at
        FROM mini_qolip_order_notes
        WHERE principal_role = $1 AND principal_ref = $2
        ORDER BY updated_at DESC, order_id
        "#,
    )
    .bind(role_code(&principal.role))
    .bind(principal.ref_.trim())
    .fetch_all(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(rows.into_iter().map(row_to_order_note).collect())
}

pub(super) async fn load_order_note(
    pool: &PgPool,
    principal: &Principal,
    order_id: &str,
) -> Result<Option<QolipOrderNote>, QolipError> {
    let row = sqlx::query_as::<_, QolipOrderNoteRow>(
        r#"
        SELECT order_id, item_code, item_name, qolip_codes, status,
               to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"') AS updated_at
        FROM mini_qolip_order_notes
        WHERE principal_role = $1 AND principal_ref = $2 AND order_id = $3
        "#,
    )
    .bind(role_code(&principal.role))
    .bind(principal.ref_.trim())
    .bind(order_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(row.map(row_to_order_note))
}

pub(super) async fn load_order_note_qolip_codes_in_use(
    pool: &PgPool,
    principal: &Principal,
    order_id: &str,
) -> Result<Vec<String>, QolipError> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT DISTINCT lower(code.value)
        FROM mini_qolip_order_notes AS note
        CROSS JOIN LATERAL unnest(note.qolip_codes) AS code(value)
        WHERE lower(note.status) = 'given'
          AND NOT (
              note.order_id = $1
              AND note.principal_role = $2
              AND note.principal_ref = $3
          )
        ORDER BY lower(code.value)
        "#,
    )
    .bind(order_id.trim())
    .bind(role_code(&principal.role))
    .bind(principal.ref_.trim())
    .fetch_all(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)
}

pub(super) async fn save_order_note(
    pool: &PgPool,
    principal: &Principal,
    note: QolipOrderNote,
) -> Result<QolipOrderNote, QolipError> {
    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;
    let code_keys = note
        .qolip_codes
        .iter()
        .map(|code| code.trim().to_lowercase())
        .filter(|code| !code.is_empty())
        .collect::<BTreeSet<_>>();
    for code in &code_keys {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext(lower($1))::bigint)")
            .bind(code)
            .execute(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
    }
    if note.status.trim().eq_ignore_ascii_case("given") && !code_keys.is_empty() {
        let in_use = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM mini_qolip_order_notes AS existing
                CROSS JOIN LATERAL unnest(existing.qolip_codes) AS code(value)
                WHERE lower(existing.status) = 'given'
                  AND NOT (
                      existing.order_id = $1
                      AND existing.principal_role = $2
                      AND existing.principal_ref = $3
                  )
                  AND lower(code.value) = ANY($4)
            )
            "#,
        )
        .bind(note.order_id.trim())
        .bind(role_code(&principal.role))
        .bind(principal.ref_.trim())
        .bind(code_keys.iter().cloned().collect::<Vec<_>>())
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
        if in_use {
            return Err(QolipError::QolipInUse);
        }
    }
    let row = sqlx::query_as::<_, QolipOrderNoteRow>(
        r#"
        INSERT INTO mini_qolip_order_notes (
            order_id, principal_role, principal_ref, principal_name,
            item_code, item_name, qolip_codes, status
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (order_id, principal_role, principal_ref) DO UPDATE SET
            principal_name = EXCLUDED.principal_name,
            item_code = EXCLUDED.item_code,
            item_name = EXCLUDED.item_name,
            qolip_codes = EXCLUDED.qolip_codes,
            status = EXCLUDED.status,
            updated_at = now()
        RETURNING order_id, item_code, item_name, qolip_codes, status,
                  to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"') AS updated_at
        "#,
    )
    .bind(note.order_id.trim())
    .bind(role_code(&principal.role))
    .bind(principal.ref_.trim())
    .bind(principal.display_name.trim())
    .bind(note.item_code.trim())
    .bind(note.item_name.trim())
    .bind(note.qolip_codes)
    .bind(note.status.trim())
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;

    Ok(row_to_order_note(row))
}

fn row_to_order_note(row: QolipOrderNoteRow) -> QolipOrderNote {
    QolipOrderNote {
        order_id: row.order_id,
        item_code: row.item_code,
        item_name: row.item_name,
        qolip_codes: row.qolip_codes,
        status: row.status,
        updated_at: row.updated_at,
    }
}
