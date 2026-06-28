use sqlx::PgPool;

use crate::core::qolip::{QolipCellQr, QolipError};

use super::rows::{QolipCellQrRow, row_to_cell_qr};

pub(super) async fn save_cell_qr(
    pool: &PgPool,
    cell: QolipCellQr,
) -> Result<QolipCellQr, QolipError> {
    let row = sqlx::query_as::<_, QolipCellQrRow>(
        "INSERT INTO mini_qolip_cell_qrs (
             id, block, warehouse, row_letter, column_number, location_label,
             qr_payload, created_by_role, created_by_ref, created_by_name, payload_json
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         ON CONFLICT (id) DO UPDATE SET
             block = excluded.block,
             warehouse = excluded.warehouse,
             row_letter = excluded.row_letter,
             column_number = excluded.column_number,
             location_label = excluded.location_label,
             updated_at = now()
         RETURNING id, block, warehouse, row_letter, column_number, location_label,
             qr_payload, created_by_role, created_by_ref, created_by_name",
    )
    .bind(cell.id.trim())
    .bind(cell.block.trim())
    .bind(cell.warehouse.trim())
    .bind(cell.row_letter.trim())
    .bind(cell.column_number)
    .bind(cell.location_label.trim())
    .bind(cell.qr_payload.trim())
    .bind(cell.created_by_role.trim())
    .bind(cell.created_by_ref.trim())
    .bind(cell.created_by_name.trim())
    .bind(serde_json::to_value(&cell).map_err(|_| QolipError::StoreFailed)?)
    .fetch_one(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(row_to_cell_qr(row))
}

pub(super) async fn load_cell_qr_by_payload(
    pool: &PgPool,
    qr_payload: &str,
) -> Result<Option<QolipCellQr>, QolipError> {
    let row = sqlx::query_as::<_, QolipCellQrRow>(
        "SELECT id, block, warehouse, row_letter, column_number, location_label,
                qr_payload, created_by_role, created_by_ref, created_by_name
         FROM mini_qolip_cell_qrs
         WHERE lower(qr_payload) = lower($1)",
    )
    .bind(qr_payload.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(row.map(row_to_cell_qr))
}
