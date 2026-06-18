use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{
    ApparatusMaterialRule, ApparatusQueueActionEvent, ApparatusQueuePolicy, CompletedQueueOrder,
    OrderProgressBatch, OrderProgressBatchStatus, OrderProgressEvent, OrderRunSession,
    OrderRunStatus, ProductionMapDefinition, ProductionMapError, ProductionMapStorePort,
    QueueActionActor, RawMaterialAssignment,
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

    async fn completed_queue_orders_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
        let actor_ref = actor_ref.trim();
        if actor_ref.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit.min(500)).unwrap_or(500);
        let rows = sqlx::query_as::<_, (String, String, i64)>(
            "SELECT order_id, apparatus, completed_at_unix
             FROM (
                SELECT DISTINCT ON (order_id)
                    order_id,
                    apparatus,
                    created_at,
                    EXTRACT(EPOCH FROM created_at)::bigint AS completed_at_unix
                FROM mini_queue_action_events
                WHERE actor_ref = $1
                  AND action = 'complete'
                  AND to_state = 'completed'
                ORDER BY order_id, created_at DESC
             ) latest
             ORDER BY created_at DESC
             LIMIT $2",
        )
        .bind(actor_ref)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;

        Ok(rows
            .into_iter()
            .map(
                |(order_id, apparatus, completed_at_unix)| CompletedQueueOrder {
                    apparatus,
                    order_id,
                    completed_at_unix,
                },
            )
            .collect())
    }

    async fn active_order_run_session(
        &self,
        apparatus: &str,
        order_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        let row = sqlx::query_as::<_, ProgressSessionRow>(
            "SELECT session_id, apparatus, order_id, status,
                    worker_role, worker_ref, worker_display_name,
                    EXTRACT(EPOCH FROM started_at)::bigint AS started_at_unix,
                    EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at_unix,
                    payload_json
             FROM mini_order_run_sessions
             WHERE order_id = $1
               AND lower(apparatus) = lower($2)
               AND status IN ('active', 'paused')
             ORDER BY updated_at DESC
             LIMIT 1",
        )
        .bind(order_id.trim())
        .bind(apparatus.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        row.map(progress_session_from_row).transpose()
    }

    async fn order_run_session(
        &self,
        session_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        let row = sqlx::query_as::<_, ProgressSessionRow>(
            "SELECT session_id, apparatus, order_id, status,
                    worker_role, worker_ref, worker_display_name,
                    EXTRACT(EPOCH FROM started_at)::bigint AS started_at_unix,
                    EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at_unix,
                    payload_json
             FROM mini_order_run_sessions
             WHERE session_id = $1",
        )
        .bind(session_id.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        row.map(progress_session_from_row).transpose()
    }

    async fn progress_batch(
        &self,
        batch_id: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        let row = sqlx::query_as::<_, ProgressBatchRow>(
            "SELECT batch_id, session_id, apparatus, order_id, action, status,
                    produced_qty::float8 AS produced_qty, uom, qr_payload,
                    label_item_code, label_item_name, executor_name,
                    worker_role, worker_ref, worker_display_name, payload_json
             FROM mini_progress_batches
             WHERE batch_id = $1",
        )
        .bind(batch_id.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        row.map(progress_batch_from_row).transpose()
    }

    async fn progress_batch_by_qr(
        &self,
        qr_payload: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        let row = sqlx::query_as::<_, ProgressBatchRow>(
            "SELECT batch_id, session_id, apparatus, order_id, action, status,
                    produced_qty::float8 AS produced_qty, uom, qr_payload,
                    label_item_code, label_item_name, executor_name,
                    worker_role, worker_ref, worker_display_name, payload_json
             FROM mini_progress_batches
             WHERE lower(qr_payload) = lower($1)",
        )
        .bind(qr_payload.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        row.map(progress_batch_from_row).transpose()
    }

    async fn put_order_run_session(
        &self,
        session: OrderRunSession,
    ) -> Result<(), ProductionMapError> {
        put_order_run_session(&self.pool, &session).await
    }

    async fn put_order_progress_event(
        &self,
        event: OrderProgressEvent,
    ) -> Result<(), ProductionMapError> {
        put_order_progress_event(&self.pool, &event).await
    }

    async fn put_order_progress_batch(
        &self,
        batch: OrderProgressBatch,
    ) -> Result<(), ProductionMapError> {
        put_order_progress_batch(&self.pool, &batch).await
    }

    async fn put_apparatus_queue_states_with_event_and_progress(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
        event: ApparatusQueueActionEvent,
        session: Option<OrderRunSession>,
        progress_event: Option<OrderProgressEvent>,
        progress_batch: Option<OrderProgressBatch>,
    ) -> Result<(), ProductionMapError> {
        let apparatus = apparatus.trim();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        put_queue_states_tx(&mut tx, apparatus, states).await?;
        insert_queue_action_event_tx(&mut tx, &event).await?;
        if let Some(session) = session {
            put_order_run_session_tx(&mut tx, &session).await?;
        }
        if let Some(event) = progress_event {
            put_order_progress_event_tx(&mut tx, &event).await?;
        }
        if let Some(batch) = progress_batch {
            put_order_progress_batch_tx(&mut tx, &batch).await?;
        }
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn apparatus_material_rules(
        &self,
    ) -> Result<Vec<ApparatusMaterialRule>, ProductionMapError> {
        let rows = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json
             FROM mini_apparatus_material_rules
             ORDER BY lower(apparatus) ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;

        rows.into_iter()
            .map(|payload| {
                serde_json::from_value::<ApparatusMaterialRule>(payload)
                    .map_err(|_| ProductionMapError::StoreFailed)
            })
            .collect()
    }

    async fn put_apparatus_material_rule(
        &self,
        rule: ApparatusMaterialRule,
    ) -> Result<(), ProductionMapError> {
        let item_groups =
            serde_json::to_value(&rule.item_groups).map_err(|_| ProductionMapError::StoreFailed)?;
        let payload = serde_json::to_value(&rule).map_err(|_| ProductionMapError::StoreFailed)?;
        sqlx::query(
            "INSERT INTO mini_apparatus_material_rules
                (apparatus, item_groups, requires_material, payload_json, updated_at)
             VALUES ($1, $2, $3, $4, now())
             ON CONFLICT (apparatus) DO UPDATE SET
               item_groups = excluded.item_groups,
               requires_material = excluded.requires_material,
               payload_json = excluded.payload_json,
               updated_at = excluded.updated_at",
        )
        .bind(rule.apparatus.trim())
        .bind(item_groups)
        .bind(rule.requires_material)
        .bind(payload)
        .execute(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(())
    }

    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        let rows = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json
             FROM mini_raw_material_assignments
             ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;

        rows.into_iter()
            .map(|payload| {
                serde_json::from_value::<RawMaterialAssignment>(payload)
                    .map_err(|_| ProductionMapError::StoreFailed)
            })
            .collect()
    }

    async fn put_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        let payload =
            serde_json::to_value(&assignment).map_err(|_| ProductionMapError::StoreFailed)?;
        sqlx::query(
            "INSERT INTO mini_raw_material_assignments
                (barcode, order_id, apparatus, item_code, item_group, payload_json, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())",
        )
        .bind(assignment.barcode.trim())
        .bind(assignment.order_id.trim())
        .bind(assignment.apparatus.trim())
        .bind(assignment.item_code.trim())
        .bind(assignment.item_group.trim())
        .bind(payload)
        .execute(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(())
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
        crate::core::production_map::queue_state::ApparatusQueueAction::Pause => "pause",
        crate::core::production_map::queue_state::ApparatusQueueAction::Resume => "resume",
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

async fn put_order_run_session(
    pool: &PgPool,
    session: &OrderRunSession,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    put_order_run_session_tx(&mut tx, session).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

async fn put_order_run_session_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
) -> Result<(), ProductionMapError> {
    sqlx::query(
        "INSERT INTO mini_order_run_sessions (
            session_id, apparatus, order_id, status,
            worker_role, worker_ref, worker_display_name,
            started_at, updated_at, payload_json
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, to_timestamp($8), to_timestamp($9), $10)
         ON CONFLICT (session_id) DO UPDATE SET
            status = excluded.status,
            worker_role = excluded.worker_role,
            worker_ref = excluded.worker_ref,
            worker_display_name = excluded.worker_display_name,
            updated_at = excluded.updated_at,
            payload_json = excluded.payload_json",
    )
    .bind(session.session_id.trim())
    .bind(session.apparatus.trim())
    .bind(session.order_id.trim())
    .bind(session.status.as_str())
    .bind(session.worker_role.trim())
    .bind(session.worker_ref.trim())
    .bind(session.worker_display_name.trim())
    .bind(session.started_at_unix as f64)
    .bind(session.updated_at_unix as f64)
    .bind(&session.payload_json)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

async fn put_order_progress_event(
    pool: &PgPool,
    event: &OrderProgressEvent,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    put_order_progress_event_tx(&mut tx, event).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

async fn put_order_progress_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    event: &OrderProgressEvent,
) -> Result<(), ProductionMapError> {
    sqlx::query(
        "INSERT INTO mini_order_progress_events (
            event_id, session_id, batch_id, apparatus, order_id, action,
            produced_qty, uom, worker_role, worker_ref, worker_display_name,
            qr_payload, payload_json, created_at
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, now())
         ON CONFLICT (event_id) DO UPDATE SET
            session_id = excluded.session_id,
            batch_id = excluded.batch_id,
            action = excluded.action,
            produced_qty = excluded.produced_qty,
            uom = excluded.uom,
            worker_role = excluded.worker_role,
            worker_ref = excluded.worker_ref,
            worker_display_name = excluded.worker_display_name,
            qr_payload = excluded.qr_payload,
            payload_json = excluded.payload_json",
    )
    .bind(event.event_id.trim())
    .bind(event.session_id.trim())
    .bind(event.batch_id.trim())
    .bind(event.apparatus.trim())
    .bind(event.order_id.trim())
    .bind(queue_action_as_str(event.action))
    .bind(event.produced_qty)
    .bind(event.uom.trim())
    .bind(event.worker_role.trim())
    .bind(event.worker_ref.trim())
    .bind(event.worker_display_name.trim())
    .bind(event.qr_payload.trim())
    .bind(&event.payload_json)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

async fn put_order_progress_batch(
    pool: &PgPool,
    batch: &OrderProgressBatch,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    put_order_progress_batch_tx(&mut tx, batch).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

async fn put_order_progress_batch_tx(
    tx: &mut Transaction<'_, Postgres>,
    batch: &OrderProgressBatch,
) -> Result<(), ProductionMapError> {
    sqlx::query(
        "INSERT INTO mini_progress_batches (
            batch_id, session_id, apparatus, order_id, action, status,
            produced_qty, uom, qr_payload, label_item_code, label_item_name,
            executor_name, worker_role, worker_ref, worker_display_name,
            payload_json, created_at, updated_at
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, now(), now())
         ON CONFLICT (batch_id) DO UPDATE SET
            status = excluded.status,
            produced_qty = excluded.produced_qty,
            uom = excluded.uom,
            qr_payload = excluded.qr_payload,
            label_item_code = excluded.label_item_code,
            label_item_name = excluded.label_item_name,
            executor_name = excluded.executor_name,
            worker_role = excluded.worker_role,
            worker_ref = excluded.worker_ref,
            worker_display_name = excluded.worker_display_name,
            payload_json = excluded.payload_json,
            updated_at = now()",
    )
    .bind(batch.batch_id.trim())
    .bind(batch.session_id.trim())
    .bind(batch.apparatus.trim())
    .bind(batch.order_id.trim())
    .bind(queue_action_as_str(batch.action))
    .bind(batch.status.as_str())
    .bind(batch.produced_qty)
    .bind(batch.uom.trim())
    .bind(batch.qr_payload.trim())
    .bind(batch.label_item_code.trim())
    .bind(batch.label_item_name.trim())
    .bind(batch.executor_name.trim())
    .bind(batch.worker_role.trim())
    .bind(batch.worker_ref.trim())
    .bind(batch.worker_display_name.trim())
    .bind(&batch.payload_json)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ProgressSessionRow {
    session_id: String,
    apparatus: String,
    order_id: String,
    status: String,
    worker_role: String,
    worker_ref: String,
    worker_display_name: String,
    started_at_unix: i64,
    updated_at_unix: i64,
    payload_json: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct ProgressBatchRow {
    batch_id: String,
    session_id: String,
    apparatus: String,
    order_id: String,
    action: String,
    status: String,
    produced_qty: f64,
    uom: String,
    qr_payload: String,
    label_item_code: String,
    label_item_name: String,
    executor_name: String,
    worker_role: String,
    worker_ref: String,
    worker_display_name: String,
    payload_json: serde_json::Value,
}

fn progress_session_from_row(
    row: ProgressSessionRow,
) -> Result<OrderRunSession, ProductionMapError> {
    Ok(OrderRunSession {
        session_id: row.session_id,
        apparatus: row.apparatus,
        order_id: row.order_id,
        status: OrderRunStatus::parse(&row.status).ok_or(ProductionMapError::StoreFailed)?,
        worker_role: row.worker_role,
        worker_ref: row.worker_ref,
        worker_display_name: row.worker_display_name,
        started_at_unix: row.started_at_unix,
        updated_at_unix: row.updated_at_unix,
        payload_json: row.payload_json,
    })
}

fn progress_batch_from_row(
    row: ProgressBatchRow,
) -> Result<OrderProgressBatch, ProductionMapError> {
    Ok(OrderProgressBatch {
        batch_id: row.batch_id,
        session_id: row.session_id,
        apparatus: row.apparatus,
        order_id: row.order_id,
        action: queue_action_from_str(&row.action).ok_or(ProductionMapError::StoreFailed)?,
        status: OrderProgressBatchStatus::parse(&row.status)
            .ok_or(ProductionMapError::StoreFailed)?,
        produced_qty: row.produced_qty,
        uom: row.uom,
        qr_payload: row.qr_payload,
        label_item_code: row.label_item_code,
        label_item_name: row.label_item_name,
        executor_name: row.executor_name,
        worker_role: row.worker_role,
        worker_ref: row.worker_ref,
        worker_display_name: row.worker_display_name,
        payload_json: row.payload_json,
    })
}

fn queue_action_from_str(
    value: &str,
) -> Option<crate::core::production_map::queue_state::ApparatusQueueAction> {
    match value.trim().to_ascii_lowercase().as_str() {
        "start" => Some(crate::core::production_map::queue_state::ApparatusQueueAction::Start),
        "pause" => Some(crate::core::production_map::queue_state::ApparatusQueueAction::Pause),
        "resume" => Some(crate::core::production_map::queue_state::ApparatusQueueAction::Resume),
        "complete" => {
            Some(crate::core::production_map::queue_state::ApparatusQueueAction::Complete)
        }
        _ => None,
    }
}

fn queue_action_as_str(
    action: crate::core::production_map::queue_state::ApparatusQueueAction,
) -> &'static str {
    match action {
        crate::core::production_map::queue_state::ApparatusQueueAction::Start => "start",
        crate::core::production_map::queue_state::ApparatusQueueAction::Pause => "pause",
        crate::core::production_map::queue_state::ApparatusQueueAction::Resume => "resume",
        crate::core::production_map::queue_state::ApparatusQueueAction::Complete => "complete",
    }
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
