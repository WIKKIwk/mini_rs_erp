use std::collections::BTreeMap;

use rusqlite::params;

use super::map_helpers::{
    put_map_inner, reject_duplicate_order_number, reject_order_number_immutable,
};
use super::{ProductionMapStore, unix_micros};
use crate::core::production_map::{ProductionMapDefinition, ProductionMapError};

pub(super) async fn maps(
    store: &ProductionMapStore,
) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut stmt = conn
        .prepare(
            "SELECT payload_json
             FROM production_maps
             ORDER BY saved_at DESC",
        )
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let rows = stmt
        .query_map([], |row| {
            let payload: String = row.get(0)?;
            let map = serde_json::from_str::<ProductionMapDefinition>(&payload)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
            Ok(map)
        })
        .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn put_map(
    store: &ProductionMapStore,
    map: ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    reject_order_number_immutable(&conn, &map)?;
    reject_duplicate_order_number(&conn, &map)?;
    put_map_inner(&conn, &map)
}

pub(super) async fn put_maps_batch(
    store: &ProductionMapStore,
    maps: &[ProductionMapDefinition],
) -> Result<(), ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let result = (|| {
        for map in maps {
            reject_order_number_immutable(&conn, map)?;
            reject_duplicate_order_number(&conn, map)?;
            put_map_inner(&conn, map)?;
        }
        Ok::<(), ProductionMapError>(())
    })();
    if result.is_ok() {
        conn.execute("COMMIT", [])
            .map_err(|_| ProductionMapError::StoreFailed)?;
    } else {
        let _ = conn.execute("ROLLBACK", []);
    }
    result
}

pub(super) async fn delete_map(
    store: &ProductionMapStore,
    map_id: &str,
) -> Result<(), ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    conn.execute(
        "DELETE FROM production_maps WHERE id = ?1",
        params![map_id.trim()],
    )
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn apparatus_sequences(
    store: &ProductionMapStore,
) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut stmt = conn
        .prepare("SELECT apparatus, order_ids_json FROM apparatus_sequences")
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let rows = stmt
        .query_map([], |row| {
            let apparatus: String = row.get(0)?;
            let payload: String = row.get(1)?;
            let order_ids = serde_json::from_str::<Vec<String>>(&payload)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
            Ok((apparatus, order_ids))
        })
        .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn put_apparatus_sequence(
    store: &ProductionMapStore,
    apparatus: &str,
    order_ids: Vec<String>,
) -> Result<(), ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let payload = serde_json::to_string(&order_ids).map_err(|_| ProductionMapError::StoreFailed)?;
    conn.execute(
        "INSERT INTO apparatus_sequences (apparatus, order_ids_json, saved_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(apparatus) DO UPDATE SET
            order_ids_json = excluded.order_ids_json,
            saved_at = excluded.saved_at",
        params![apparatus.trim(), payload, unix_micros().to_string()],
    )
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}
