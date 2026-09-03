use std::collections::{BTreeMap, BTreeSet};

use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapError, ProductionOrderLifecycleRecord,
    ProductionOrderLifecycleStatus, ProductionOrderOperationalStatus, QueueActionActor,
    derive_production_order_lifecycle_with_completed_stage_nodes,
    derive_production_order_operational_status,
};

pub(crate) async fn load_production_order_lifecycles(
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
            String,
            String,
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
                completed_with_issue_count,
                flow_status,
                stock_status
         FROM mini_production_maps
         WHERE cardinality($1::TEXT[]) = 0 OR id = ANY($1::TEXT[])
         ORDER BY id",
    )
    .bind(&order_ids)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    Ok(rows
        .into_iter()
        .filter_map(
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
                flow_status,
                stock_status,
            )| {
                let status = match ProductionOrderLifecycleStatus::parse(&status) {
                    Ok(status) => status,
                    Err(error) => {
                        tracing::warn!(
                            ?error,
                            order_id = %order_id,
                            "skipping order lifecycle with invalid status"
                        );
                        return None;
                    }
                };
                let operational_status =
                    match ProductionOrderOperationalStatus::parse(&operational_status) {
                        Ok(status) => status,
                        Err(error) => {
                            tracing::warn!(
                                ?error,
                                order_id = %order_id,
                                "skipping order lifecycle with invalid operational status"
                            );
                            return None;
                        }
                    };
                let completed_with_issue_count = match usize::try_from(completed_with_issue_count) {
                    Ok(count) => count,
                    Err(error) => {
                        tracing::warn!(
                            ?error,
                            order_id = %order_id,
                            "skipping order lifecycle with invalid issue count"
                        );
                        return None;
                    }
                };
                let record = ProductionOrderLifecycleRecord {
                    order_id: order_id.clone(),
                    status,
                    completion_outcome,
                    lifecycle_changed_at_unix,
                    production_completed_at_unix,
                    closed_at_unix,
                    lifecycle_version,
                    operational_status,
                    operational_status_changed_at_unix,
                    completed_with_issue_count,
                    flow_status,
                    stock_status,
                };
                Some((order_id, record))
            },
        )
        .collect())
}

pub(crate) async fn refresh_production_order_lifecycle_tx(
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
        current_flow_status,
        current_stock_status,
    )) = sqlx::query_as::<_, (serde_json::Value, String, i64, String, i64, String, String)>(
        "SELECT map_json, lifecycle_status, lifecycle_version,
                operational_status, completed_with_issue_count,
                flow_status, stock_status
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
    let completed_stage_node_ids = sqlx::query_scalar::<_, String>(
        "SELECT stage_node_id
         FROM mini_queue_action_events
         WHERE order_id = $1
           AND action = 'complete'
           AND stage_node_id <> ''",
    )
    .bind(order_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .into_iter()
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
    .collect::<BTreeSet<_>>();
    let derived_status = derive_production_order_lifecycle_with_completed_stage_nodes(
        &map,
        &queue_states,
        &completed_stage_node_ids,
    )
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
    let completed_with_issue_count_usize =
        usize::try_from(completed_with_issue_count).map_err(|_| ProductionMapError::StoreFailed)?;
    let has_roll_detached = sqlx::query_scalar::<_, i64>(
        "SELECT count(*)::BIGINT
         FROM mini_order_run_sessions
         WHERE order_id = $1
           AND status = 'roll_detached'",
    )
    .bind(order_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
        > 0;

    let (free_wip_count, waiting_next_stage_count, in_use_wip_count, accepted_wip_count) =
        sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT count(*) FILTER (
                        WHERE wip_status = 'waiting'
                          AND (COALESCE(canonical_next_apparatus_id, '') = '' OR canonical_next_apparatus_id IS NULL)
                    )::BIGINT,
                    count(*) FILTER (
                        WHERE wip_status = 'waiting'
                          AND COALESCE(canonical_next_apparatus_id, '') <> ''
                    )::BIGINT,
                    count(*) FILTER (
                        WHERE wip_status = 'in_use'
                    )::BIGINT,
                    count(*) FILTER (
                        WHERE wip_status = 'processed'
                          AND lower(COALESCE(processed_by_apparatus, '')) LIKE 'warehouse:%'
                    )::BIGINT
             FROM mini_progress_batches
             WHERE order_id = $1",
        )
        .bind(order_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;

    let free_wip_usize = usize::try_from(free_wip_count).unwrap_or(0);
    let waiting_next_stage_usize = usize::try_from(waiting_next_stage_count).unwrap_or(0);
    let in_use_wip_usize = usize::try_from(in_use_wip_count).unwrap_or(0);
    let accepted_wip_usize = usize::try_from(accepted_wip_count).unwrap_or(0);

    let next_operational_status = derive_production_order_operational_status(
        next_status,
        &queue_states,
        order_id,
        completed_with_issue_count_usize,
        has_roll_detached,
        waiting_next_stage_usize,
    );
    let (next_flow_status, next_stock_status) =
        crate::core::production_map::derive_order_flow_and_stock_status(
            next_operational_status.as_str(),
            free_wip_usize,
            waiting_next_stage_usize,
            in_use_wip_usize,
            accepted_wip_usize,
        );

    let flow_changed = next_flow_status != current_flow_status.as_str()
        || next_stock_status != current_stock_status.as_str();
    let operational_changed = next_operational_status != current_operational_status
        || completed_with_issue_count != current_completed_with_issue_count;
    if !lifecycle_changed && !operational_changed && !flow_changed {
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
        | ProductionOrderLifecycleStatus::Closed => "normal",
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
             completed_with_issue_count = $8,
             flow_status = $9,
             stock_status = $10
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
    .bind(next_flow_status)
    .bind(next_stock_status)
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
