use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapError, ProductionMapNodeKind,
};
use crate::core::quantity::positive_erp_quantity;

pub(super) async fn put_map_inner(
    pool: &PgPool,
    map: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    put_map_inner_tx(&mut tx, map).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn put_map_inner_tx(
    tx: &mut Transaction<'_, Postgres>,
    map: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let mut stored_map = map.clone();
    stored_map.roll_count = map.roll_count.and_then(positive_erp_quantity);
    stored_map.width_mm = map.width_mm.and_then(positive_erp_quantity);
    let payload = serde_json::to_value(&stored_map).map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO mini_production_maps
            (id, product_code, title, code, order_number, roll_count, width_mm, map_json, updated_at)
         VALUES ($1, $2, $3, $4, $5,
                 ($6::double precision)::numeric(24,9),
                 ($7::double precision)::numeric(24,9), $8, now())
         ON CONFLICT (id) DO UPDATE SET
            product_code = excluded.product_code,
            title = excluded.title,
            code = excluded.code,
            order_number = excluded.order_number,
            roll_count = excluded.roll_count,
            width_mm = excluded.width_mm,
            map_json = excluded.map_json,
            updated_at = excluded.updated_at",
    )
    .bind(map.id.trim())
    .bind(map.product_code.trim())
    .bind(map.title.trim())
    .bind(map.code.trim())
    .bind(map.order_number.trim())
    .bind(stored_map.roll_count)
    .bind(stored_map.width_mm)
    .bind(payload)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    mirror_map_graph_tx(tx, map).await?;
    Ok(())
}

async fn mirror_map_graph_tx(
    tx: &mut Transaction<'_, Postgres>,
    map: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let map_id = map.id.trim();
    sqlx::query("DELETE FROM mini_production_map_edges WHERE map_id = $1")
        .bind(map_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query("DELETE FROM mini_production_map_nodes WHERE map_id = $1")
        .bind(map_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;

    for node in &map.nodes {
        let payload = serde_json::to_value(node).map_err(|_| ProductionMapError::StoreFailed)?;
        sqlx::query(
            "INSERT INTO mini_production_map_nodes
                (map_id, node_id, kind, title, payload_json)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(map_id)
        .bind(node.id.trim())
        .bind(node_kind(&node.kind))
        .bind(node.title.trim())
        .bind(payload)
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    }

    for (index, edge) in map.edges.iter().enumerate() {
        let payload = serde_json::to_value(edge).map_err(|_| ProductionMapError::StoreFailed)?;
        sqlx::query(
            "INSERT INTO mini_production_map_edges
                (map_id, edge_index, from_node_id, to_node_id, branch, payload_json)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(map_id)
        .bind(index as i32)
        .bind(edge.from.trim())
        .bind(edge.to.trim())
        .bind(edge.branch.trim())
        .bind(payload)
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(())
}

fn node_kind(kind: &ProductionMapNodeKind) -> &'static str {
    match kind {
        ProductionMapNodeKind::Start => "start",
        ProductionMapNodeKind::Location => "location",
        ProductionMapNodeKind::Material => "material",
        ProductionMapNodeKind::Apparatus => "apparatus",
        ProductionMapNodeKind::KkProduct => "kk_product",
        ProductionMapNodeKind::Formula => "formula",
        ProductionMapNodeKind::Condition => "condition",
        ProductionMapNodeKind::Task => "task",
        ProductionMapNodeKind::Wait => "wait",
        ProductionMapNodeKind::Output => "output",
        ProductionMapNodeKind::End => "end",
    }
}

pub(super) async fn reject_order_number_immutable(
    pool: &PgPool,
    map: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let result = reject_order_number_immutable_tx(&mut tx, map).await;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    result
}

pub(super) async fn reject_order_number_immutable_tx(
    tx: &mut Transaction<'_, Postgres>,
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
    let existing = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT map_json FROM mini_production_maps WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let Some(payload) = existing else {
        return Ok(());
    };
    let existing_map = serde_json::from_value::<ProductionMapDefinition>(payload)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let existing_number = existing_map.order_number.trim();
    if !existing_number.is_empty() && existing_number != order_number {
        return Err(ProductionMapError::OrderNumberImmutable);
    }
    Ok(())
}

pub(super) async fn reject_duplicate_order_number(
    pool: &PgPool,
    map: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let result = reject_duplicate_order_number_tx(&mut tx, map).await;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    result
}

pub(super) async fn reject_duplicate_order_number_tx(
    tx: &mut Transaction<'_, Postgres>,
    map: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let order_number = map.order_number.trim();
    if order_number.is_empty() {
        return Ok(());
    }
    let rows = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT map_json
         FROM mini_production_maps
         WHERE order_number = $1",
    )
    .bind(order_number)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    for payload in rows {
        let existing = serde_json::from_value::<ProductionMapDefinition>(payload)
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
