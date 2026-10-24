use crate::core::qolip::normalize::qolip_location_id;
use crate::core::qolip::{QolipError, QolipProductSpec};
use sqlx::{Postgres, Transaction};

// Both code edits and product corrections must change the stocked identity together.
pub(super) async fn update_product_locations_tx(
    tx: &mut Transaction<'_, Postgres>,
    previous: &str,
    spec: &QolipProductSpec,
) -> Result<(), QolipError> {
    let locations = sqlx::query_as::<_, (String, String, String, Option<i32>, String, String)>(
        "SELECT id, block, row_letter, column_number, item_code, item_name
         FROM mini_qolip_locations
         WHERE lower(qolip_code) = $1
         FOR UPDATE",
    )
    .bind(previous)
    .fetch_all(&mut **tx)
    .await
    .map_err(super::reworks::storage_error)?;
    let location_updates = locations
        .into_iter()
        .map(|(old_id, block, row_letter, column_number, mut item_code, mut item_name)| {
            if !spec.item_code.is_empty() {
                item_code = spec.item_code.clone();
                item_name = spec.item_name.clone();
            }
            let new_id = qolip_location_id(
                &block,
                &item_code,
                &spec.qolip_code,
                spec.size,
                &row_letter,
                column_number,
            );
            (old_id, new_id, item_code, item_name)
        })
        .collect::<Vec<_>>();
    for (old_id, new_id, item_code, item_name) in &location_updates {
        if old_id != new_id {
            let id_conflict = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM mini_qolip_locations WHERE id = $1 AND id <> $2)",
            )
            .bind(new_id)
            .bind(old_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(super::reworks::storage_error)?;
            if id_conflict {
                return Err(QolipError::QolipCodeConflict);
            }
        }
        sqlx::query(
            "UPDATE mini_qolip_locations
             SET id = $2,
                 item_code = $3,
                 item_name = $4,
                 qolip_code = $5,
                 size = $6,
                 payload_json = jsonb_set(
                     jsonb_set(
                         jsonb_set(
                             jsonb_set(
                                 jsonb_set(COALESCE(payload_json, '{}'::jsonb), '{id}', to_jsonb($2::text), true),
                                 '{item_code}', to_jsonb($3::text), true
                             ),
                             '{item_name}', to_jsonb($4::text), true
                         ),
                         '{qolip_code}', to_jsonb($5::text), true
                     ),
                     '{size}', to_jsonb($6::integer), true
                 ),
                 updated_at = now()
             WHERE id = $1",
        )
        .bind(old_id)
        .bind(new_id)
        .bind(item_code.trim())
        .bind(item_name.trim())
        .bind(spec.qolip_code.trim())
        .bind(spec.size)
        .execute(&mut **tx)
        .await
        .map_err(super::reworks::storage_error)?;
    }
    Ok(())
}
