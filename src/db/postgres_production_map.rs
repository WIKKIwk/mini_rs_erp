use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{
    ApparatusQueueActionEvent, ApparatusQueuePolicy, ProductionMapDefinition, ProductionMapError,
    ProductionMapStorePort, QueueActionActor,
};

#[derive(Clone)]
pub struct PostgresProductionMapStore {
    pool: PgPool,
}

impl PostgresProductionMapStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProductionMapStorePort for PostgresProductionMapStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        let rows = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT map_json
             FROM mini_production_maps
             ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;

        rows.into_iter()
            .map(|payload| {
                serde_json::from_value::<ProductionMapDefinition>(payload)
                    .map_err(|_| ProductionMapError::StoreFailed)
            })
            .collect()
    }

    async fn put_map(&self, map: ProductionMapDefinition) -> Result<(), ProductionMapError> {
        reject_order_number_immutable(&self.pool, &map).await?;
        reject_duplicate_order_number(&self.pool, &map).await?;
        put_map_inner(&self.pool, &map).await
    }

    async fn put_maps_batch(
        &self,
        maps: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        for map in maps {
            reject_order_number_immutable_tx(&mut tx, map).await?;
            reject_duplicate_order_number_tx(&mut tx, map).await?;
            put_map_inner_tx(&mut tx, map).await?;
        }
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn delete_map(&self, map_id: &str) -> Result<(), ProductionMapError> {
        sqlx::query("DELETE FROM mini_production_maps WHERE id = $1")
            .bind(map_id.trim())
            .execute(&self.pool)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(())
    }

    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
            "SELECT apparatus, order_ids
             FROM mini_queue_sequences
             ORDER BY apparatus ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;

        rows.into_iter()
            .map(|(apparatus, payload)| {
                let order_ids = serde_json::from_value::<Vec<String>>(payload)
                    .map_err(|_| ProductionMapError::StoreFailed)?;
                Ok((apparatus, order_ids))
            })
            .collect()
    }

    async fn put_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        let order_ids = order_ids
            .into_iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        let payload =
            serde_json::to_value(order_ids).map_err(|_| ProductionMapError::StoreFailed)?;
        sqlx::query(
            "INSERT INTO mini_queue_sequences (apparatus, order_ids, updated_at)
             VALUES ($1, $2, now())
             ON CONFLICT (apparatus) DO UPDATE SET
               order_ids = excluded.order_ids,
               updated_at = excluded.updated_at",
        )
        .bind(apparatus.trim())
        .bind(payload)
        .execute(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(())
    }

    async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT apparatus, order_id, state
             FROM mini_queue_states
             ORDER BY apparatus ASC, order_id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;

        let mut grouped = BTreeMap::<String, BTreeMap<String, String>>::new();
        for (apparatus, order_id, state) in rows {
            grouped
                .entry(apparatus)
                .or_default()
                .insert(order_id, state);
        }
        Ok(grouped)
    }

    async fn put_apparatus_queue_states(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
    ) -> Result<(), ProductionMapError> {
        let apparatus = apparatus.trim();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        put_queue_states_tx(&mut tx, apparatus, states).await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn apparatus_queue_policies(
        &self,
    ) -> Result<BTreeMap<String, ApparatusQueuePolicy>, ProductionMapError> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT apparatus, policy
             FROM mini_apparatus_queue_policies
             ORDER BY apparatus ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;

        rows.into_iter()
            .map(|(apparatus, policy)| {
                let policy =
                    ApparatusQueuePolicy::parse(&policy).ok_or(ProductionMapError::StoreFailed)?;
                Ok((apparatus, policy))
            })
            .collect()
    }

    async fn put_apparatus_queue_policy(
        &self,
        apparatus: &str,
        policy: ApparatusQueuePolicy,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        let payload = serde_json::json!({
            "actor": actor,
            "policy": policy.as_str(),
        });
        sqlx::query(
            "INSERT INTO mini_apparatus_queue_policies
                (apparatus, policy, actor_role, actor_ref, actor_display_name, payload_json, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())
             ON CONFLICT (apparatus) DO UPDATE SET
               policy = excluded.policy,
               actor_role = excluded.actor_role,
               actor_ref = excluded.actor_ref,
               actor_display_name = excluded.actor_display_name,
               payload_json = excluded.payload_json,
               updated_at = excluded.updated_at",
        )
        .bind(apparatus.trim())
        .bind(policy.as_str())
        .bind(actor.role.trim())
        .bind(actor.ref_.trim())
        .bind(actor.display_name.trim())
        .bind(payload)
        .execute(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(())
    }

    async fn put_apparatus_queue_states_with_event(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        let apparatus = apparatus.trim();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        put_queue_states_tx(&mut tx, apparatus, states).await?;
        insert_queue_action_event_tx(&mut tx, &event).await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn append_apparatus_queue_action_event(
        &self,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        insert_queue_action_event_tx(&mut tx, &event).await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }
}

async fn put_queue_states_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
    states: BTreeMap<String, String>,
) -> Result<(), ProductionMapError> {
    sqlx::query("DELETE FROM mini_queue_states WHERE apparatus = $1")
        .bind(apparatus)
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    for (order_id, state) in states {
        sqlx::query(
            "INSERT INTO mini_queue_states (apparatus, order_id, state, updated_at)
             VALUES ($1, $2, $3, now())",
        )
        .bind(apparatus)
        .bind(order_id.trim())
        .bind(state.trim())
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(())
}

async fn insert_queue_action_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    event: &ApparatusQueueActionEvent,
) -> Result<(), ProductionMapError> {
    sqlx::query(
        "INSERT INTO mini_queue_action_events
            (event_id, apparatus, order_id, action, from_state, to_state, policy,
             actor_role, actor_ref, actor_display_name, assigned_apparatus, payload_json, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now())",
    )
    .bind(event.event_id.trim())
    .bind(event.apparatus.trim())
    .bind(event.order_id.trim())
    .bind(match event.action {
        crate::core::production_map::queue_state::ApparatusQueueAction::Start => "start",
        crate::core::production_map::queue_state::ApparatusQueueAction::Complete => "complete",
    })
    .bind(event.from_state.as_str())
    .bind(event.to_state.as_str())
    .bind(event.policy.as_str())
    .bind(event.actor.role.trim())
    .bind(event.actor.ref_.trim())
    .bind(event.actor.display_name.trim())
    .bind(
        serde_json::to_value(&event.assigned_apparatus)
            .map_err(|_| ProductionMapError::StoreFailed)?,
    )
    .bind(&event.payload_json)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

async fn put_map_inner(
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

async fn put_map_inner_tx(
    tx: &mut Transaction<'_, Postgres>,
    map: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let payload = serde_json::to_value(map).map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO mini_production_maps
            (id, product_code, title, code, order_number, roll_count, width_mm, map_json, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
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
    .bind(map.roll_count)
    .bind(map.width_mm)
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

fn node_kind(kind: &crate::core::production_map::ProductionMapNodeKind) -> &'static str {
    match kind {
        crate::core::production_map::ProductionMapNodeKind::Start => "start",
        crate::core::production_map::ProductionMapNodeKind::Location => "location",
        crate::core::production_map::ProductionMapNodeKind::Material => "material",
        crate::core::production_map::ProductionMapNodeKind::Apparatus => "apparatus",
        crate::core::production_map::ProductionMapNodeKind::KkProduct => "kk_product",
        crate::core::production_map::ProductionMapNodeKind::Formula => "formula",
        crate::core::production_map::ProductionMapNodeKind::Condition => "condition",
        crate::core::production_map::ProductionMapNodeKind::Task => "task",
        crate::core::production_map::ProductionMapNodeKind::Wait => "wait",
        crate::core::production_map::ProductionMapNodeKind::Output => "output",
        crate::core::production_map::ProductionMapNodeKind::End => "end",
    }
}

async fn reject_order_number_immutable(
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

async fn reject_order_number_immutable_tx(
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

async fn reject_duplicate_order_number(
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

async fn reject_duplicate_order_number_tx(
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
