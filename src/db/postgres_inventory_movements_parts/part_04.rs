
async fn load_transfer(
    pool: &PgPool,
    transfer_id: &str,
) -> Result<InventoryTransfer, InventoryMovementError> {
    let row = sqlx::query_as::<_, InventoryTransferRow>(
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
        "#,
    )
    .bind(transfer_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(store_error)?
    .ok_or(InventoryMovementError::TransferNotFound)?;
    let mut transfers = hydrate_transfers(pool, vec![row]).await?;
    transfers
        .pop()
        .ok_or(InventoryMovementError::TransferNotFound)
}

async fn hydrate_transfers(
    pool: &PgPool,
    rows: Vec<InventoryTransferRow>,
) -> Result<Vec<InventoryTransfer>, InventoryMovementError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    let line_rows = sqlx::query_as::<_, InventoryTransferLineRow>(
        r#"
        SELECT
            transfer_id, asset_kind, asset_ref,
            item_code, item_name, identifier,
            (qty * 1000000)::bigint AS qty_units, uom, source_physical_location_id
        FROM mini_inventory_transfer_lines
        WHERE transfer_id = ANY($1)
        ORDER BY transfer_id, asset_kind, asset_ref
        "#,
    )
    .bind(ids)
    .fetch_all(pool)
    .await
    .map_err(store_error)?;
    let mut lines_by_transfer = BTreeMap::<String, Vec<InventoryTransferLine>>::new();
    for line in line_rows {
        lines_by_transfer
            .entry(line.transfer_id)
            .or_default()
            .push(InventoryTransferLine {
                asset_kind: InventoryAssetKind::parse(&line.asset_kind)?,
                asset_ref: line.asset_ref,
                item_code: line.item_code,
                item_name: line.item_name,
                identifier: line.identifier,
                qty: erp_quantity_from_units(line.qty_units),
                uom: line.uom,
                source_physical_location_id: line.source_physical_location_id,
            });
    }
    rows.into_iter()
        .map(|row| {
            let lines = lines_by_transfer.remove(&row.id).unwrap_or_default();
            Ok(InventoryTransfer {
                id: row.id,
                source_warehouse_id: row.source_warehouse_id,
                source_warehouse: row.source_warehouse,
                destination_warehouse_id: row.destination_warehouse_id,
                destination_warehouse: row.destination_warehouse,
                status: InventoryTransferStatus::parse(&row.status)?,
                note: row.note,
                requested_by_name: row.requested_by_name,
                approved_by_name: row.approved_by_name,
                dispatched_by_name: row.dispatched_by_name,
                received_by_name: row.received_by_name,
                rejected_by_name: row.rejected_by_name,
                cancelled_by_name: row.cancelled_by_name,
                created_at_unix: row.created_at_unix,
                approved_at_unix: row.approved_at_unix,
                dispatched_at_unix: row.dispatched_at_unix,
                received_at_unix: row.received_at_unix,
                rejected_at_unix: row.rejected_at_unix,
                cancelled_at_unix: row.cancelled_at_unix,
                lines,
            })
        })
        .collect()
}

struct MovementEventDraft<'a> {
    idempotency_key: String,
    event_type: &'a str,
    transfer_id: &'a str,
    asset_kind: InventoryAssetKind,
    asset_ref: String,
    from_warehouse_id: &'a str,
    to_warehouse_id: &'a str,
    from_location_id: &'a str,
    to_location_id: &'a str,
    qty_units: i64,
    uom: &'a str,
    actor: &'a InventoryActor,
    note: &'a str,
}

async fn insert_movement_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    draft: MovementEventDraft<'_>,
) -> Result<(), InventoryMovementError> {
    sqlx::query(
        r#"
        INSERT INTO mini_inventory_movement_events (
            id, idempotency_key, event_type, transfer_id,
            asset_kind, asset_ref,
            from_warehouse_id, to_warehouse_id,
            from_location_id, to_location_id,
            qty, uom,
            actor_role, actor_ref, actor_name,
            note, payload_json
        )
        VALUES (
            $1, $2, $3, NULLIF($4, ''),
            $5, $6, $7, $8, $9, $10,
            ($11::bigint::numeric / 1000000)::numeric(18,6), $12,
            $13, $14, $15, $16, '{}'::jsonb
        )
        ON CONFLICT (idempotency_key) DO NOTHING
        "#,
    )
    .bind(random_id("inventory_event"))
    .bind(draft.idempotency_key)
    .bind(draft.event_type)
    .bind(draft.transfer_id)
    .bind(draft.asset_kind.as_str())
    .bind(draft.asset_ref)
    .bind(draft.from_warehouse_id)
    .bind(draft.to_warehouse_id)
    .bind(draft.from_location_id)
    .bind(draft.to_location_id)
    .bind(draft.qty_units)
    .bind(draft.uom)
    .bind(inventory_role_code(&draft.actor.principal.role))
    .bind(draft.actor.principal.ref_.trim())
    .bind(draft.actor.principal.display_name.trim())
    .bind(draft.note)
    .execute(&mut **tx)
    .await
    .map_err(store_error)?;
    Ok(())
}

fn random_id(prefix: &str) -> String {
    format!("{prefix}_{}", HEXLOWER.encode(&rand::random::<[u8; 16]>()))
}

fn store_error(_error: sqlx::Error) -> InventoryMovementError {
    InventoryMovementError::StoreFailed
}
