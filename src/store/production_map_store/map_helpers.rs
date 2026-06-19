use rusqlite::{Connection, OptionalExtension, params};

use crate::core::production_map::{ProductionMapDefinition, ProductionMapError};

use super::unix_micros;

pub(super) fn put_map_inner(
    conn: &Connection,
    map: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let payload = serde_json::to_string(map).map_err(|_| ProductionMapError::StoreFailed)?;
    conn.execute(
        "INSERT INTO production_maps
            (id, product_code, title, saved_at, payload_json)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
            product_code = excluded.product_code,
            title = excluded.title,
            saved_at = excluded.saved_at,
            payload_json = excluded.payload_json",
        params![
            map.id.trim(),
            map.product_code.trim(),
            map.title.trim(),
            unix_micros().to_string(),
            payload
        ],
    )
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) fn reject_order_number_immutable(
    conn: &Connection,
    map: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let id = map.id.trim();
    if !id.starts_with("zakaz-") {
        return Ok(());
    }
    let order_number = map.order_number.trim();
    if order_number.is_empty() {
        return Ok(());
    }
    let existing = conn
        .query_row(
            "SELECT payload_json FROM production_maps WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let Some(payload) = existing else {
        return Ok(());
    };
    let existing_map = serde_json::from_str::<ProductionMapDefinition>(&payload)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let existing_number = existing_map.order_number.trim();
    if !existing_number.is_empty() && existing_number != order_number {
        return Err(ProductionMapError::OrderNumberImmutable);
    }
    Ok(())
}

pub(super) fn reject_duplicate_order_number(
    conn: &Connection,
    map: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let order_number = map.order_number.trim();
    if order_number.is_empty() {
        return Ok(());
    }
    let mut stmt = conn
        .prepare("SELECT payload_json FROM production_maps")
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|_| ProductionMapError::StoreFailed)?;
    for row in rows {
        let payload = row.map_err(|_| ProductionMapError::StoreFailed)?;
        let existing = serde_json::from_str::<ProductionMapDefinition>(&payload)
            .map_err(|_| ProductionMapError::StoreFailed)?;
        if existing.order_number.trim() == order_number && !is_same_zakaz(&existing, map) {
            return Err(ProductionMapError::DuplicateOrderNumber);
        }
    }
    Ok(())
}

fn is_same_zakaz(existing: &ProductionMapDefinition, next: &ProductionMapDefinition) -> bool {
    existing.id.trim() == next.id.trim()
        && existing.title.trim() == next.title.trim()
        && existing.product_code.trim() == next.product_code.trim()
}
