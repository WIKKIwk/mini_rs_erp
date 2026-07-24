use std::collections::BTreeMap;

use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{
    OrderControlRecord, OrderControlState, OrderFreezeRequest, OrderFreezeRequestStatus,
    ProductionMapError, QueueActionActor,
};

#[derive(sqlx::FromRow)]
struct OrderControlRow {
    order_id: String,
    state: String,
    actor_role: String,
    actor_ref: String,
    actor_display_name: String,
    requested_at_unix: i64,
    frozen_at_unix: Option<i64>,
    request_id: Option<String>,
    request_status: Option<String>,
    target_session_id: Option<String>,
    target_apparatus: Option<String>,
    target_worker_role: Option<String>,
    target_worker_ref: Option<String>,
    target_worker_display_name: Option<String>,
    request_transitioned_at_unix: Option<i64>,
}

pub(super) async fn load_order_control_states(
    pool: &PgPool,
) -> Result<BTreeMap<String, OrderControlRecord>, ProductionMapError> {
    let rows = sqlx::query_as::<_, OrderControlRow>(
        r#"SELECT
             control.order_id,
             control.state,
             control.actor_role,
             control.actor_ref,
             control.actor_display_name,
             control.requested_at_unix,
             control.frozen_at_unix,
             request.request_id,
             request.status AS request_status,
             request.target_session_id,
             request.target_apparatus,
             request.target_worker_role,
             request.target_worker_ref,
             request.target_worker_display_name,
             request.transitioned_at_unix AS request_transitioned_at_unix
           FROM mini_order_control_states control
           LEFT JOIN mini_order_freeze_requests request
             ON request.request_id = control.freeze_request_id
           ORDER BY control.order_id ASC"#,
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    rows.into_iter()
        .map(|row| {
            let state =
                OrderControlState::parse(&row.state).ok_or(ProductionMapError::StoreFailed)?;
            let freeze_request = match (row.request_id, row.request_status) {
                (Some(request_id), Some(status)) => Some(OrderFreezeRequest {
                    request_id,
                    status: OrderFreezeRequestStatus::parse(&status)
                        .ok_or(ProductionMapError::StoreFailed)?,
                    target_session_id: row.target_session_id.unwrap_or_default(),
                    target_apparatus: row.target_apparatus.unwrap_or_default(),
                    target_worker_role: row.target_worker_role.unwrap_or_default(),
                    target_worker_ref: row.target_worker_ref.unwrap_or_default(),
                    target_worker_display_name: row.target_worker_display_name.unwrap_or_default(),
                    requested_at_unix: row.requested_at_unix,
                    transitioned_at_unix: row.request_transitioned_at_unix.unwrap_or_default(),
                }),
                (None, None) => None,
                _ => return Err(ProductionMapError::StoreFailed),
            };
            Ok((
                row.order_id.clone(),
                OrderControlRecord {
                    order_id: row.order_id,
                    state,
                    actor: QueueActionActor {
                        role: row.actor_role,
                        ref_: row.actor_ref,
                        display_name: row.actor_display_name,
                    },
                    requested_at_unix: row.requested_at_unix,
                    frozen_at_unix: row.frozen_at_unix,
                    freeze_request,
                },
            ))
        })
        .collect()
}

pub(super) async fn save_order_control_state(
    pool: &PgPool,
    record: &OrderControlRecord,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    save_order_control_state_tx(&mut tx, record).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn save_order_control_state_tx(
    tx: &mut Transaction<'_, Postgres>,
    record: &OrderControlRecord,
) -> Result<(), ProductionMapError> {
    let current = sqlx::query_as::<_, (String, Option<String>)>(
        r#"SELECT state, freeze_request_id
           FROM mini_order_control_states
           WHERE order_id = $1
           FOR UPDATE"#,
    )
    .bind(record.order_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    if let Some(request) = &record.freeze_request {
        validate_freeze_transition(current.as_ref(), record.state, request)?;
        save_freeze_request_tx(tx, record, request).await?;
    } else if current
        .as_ref()
        .is_some_and(|(state, _)| state != OrderControlState::Active.as_str())
    {
        return Err(ProductionMapError::OrderControlActionNotAllowed);
    }

    sqlx::query(
        r#"INSERT INTO mini_order_control_states
             (order_id, state, actor_role, actor_ref, actor_display_name,
              requested_at_unix, frozen_at_unix, freeze_request_id, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
           ON CONFLICT (order_id) DO UPDATE SET
              state = excluded.state,
              actor_role = excluded.actor_role,
              actor_ref = excluded.actor_ref,
              actor_display_name = excluded.actor_display_name,
              requested_at_unix = excluded.requested_at_unix,
              frozen_at_unix = excluded.frozen_at_unix,
              freeze_request_id = excluded.freeze_request_id,
              updated_at = excluded.updated_at"#,
    )
    .bind(record.order_id.trim())
    .bind(record.state.as_str())
    .bind(record.actor.role.trim())
    .bind(record.actor.ref_.trim())
    .bind(record.actor.display_name.trim())
    .bind(record.requested_at_unix)
    .bind(record.frozen_at_unix)
    .bind(
        record
            .freeze_request
            .as_ref()
            .map(|request| request.request_id.trim()),
    )
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

fn validate_freeze_transition(
    current: Option<&(String, Option<String>)>,
    next_state: OrderControlState,
    request: &OrderFreezeRequest,
) -> Result<(), ProductionMapError> {
    let current_state = current.map(|(state, _)| state.as_str()).unwrap_or("active");
    let current_request_id = current
        .and_then(|(_, request_id)| request_id.as_deref())
        .unwrap_or_default();
    let same_request =
        current_request_id.is_empty() || current_request_id == request.request_id.trim();
    let allowed = match request.status {
        OrderFreezeRequestStatus::Pending => {
            current_state == "active" && next_state == OrderControlState::FreezeRequested
        }
        OrderFreezeRequestStatus::Frozen => {
            next_state == OrderControlState::Frozen
                && (current_state == "active"
                    || (same_request && matches!(current_state, "freeze_requested" | "frozen")))
        }
        OrderFreezeRequestStatus::Cancelled => {
            same_request
                && current_state == "freeze_requested"
                && next_state == OrderControlState::Active
        }
        OrderFreezeRequestStatus::Unfrozen => {
            same_request && current_state == "frozen" && next_state == OrderControlState::Active
        }
    };
    allowed
        .then_some(())
        .ok_or(ProductionMapError::OrderControlActionNotAllowed)
}

async fn save_freeze_request_tx(
    tx: &mut Transaction<'_, Postgres>,
    control: &OrderControlRecord,
    request: &OrderFreezeRequest,
) -> Result<(), ProductionMapError> {
    let previous_status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM mini_order_freeze_requests WHERE request_id = $1 FOR UPDATE",
    )
    .bind(request.request_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let status_allowed = match (previous_status.as_deref(), request.status) {
        (None, OrderFreezeRequestStatus::Pending | OrderFreezeRequestStatus::Frozen)
        | (Some("pending"), OrderFreezeRequestStatus::Pending)
        | (Some("pending"), OrderFreezeRequestStatus::Frozen)
        | (Some("pending"), OrderFreezeRequestStatus::Cancelled)
        | (Some("frozen"), OrderFreezeRequestStatus::Frozen)
        | (Some("frozen"), OrderFreezeRequestStatus::Unfrozen)
        | (Some("cancelled"), OrderFreezeRequestStatus::Cancelled)
        | (Some("unfrozen"), OrderFreezeRequestStatus::Unfrozen) => true,
        _ => false,
    };
    if !status_allowed {
        return Err(ProductionMapError::OrderControlActionNotAllowed);
    }

    sqlx::query(
        r#"INSERT INTO mini_order_freeze_requests
             (request_id, order_id, status,
              requester_role, requester_ref, requester_display_name,
              target_session_id, target_apparatus, target_worker_role,
              target_worker_ref, target_worker_display_name,
              requested_at_unix, transitioned_at_unix, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, now())
           ON CONFLICT (request_id) DO UPDATE SET
              status = excluded.status,
              transitioned_at_unix = excluded.transitioned_at_unix,
              updated_at = excluded.updated_at"#,
    )
    .bind(request.request_id.trim())
    .bind(control.order_id.trim())
    .bind(request.status.as_str())
    .bind(control.actor.role.trim())
    .bind(control.actor.ref_.trim())
    .bind(control.actor.display_name.trim())
    .bind(request.target_session_id.trim())
    .bind(request.target_apparatus.trim())
    .bind(request.target_worker_role.trim())
    .bind(request.target_worker_ref.trim())
    .bind(request.target_worker_display_name.trim())
    .bind(request.requested_at_unix)
    .bind(request.transitioned_at_unix)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    if !request.target_worker_ref.trim().is_empty() {
        let event_id = format!(
            "order-freeze-chat:{}:{}",
            request.request_id.trim(),
            request.status.as_str()
        );
        sqlx::query(
            r#"INSERT INTO mini_order_freeze_chat_outbox
                 (event_id, request_id, status)
               VALUES ($1, $2, $3)
               ON CONFLICT (event_id) DO NOTHING"#,
        )
        .bind(event_id)
        .bind(request.request_id.trim())
        .bind(request.status.as_str())
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(())
}
