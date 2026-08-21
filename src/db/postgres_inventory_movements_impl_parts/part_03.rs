impl PostgresInventoryMovementStore {

    async fn transfer_action(
        &self,
        actor: &InventoryActor,
        transfer_id: &str,
        action: InventoryTransferActionKind,
        input: &InventoryTransferAction,
    ) -> Result<InventoryTransfer, InventoryMovementError> {
        let mut tx = self.pool.begin().await.map_err(store_error)?;
        advisory_idempotency_lock(&mut tx, &input.idempotency_key).await?;
        let transfer = transfer_for_update_tx(&mut tx, transfer_id).await?;
        let status = InventoryTransferStatus::parse(&transfer.status)?;
        let source_access = actor.can_manage_warehouse(&transfer.source_warehouse);
        let destination_access = actor.can_manage_warehouse(&transfer.destination_warehouse);
        let internal_transfer = actor.manages_transfer_internally(
            &transfer.source_warehouse,
            &transfer.destination_warehouse,
        );
        let lines = transfer_lines_tx(&mut tx, transfer_id).await?;

        let authorized = match action {
            InventoryTransferActionKind::Approve
            | InventoryTransferActionKind::Reject
            | InventoryTransferActionKind::Receive => destination_access,
            InventoryTransferActionKind::Dispatch | InventoryTransferActionKind::Cancel => {
                source_access
            }
        };
        if !authorized {
            return Err(InventoryMovementError::WarehouseForbidden);
        }
        if let Some(existing) = transfer_action_identity_tx(&mut tx, &input.idempotency_key).await?
        {
            if existing.transfer_id != transfer_id || existing.action != action.as_str() {
                return Err(InventoryMovementError::IdempotencyConflict);
            }
            tx.commit().await.map_err(store_error)?;
            return load_transfer(&self.pool, transfer_id).await;
        }
        if action_already_applied(status, action) {
            insert_transfer_action_identity_tx(
                &mut tx,
                &input.idempotency_key,
                transfer_id,
                action,
                actor,
            )
            .await?;
            tx.commit().await.map_err(store_error)?;
            return load_transfer(&self.pool, transfer_id).await;
        }
        match action {
            InventoryTransferActionKind::Approve | InventoryTransferActionKind::Reject => {
                if status != InventoryTransferStatus::Requested {
                    return Err(InventoryMovementError::InvalidTransition);
                }
            }
            InventoryTransferActionKind::Dispatch => {
                if status != InventoryTransferStatus::Approved {
                    return Err(InventoryMovementError::InvalidTransition);
                }
            }
            InventoryTransferActionKind::Receive => {
                if status != InventoryTransferStatus::InTransit {
                    return Err(InventoryMovementError::InvalidTransition);
                }
            }
            InventoryTransferActionKind::Cancel => {
                if !matches!(
                    status,
                    InventoryTransferStatus::Requested | InventoryTransferStatus::Approved
                ) {
                    return Err(InventoryMovementError::InvalidTransition);
                }
            }
        }
        ensure_transfer_assets_tx(&mut tx, transfer_id, &lines).await?;
        insert_transfer_action_identity_tx(
            &mut tx,
            &input.idempotency_key,
            transfer_id,
            action,
            actor,
        )
        .await?;

        match action {
            InventoryTransferActionKind::Approve => {
                update_transfer_actor_tx(&mut tx, transfer_id, "approved", "approved", actor)
                    .await?;
                if internal_transfer {
                    dispatch_transfer_assets_tx(&mut tx, transfer_id, &lines).await?;
                    update_transfer_actor_tx(
                        &mut tx,
                        transfer_id,
                        "dispatched",
                        "in_transit",
                        actor,
                    )
                    .await?;
                    receive_transfer_assets_tx(&mut tx, &transfer, &lines, actor).await?;
                    update_transfer_actor_tx(&mut tx, transfer_id, "received", "received", actor)
                        .await?;
                }
            }
            InventoryTransferActionKind::Reject => {
                release_transfer_assets_tx(&mut tx, transfer_id, &lines).await?;
                update_transfer_actor_tx(&mut tx, transfer_id, "rejected", "rejected", actor)
                    .await?;
            }
            InventoryTransferActionKind::Dispatch => {
                dispatch_transfer_assets_tx(&mut tx, transfer_id, &lines).await?;
                update_transfer_actor_tx(&mut tx, transfer_id, "dispatched", "in_transit", actor)
                    .await?;
                if internal_transfer {
                    receive_transfer_assets_tx(&mut tx, &transfer, &lines, actor).await?;
                    update_transfer_actor_tx(&mut tx, transfer_id, "received", "received", actor)
                        .await?;
                }
            }
            InventoryTransferActionKind::Receive => {
                receive_transfer_assets_tx(&mut tx, &transfer, &lines, actor).await?;
                update_transfer_actor_tx(&mut tx, transfer_id, "received", "received", actor)
                    .await?;
            }
            InventoryTransferActionKind::Cancel => {
                release_transfer_assets_tx(&mut tx, transfer_id, &lines).await?;
                update_transfer_actor_tx(&mut tx, transfer_id, "cancelled", "cancelled", actor)
                    .await?;
            }
        }

        let event_type = match action {
            InventoryTransferActionKind::Approve => "transfer_approved",
            InventoryTransferActionKind::Reject => "transfer_rejected",
            InventoryTransferActionKind::Dispatch => "transfer_dispatched",
            InventoryTransferActionKind::Receive => "transfer_received",
            InventoryTransferActionKind::Cancel => "transfer_cancelled",
        };
        insert_transfer_stage_events_tx(
            &mut tx,
            &transfer,
            &lines,
            actor,
            &input.note,
            &input.idempotency_key,
            event_type,
            action == InventoryTransferActionKind::Receive,
        )
        .await?;
        if internal_transfer && action == InventoryTransferActionKind::Approve {
            insert_transfer_stage_events_tx(
                &mut tx,
                &transfer,
                &lines,
                actor,
                &input.note,
                &format!("{}:internal:dispatch", input.idempotency_key),
                "transfer_dispatched",
                false,
            )
            .await?;
            insert_transfer_stage_events_tx(
                &mut tx,
                &transfer,
                &lines,
                actor,
                &input.note,
                &format!("{}:internal:receive", input.idempotency_key),
                "transfer_received",
                true,
            )
            .await?;
        } else if internal_transfer && action == InventoryTransferActionKind::Dispatch {
            insert_transfer_stage_events_tx(
                &mut tx,
                &transfer,
                &lines,
                actor,
                &input.note,
                &format!("{}:internal:receive", input.idempotency_key),
                "transfer_received",
                true,
            )
            .await?;
        }
        if !internal_transfer {
            enqueue_transfer_chat_events_tx(
                &mut tx,
                transfer_id,
                transfer_chat_status(action),
                &transfer.destination_warehouse,
            )
            .await?;
        }
        tx.commit().await.map_err(store_error)?;
        load_transfer(&self.pool, transfer_id).await
    }
}
