use sqlx::PgPool;

use crate::core::production_map::{
    CompletionRequestDecision, CompletionRequestDecisionNotification,
    CompletionRequestNotification, CompletionRequestStateResolution, ProductionMapError,
    QueueActionActor,
};

use super::progress_helpers::put_order_run_session_tx;
use super::queue_helpers::{insert_queue_action_event_tx, put_queue_states_tx};

#[derive(sqlx::FromRow)]
struct CompletionRequestRow {
    event_id: String,
    apparatus: String,
    order_id: String,
    order_number: String,
    order_title: String,
    product_code: String,
    worker_role: String,
    worker_ref: String,
    worker_display_name: String,
    description: String,
    notice_kind: String,
    decision_required: bool,
    created_at_unix: i64,
}

#[derive(sqlx::FromRow)]
struct CompletionRequestDecisionRow {
    event_id: String,
    request_event_id: String,
    decision: String,
    apparatus: String,
    order_id: String,
    order_number: String,
    order_title: String,
    product_code: String,
    worker_role: String,
    worker_ref: String,
    worker_display_name: String,
    decided_by_role: String,
    decided_by_ref: String,
    decided_by_display_name: String,
    description: String,
    message: String,
    created_at_unix: i64,
}

pub(super) async fn load_completion_requests(
    pool: &PgPool,
    limit: usize,
) -> Result<Vec<CompletionRequestNotification>, ProductionMapError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let limit = i64::try_from(limit.min(500)).unwrap_or(500);
    let rows = sqlx::query_as::<_, CompletionRequestRow>(
        "SELECT event_id,
                apparatus,
                order_id,
                COALESCE(payload_json->>'order_number', '') AS order_number,
                COALESCE(payload_json->>'order_title', '') AS order_title,
                COALESCE(payload_json->>'product_code', '') AS product_code,
                actor_role AS worker_role,
                actor_ref AS worker_ref,
                actor_display_name AS worker_display_name,
                COALESCE(payload_json->>'description', '') AS description,
                COALESCE(payload_json->>'notice_kind', 'completion_request') AS notice_kind,
                COALESCE((payload_json->>'decision_required')::boolean, true) AS decision_required,
                EXTRACT(EPOCH FROM created_at)::bigint AS created_at_unix
         FROM mini_queue_action_events
         WHERE action = 'complete'
           AND (
                (
                    payload_json->>'completion_request' = 'true'
                    AND COALESCE(payload_json->>'completion_request_status', 'pending') = 'pending'
                )
                OR payload_json->>'notice_kind' = 'laminatsiya_double_leftover'
           )
         ORDER BY created_at DESC, id DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    Ok(rows
        .into_iter()
        .filter(|row| !row.description.trim().is_empty())
        .map(completion_request_from_row)
        .collect())
}

pub(super) async fn load_completion_request_by_event_id(
    pool: &PgPool,
    event_id: &str,
) -> Result<Option<CompletionRequestNotification>, ProductionMapError> {
    let event_id = event_id.trim();
    if event_id.is_empty() {
        return Ok(None);
    }
    let row = sqlx::query_as::<_, CompletionRequestRow>(
        "SELECT event_id,
                apparatus,
                order_id,
                COALESCE(payload_json->>'order_number', '') AS order_number,
                COALESCE(payload_json->>'order_title', '') AS order_title,
                COALESCE(payload_json->>'product_code', '') AS product_code,
                actor_role AS worker_role,
                actor_ref AS worker_ref,
                actor_display_name AS worker_display_name,
                COALESCE(payload_json->>'description', '') AS description,
                COALESCE(payload_json->>'notice_kind', 'completion_request') AS notice_kind,
                true AS decision_required,
                EXTRACT(EPOCH FROM created_at)::bigint AS created_at_unix
         FROM mini_queue_action_events
         WHERE event_id = $1
           AND action = 'complete'
           AND payload_json->>'completion_request' = 'true'
           AND COALESCE(payload_json->>'completion_request_status', 'pending') = 'pending'
         LIMIT 1",
    )
    .bind(event_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    Ok(row
        .filter(|row| !row.description.trim().is_empty())
        .map(completion_request_from_row))
}

pub(super) async fn load_completion_request_decisions_for_actor(
    pool: &PgPool,
    actor_ref: &str,
    limit: usize,
) -> Result<Vec<CompletionRequestDecisionNotification>, ProductionMapError> {
    let actor_ref = actor_ref.trim();
    if actor_ref.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let limit = i64::try_from(limit.min(500)).unwrap_or(500);
    let rows = sqlx::query_as::<_, CompletionRequestDecisionRow>(
        "SELECT
                COALESCE(payload_json->>'decision_event_id', '') AS event_id,
                event_id AS request_event_id,
                COALESCE(payload_json->>'completion_request_status', '') AS decision,
                apparatus,
                order_id,
                COALESCE(payload_json->>'order_number', '') AS order_number,
                COALESCE(payload_json->>'order_title', '') AS order_title,
                COALESCE(payload_json->>'product_code', '') AS product_code,
                actor_role AS worker_role,
                actor_ref AS worker_ref,
                actor_display_name AS worker_display_name,
                COALESCE(payload_json->>'decided_by_role', '') AS decided_by_role,
                COALESCE(payload_json->>'decided_by_ref', '') AS decided_by_ref,
                COALESCE(payload_json->>'decided_by_display_name', '') AS decided_by_display_name,
                COALESCE(payload_json->>'description', '') AS description,
                COALESCE(payload_json->>'decision_message', '') AS message,
                COALESCE((payload_json->>'decision_at_unix')::bigint, EXTRACT(EPOCH FROM created_at)::bigint) AS created_at_unix
         FROM mini_queue_action_events
         WHERE action = 'complete'
           AND payload_json->>'completion_request' = 'true'
           AND payload_json->>'completion_request_status' IN ('approved', 'rejected')
           AND actor_ref = $1
         ORDER BY created_at_unix DESC, id DESC
         LIMIT $2",
    )
    .bind(actor_ref)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    Ok(rows
        .into_iter()
        .map(|row| CompletionRequestDecisionNotification {
            event_id: row.event_id,
            request_event_id: row.request_event_id,
            decision: row.decision,
            apparatus: row.apparatus,
            order_id: row.order_id,
            order_number: row.order_number,
            order_title: row.order_title,
            product_code: row.product_code,
            worker_role: row.worker_role,
            worker_ref: row.worker_ref,
            worker_display_name: row.worker_display_name,
            decided_by_role: row.decided_by_role,
            decided_by_ref: row.decided_by_ref,
            decided_by_display_name: row.decided_by_display_name,
            description: row.description,
            message: row.message,
            created_at_unix: row.created_at_unix,
        })
        .collect())
}

pub(super) async fn resolve_completion_request_decision(
    pool: &PgPool,
    request_event_id: &str,
    decision: CompletionRequestDecision,
    actor: &QueueActionActor,
    notification: &CompletionRequestDecisionNotification,
    state_resolution: Option<CompletionRequestStateResolution>,
) -> Result<(), ProductionMapError> {
    let request_event_id = request_event_id.trim();
    if request_event_id.is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    if let Some(resolution) = state_resolution {
        put_queue_states_tx(&mut tx, &resolution.apparatus, resolution.states).await?;
        insert_queue_action_event_tx(&mut tx, &resolution.event).await?;
        if let Some(session) = resolution.session {
            put_order_run_session_tx(&mut tx, &session).await?;
        }
    }
    let result = sqlx::query(
        "UPDATE mini_queue_action_events
         SET payload_json = payload_json || $2::jsonb
         WHERE event_id = $1
           AND payload_json->>'completion_request' = 'true'
           AND COALESCE(payload_json->>'completion_request_status', 'pending') = 'pending'",
    )
    .bind(request_event_id)
    .bind(serde_json::json!({
        "completion_request_status": decision.as_str(),
        "decision_event_id": notification.event_id,
        "decision_message": notification.message,
        "decided_by_role": actor.role.trim(),
        "decided_by_ref": actor.ref_.trim(),
        "decided_by_display_name": notification.decided_by_display_name,
        "decision_at_unix": notification.created_at_unix,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if result.rows_affected() != 1 {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

fn completion_request_from_row(row: CompletionRequestRow) -> CompletionRequestNotification {
    CompletionRequestNotification {
        event_id: row.event_id,
        apparatus: row.apparatus,
        order_id: row.order_id,
        order_number: row.order_number,
        order_title: row.order_title,
        product_code: row.product_code,
        worker_role: row.worker_role,
        worker_ref: row.worker_ref,
        worker_display_name: row.worker_display_name,
        description: row.description,
        notice_kind: row.notice_kind,
        decision_required: row.decision_required,
        created_at_unix: row.created_at_unix,
    }
}
