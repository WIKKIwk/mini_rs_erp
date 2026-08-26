use std::collections::BTreeMap;

use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapError, ProductionOrderLifecycleRecord,
    ProductionOrderLifecycleStatus, ProductionOrderOperationalStatus, QueueActionActor,
    derive_production_order_lifecycle, derive_production_order_operational_status,
};

pub(super) async fn load_production_order_lifecycles(
    pool: &PgPool,
    order_ids: &[String],
) -> Result<BTreeMap<String, ProductionOrderLifecycleRecord>, ProductionMapError> {
    let order_ids = order_ids
        .iter()
        .map(|order_id| order_id.trim().to_string())
        .filter(|order_id| !order_id.is_empty())
        .collect::<Vec<_>>();
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            i64,
            Option<i64>,
            Option<i64>,
            i64,
            String,
            i64,
            i64,
        ),
    >(
        "SELECT id,
                lifecycle_status,
                completion_outcome,
                extract(epoch FROM lifecycle_changed_at)::BIGINT,
                extract(epoch FROM production_completed_at)::BIGINT,
                extract(epoch FROM closed_at)::BIGINT,
                lifecycle_version,
                operational_status,
                extract(epoch FROM operational_status_changed_at)::BIGINT,
                completed_with_issue_count
         FROM mini_production_maps
         WHERE cardinality($1::TEXT[]) = 0 OR id = ANY($1::TEXT[])
         ORDER BY id",
    )
    .bind(&order_ids)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    rows.into_iter()
        .map(
            |(
                order_id,
                status,
                completion_outcome,
                lifecycle_changed_at_unix,
                production_completed_at_unix,
                closed_at_unix,
                lifecycle_version,
                operational_status,
                operational_status_changed_at_unix,
                completed_with_issue_count,
            )| {
                let record = ProductionOrderLifecycleRecord {
                    order_id: order_id.clone(),
                    status: ProductionOrderLifecycleStatus::parse(&status)?,
                    completion_outcome,
                    lifecycle_changed_at_unix,
                    production_completed_at_unix,
                    closed_at_unix,
                    lifecycle_version,
                    operational_status: ProductionOrderOperationalStatus::parse(
                        &operational_status,
                    )?,
                    operational_status_changed_at_unix,
                    completed_with_issue_count: usize::try_from(completed_with_issue_count)
                        .map_err(|_| ProductionMapError::StoreFailed)?,
                };
                Ok((order_id, record))
            },
        )
        .collect()
}

pub(super) async fn refresh_production_order_lifecycle_tx(
    tx: &mut Transaction<'_, Postgres>,
    order_id: &str,
    actor: &QueueActionActor,
    source_event_id: &str,
    reason: &str,
) -> Result<(), ProductionMapError> {
    let order_id = order_id.trim();
    let Some((
        map_json,
        current_status,
        current_version,
        current_operational_status,
        current_completed_with_issue_count,
    )) =
        sqlx::query_as::<_, (serde_json::Value, String, i64, String, i64)>(
            "SELECT map_json, lifecycle_status, lifecycle_version,
                    operational_status, completed_with_issue_count
             FROM mini_production_maps
             WHERE id = $1
             FOR UPDATE",
        )
        .bind(order_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
    else {
        return Err(ProductionMapError::MapNotFound);
    };
    let current_status = ProductionOrderLifecycleStatus::parse(&current_status)?;
    let current_operational_status =
        ProductionOrderOperationalStatus::parse(&current_operational_status)?;

    let map = serde_json::from_value::<ProductionMapDefinition>(map_json)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let queue_rows = sqlx::query_as::<_, (String, String)>(
        "SELECT canonical_apparatus_id, state
         FROM mini_queue_states
         WHERE order_id = $1",
    )
    .bind(order_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut queue_states = BTreeMap::<String, BTreeMap<String, String>>::new();
    for (apparatus_id, state) in queue_rows {
        queue_states
            .entry(apparatus_id)
            .or_default()
            .insert(order_id.to_string(), state);
    }
    let derived_status = derive_production_order_lifecycle(&map, &queue_states)
        .ok_or(ProductionMapError::StoreFailed)?;
    let lifecycle_changed = derived_status != current_status
        && current_status.can_automatically_transition_to(derived_status);
    let next_status = if lifecycle_changed {
        derived_status
    } else {
        current_status
    };
    let completed_with_issue_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)::BIGINT
         FROM mini_queue_action_events
         WHERE order_id = $1
           AND COALESCE(payload_json->>'completed_with_issue', 'false') = 'true'",
    )
        .bind(order_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let completed_with_issue_count_usize = usize::try_from(completed_with_issue_count)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let next_operational_status = derive_production_order_operational_status(
        next_status,
        &queue_states,
        order_id,
        completed_with_issue_count_usize,
    );
    let operational_changed = next_operational_status != current_operational_status
        || completed_with_issue_count != current_completed_with_issue_count;
    if !lifecycle_changed && !operational_changed {
        return Ok(());
    }
    let completion_outcome = match next_status {
        ProductionOrderLifecycleStatus::ProductionCompleted
        | ProductionOrderLifecycleStatus::Closed
            if completed_with_issue_count > 0 =>
        {
            "with_issue"
        }
        ProductionOrderLifecycleStatus::ProductionCompleted
        | ProductionOrderLifecycleStatus::Closed => {
            "normal"
        }
        _ => "",
    };
    let next_version = current_version + i64::from(lifecycle_changed);

    sqlx::query(
        "UPDATE mini_production_maps
         SET lifecycle_status = $2,
             completion_outcome = $3,
             lifecycle_changed_at = CASE WHEN $4 THEN now() ELSE lifecycle_changed_at END,
             production_completed_at = CASE
                 WHEN $4 AND $2 = 'production_completed'
                     THEN COALESCE(production_completed_at, now())
                 ELSE production_completed_at
             END,
             lifecycle_version = $5,
             operational_status = $6,
             operational_status_changed_at = CASE
                 WHEN $7 THEN now()
                 ELSE operational_status_changed_at
             END,
             completed_with_issue_count = $8
         WHERE id = $1",
    )
    .bind(order_id)
    .bind(next_status.as_str())
    .bind(completion_outcome)
    .bind(lifecycle_changed)
    .bind(next_version)
    .bind(next_operational_status.as_str())
    .bind(operational_changed)
    .bind(completed_with_issue_count)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    if !lifecycle_changed {
        return Ok(());
    }

    let source_event_id = source_event_id.trim();
    let event_id = if source_event_id.is_empty() {
        format!(
            "lifecycle:{order_id}:{next_version}:{}",
            next_status.as_str()
        )
    } else {
        format!("lifecycle:{source_event_id}:{}", next_status.as_str())
    };
    sqlx::query(
        "INSERT INTO mini_production_order_lifecycle_events
            (event_id, order_id, from_status, to_status, completion_outcome,
             actor_role, actor_ref, actor_display_name, source_event_id,
             reason, lifecycle_version, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, now())
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(event_id)
    .bind(order_id)
    .bind(current_status.as_str())
    .bind(next_status.as_str())
    .bind(completion_outcome)
    .bind(actor.role.trim())
    .bind(actor.ref_.trim())
    .bind(actor.display_name.trim())
    .bind(source_event_id)
    .bind(reason.trim())
    .bind(next_version)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}
