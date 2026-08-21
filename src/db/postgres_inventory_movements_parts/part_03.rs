
async fn warehouse_by_id_tx(
    tx: &mut Transaction<'_, Postgres>,
    warehouse_id: &str,
) -> Result<WarehouseLookupRow, InventoryMovementError> {
    warehouse_lookup_query()
        .bind(warehouse_id.trim())
        .fetch_optional(&mut **tx)
        .await
        .map_err(store_error)?
        .ok_or(InventoryMovementError::WarehouseNotFound)
}

fn warehouse_lookup_query<'q>()
-> sqlx::query::QueryAs<'q, Postgres, WarehouseLookupRow, sqlx::postgres::PgArguments> {
    // create_transfer executes this query inside its transaction. The row lock
    // participates in the warehouse delete protocol until that transaction ends.
    sqlx::query_as::<_, WarehouseLookupRow>(
        r#"
        SELECT
            warehouse.id,
            warehouse.name,
            warehouse.is_group,
            warehouse.parent_warehouse,
            (
                SELECT count(*)::bigint
                FROM mini_warehouse_assignments assignment
                WHERE assignment.assignment_kind = 'warehouse'
                  AND lower(assignment.warehouse_name) = lower(warehouse.name)
            ) AS assignment_count
        FROM mini_warehouses warehouse
        WHERE warehouse.id = $1
        FOR KEY SHARE OF warehouse
        "#,
    )
}

fn ensure_transfer_warehouse(warehouse: &WarehouseLookupRow) -> Result<(), InventoryMovementError> {
    if warehouse.is_group || !warehouse.parent_warehouse.trim().is_empty() {
        Err(InventoryMovementError::WarehouseNotFound)
    } else {
        Ok(())
    }
}

async fn warehouse_location_id_tx(
    tx: &mut Transaction<'_, Postgres>,
    warehouse_id: &str,
) -> Result<String, InventoryMovementError> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT id
        FROM mini_inventory_locations
        WHERE kind = 'warehouse' AND warehouse_id = $1 AND active = true
        "#,
    )
    .bind(warehouse_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(store_error)?
    .ok_or(InventoryMovementError::LocationNotFound)
}

async fn enqueue_transfer_chat_events_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer_id: &str,
    status: &str,
    destination_warehouse: &str,
) -> Result<(), InventoryMovementError> {
    let existing_targets = sqlx::query_as::<_, (String, String, String)>(
        r#"
        SELECT DISTINCT target_role, target_ref, target_display_name
        FROM mini_inventory_transfer_chat_outbox
        WHERE transfer_id = $1
        ORDER BY target_role, target_ref
        "#,
    )
    .bind(transfer_id.trim())
    .fetch_all(&mut **tx)
    .await
    .map_err(store_error)?;
    let targets = if existing_targets.is_empty() {
        sqlx::query_as::<_, (String, String, String)>(
            r#"
        SELECT principal_role, principal_ref, display_name
        FROM mini_warehouse_assignments
        WHERE assignment_kind = 'warehouse'
          AND lower(warehouse_name) = lower($1)
          AND principal_role <> 'customer'
        ORDER BY lower(display_name), lower(principal_ref)
        "#,
        )
        .bind(destination_warehouse.trim())
        .fetch_all(&mut **tx)
        .await
        .map_err(store_error)?
    } else {
        existing_targets
    };
    if targets.is_empty() {
        return Err(InventoryMovementError::DestinationWarehouseUnassigned);
    }
    for (target_role, target_ref, target_display_name) in targets {
        sqlx::query(
            r#"
            INSERT INTO mini_inventory_transfer_chat_outbox (
                event_id, transfer_id, status,
                target_role, target_ref, target_display_name
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (transfer_id, target_role, target_ref, status) DO NOTHING
            "#,
        )
        .bind(random_id("inventory_transfer_chat"))
        .bind(transfer_id.trim())
        .bind(status)
        .bind(target_role)
        .bind(target_ref)
        .bind(target_display_name)
        .execute(&mut **tx)
        .await
        .map_err(store_error)?;
    }
    Ok(())
}

fn transfer_chat_status(action: InventoryTransferActionKind) -> &'static str {
    match action {
        InventoryTransferActionKind::Approve => "approved",
        InventoryTransferActionKind::Reject => "rejected",
        InventoryTransferActionKind::Dispatch => "in_transit",
        InventoryTransferActionKind::Receive => "received",
        InventoryTransferActionKind::Cancel => "cancelled",
    }
}

async fn advisory_idempotency_lock(
    tx: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
) -> Result<(), InventoryMovementError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1)::bigint)")
        .bind(idempotency_key.trim())
        .execute(&mut **tx)
        .await
        .map_err(store_error)?;
    Ok(())
}

async fn transfer_id_by_idempotency_tx(
    tx: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
) -> Result<Option<String>, InventoryMovementError> {
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM mini_inventory_transfers WHERE idempotency_key = $1",
    )
    .bind(idempotency_key.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(store_error)
}

async fn movement_event_identity_tx(
    tx: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
) -> Result<Option<MovementIdentityRow>, InventoryMovementError> {
    sqlx::query_as::<_, MovementIdentityRow>(
        r#"
        SELECT event_type, asset_kind, asset_ref, to_location_id
        FROM mini_inventory_movement_events
        WHERE idempotency_key = $1
        "#,
    )
    .bind(idempotency_key.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(store_error)
}

async fn transfer_action_identity_tx(
    tx: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
) -> Result<Option<TransferActionIdentityRow>, InventoryMovementError> {
    sqlx::query_as::<_, TransferActionIdentityRow>(
        r#"
        SELECT transfer_id, action
        FROM mini_inventory_transfer_actions
        WHERE idempotency_key = $1
        "#,
    )
    .bind(idempotency_key.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(store_error)
}

async fn insert_transfer_action_identity_tx(
    tx: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
    transfer_id: &str,
    action: InventoryTransferActionKind,
    actor: &InventoryActor,
) -> Result<(), InventoryMovementError> {
    sqlx::query(
        r#"
        INSERT INTO mini_inventory_transfer_actions (
            idempotency_key, transfer_id, action,
            actor_role, actor_ref, actor_name
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(idempotency_key.trim())
    .bind(transfer_id.trim())
    .bind(action.as_str())
    .bind(inventory_role_code(&actor.principal.role))
    .bind(actor.principal.ref_.trim())
    .bind(actor.principal.display_name.trim())
    .execute(&mut **tx)
    .await
    .map_err(store_error)?;
    Ok(())
}

async fn transfer_for_update_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer_id: &str,
) -> Result<InventoryTransferRow, InventoryMovementError> {
    sqlx::query_as::<_, InventoryTransferRow>(
        r#"
        SELECT
            id, source_warehouse_id, source_warehouse,
            destination_warehouse_id, destination_warehouse,
            status, note,
            requested_by_name, approved_by_name, dispatched_by_name,
            received_by_name, rejected_by_name, cancelled_by_name,
            EXTRACT(EPOCH FROM created_at)::bigint AS created_at_unix,
            EXTRACT(EPOCH FROM approved_at)::bigint AS approved_at_unix,
            EXTRACT(EPOCH FROM dispatched_at)::bigint AS dispatched_at_unix,
            EXTRACT(EPOCH FROM received_at)::bigint AS received_at_unix,
            EXTRACT(EPOCH FROM rejected_at)::bigint AS rejected_at_unix,
            EXTRACT(EPOCH FROM cancelled_at)::bigint AS cancelled_at_unix
        FROM mini_inventory_transfers
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(transfer_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(store_error)?
    .ok_or(InventoryMovementError::TransferNotFound)
}

async fn transfer_lines_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer_id: &str,
) -> Result<Vec<InventoryTransferLineRow>, InventoryMovementError> {
    sqlx::query_as::<_, InventoryTransferLineRow>(
        r#"
        SELECT
            transfer_id, asset_kind, asset_ref,
            item_code, item_name, identifier,
            (qty * 1000000)::bigint AS qty_units, uom, source_physical_location_id
        FROM mini_inventory_transfer_lines
        WHERE transfer_id = $1
        ORDER BY asset_kind, asset_ref
        "#,
    )
    .bind(transfer_id.trim())
    .fetch_all(&mut **tx)
    .await
    .map_err(store_error)
}

async fn update_transfer_actor_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer_id: &str,
    actor_column: &str,
    status: &str,
    actor: &InventoryActor,
) -> Result<(), InventoryMovementError> {
    let allowed = [
        "approved",
        "rejected",
        "dispatched",
        "received",
        "cancelled",
    ];
    if !allowed.contains(&actor_column) {
        return Err(InventoryMovementError::StoreFailed);
    }
    let query = format!(
        "UPDATE mini_inventory_transfers
         SET status = $2,
             {actor_column}_by_role = $3,
             {actor_column}_by_ref = $4,
             {actor_column}_by_name = $5,
             {actor_column}_at = now(),
             updated_at = now()
         WHERE id = $1"
    );
    sqlx::query(&query)
        .bind(transfer_id)
        .bind(status)
        .bind(inventory_role_code(&actor.principal.role))
        .bind(actor.principal.ref_.trim())
        .bind(actor.principal.display_name.trim())
        .execute(&mut **tx)
        .await
        .map_err(store_error)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_transfer_stage_events_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer: &InventoryTransferRow,
    lines: &[InventoryTransferLineRow],
    actor: &InventoryActor,
    note: &str,
    idempotency_prefix: &str,
    event_type: &str,
    moved_to_destination: bool,
) -> Result<(), InventoryMovementError> {
    let destination_location = if moved_to_destination {
        Some(warehouse_location_id_tx(tx, &transfer.destination_warehouse_id).await?)
    } else {
        None
    };
    for line in lines {
        insert_movement_event_tx(
            tx,
            MovementEventDraft {
                idempotency_key: format!(
                    "{}:{}:{}",
                    idempotency_prefix,
                    line.asset_kind,
                    line.asset_ref.to_ascii_lowercase()
                ),
                event_type,
                transfer_id: &transfer.id,
                asset_kind: InventoryAssetKind::parse(&line.asset_kind)?,
                asset_ref: line.asset_ref.clone(),
                from_warehouse_id: &transfer.source_warehouse_id,
                to_warehouse_id: &transfer.destination_warehouse_id,
                from_location_id: &line.source_physical_location_id,
                to_location_id: destination_location
                    .as_deref()
                    .unwrap_or(&line.source_physical_location_id),
                qty_units: line.qty_units,
                uom: &line.uom,
                actor,
                note,
            },
        )
        .await?;
    }
    Ok(())
}

fn action_already_applied(
    status: InventoryTransferStatus,
    action: InventoryTransferActionKind,
) -> bool {
    matches!(
        (status, action),
        (
            InventoryTransferStatus::Approved
                | InventoryTransferStatus::InTransit
                | InventoryTransferStatus::Received,
            InventoryTransferActionKind::Approve
        ) | (
            InventoryTransferStatus::Rejected,
            InventoryTransferActionKind::Reject
        ) | (
            InventoryTransferStatus::InTransit | InventoryTransferStatus::Received,
            InventoryTransferActionKind::Dispatch
        ) | (
            InventoryTransferStatus::Received,
            InventoryTransferActionKind::Receive
        ) | (
            InventoryTransferStatus::Cancelled,
            InventoryTransferActionKind::Cancel
        )
    )
}

fn transfer_matches_create_request(
    transfer: &InventoryTransfer,
    input: &InventoryTransferCreate,
) -> bool {
    if !transfer
        .source_warehouse_id
        .eq_ignore_ascii_case(&input.source_warehouse_id)
        || !transfer
            .destination_warehouse_id
            .eq_ignore_ascii_case(&input.destination_warehouse_id)
        || transfer.note != input.note
    {
        return false;
    }
    let existing = transfer
        .lines
        .iter()
        .map(|line| {
            (
                line.asset_kind.as_str().to_string(),
                line.asset_ref.to_ascii_lowercase(),
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    let requested = input
        .assets
        .iter()
        .map(|asset| {
            (
                asset.asset_kind.as_str().to_string(),
                asset.asset_ref.to_ascii_lowercase(),
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    existing == requested && existing.len() == transfer.lines.len()
}
