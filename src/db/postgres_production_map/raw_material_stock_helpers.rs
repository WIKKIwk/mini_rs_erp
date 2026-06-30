use std::collections::BTreeSet;

use sqlx::{Postgres, Transaction};

use crate::core::production_map::{
    ProductionMapError, RawMaterialStockTransition, RawMaterialStockTransitionKind,
};

pub(super) async fn apply_raw_material_stock_transitions_tx(
    tx: &mut Transaction<'_, Postgres>,
    transitions: &[RawMaterialStockTransition],
) -> Result<Vec<String>, ProductionMapError> {
    let mut warehouses = BTreeSet::new();
    for transition in transitions {
        if transition.is_empty() {
            continue;
        }
        let barcodes = normalized_barcodes(&transition.barcodes);
        if barcodes.is_empty() || transition.order_id.trim().is_empty() {
            continue;
        }
        let rows = match transition.kind {
            RawMaterialStockTransitionKind::InUse => {
                mark_raw_material_stock_in_use_tx(tx, &barcodes, &transition.order_id).await
            }
            RawMaterialStockTransitionKind::Consumed => {
                mark_raw_material_stock_consumed_tx(tx, &barcodes, &transition.order_id).await
            }
        }
        .map_err(|error| {
            tracing::error!(
                error = %error,
                order_id = %transition.order_id,
                "failed to update raw material stock inside queue action transaction"
            );
            ProductionMapError::StoreFailed
        })?;
        if rows.len() != barcodes.len() {
            return Err(ProductionMapError::RawMaterialStockUnavailable);
        }
        warehouses.extend(
            rows.into_iter()
                .map(|warehouse| warehouse.trim().to_string())
                .filter(|warehouse| !warehouse.is_empty()),
        );
    }
    Ok(warehouses.into_iter().collect())
}

async fn mark_raw_material_stock_in_use_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
    order_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "UPDATE mini_raw_material_stock
         SET status = 'in_use',
             reserved_order_id = $2,
             payload_json = jsonb_set(payload_json, '{in_use_order_id}', to_jsonb($2::text), true),
             updated_at = now()
         WHERE lower(barcode) = ANY($1)
           AND (status = 'available' OR (status = 'in_use' AND reserved_order_id = $2))
         RETURNING warehouse",
    )
    .bind(barcodes)
    .bind(order_id.trim())
    .fetch_all(&mut **tx)
    .await
}

async fn mark_raw_material_stock_consumed_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
    order_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "UPDATE mini_raw_material_stock
         SET status = 'consumed',
             payload_json = jsonb_set(payload_json, '{consumed_order_id}', to_jsonb($2::text), true),
             updated_at = now()
         WHERE lower(barcode) = ANY($1)
           AND reserved_order_id = $2
           AND status IN ('in_use', 'consumed')
         RETURNING warehouse",
    )
    .bind(barcodes)
    .bind(order_id.trim())
    .fetch_all(&mut **tx)
    .await
}

fn normalized_barcodes(barcodes: &[String]) -> Vec<String> {
    barcodes
        .iter()
        .map(|barcode| barcode.trim().to_ascii_lowercase())
        .filter(|barcode| !barcode.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
