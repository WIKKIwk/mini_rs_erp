
#[derive(Debug, Error)]
pub enum OrderResetError {
    #[error("order reset store failed")]
    StoreFailed(#[source] sqlx::Error),
    #[error("order reset verification failed")]
    VerificationFailed,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct OrderResetReport {
    pub orders_deleted: u64,
    pub production_maps_deleted: u64,
    pub order_products_deleted: u64,
    pub queue_states_deleted: u64,
    pub queue_events_deleted: u64,
    pub run_sessions_deleted: u64,
    pub progress_events_deleted: u64,
    pub progress_batches_deleted: u64,
    pub progress_corrections_deleted: u64,
    pub raw_material_assignments_deleted: u64,
    pub raw_material_events_deleted: u64,
    pub raw_material_rows_restored: u64,
    pub paddons_deleted: u64,
    pub qolip_locations_restored: u64,
    pub qolip_checkouts_deleted: u64,
    pub qolip_order_notes_deleted: u64,
    pub returned_paint_requests_deleted: u64,
    pub returned_paint_images_deleted: u64,
    pub apparatus_transfers_deleted: u64,
    pub schedule_reservations_deleted: u64,
    pub finished_goods_deleted: u64,
    pub engine_events_deleted: u64,
    pub idempotency_keys_deleted: u64,
}

#[derive(Clone)]
pub struct PostgresOrderResetStore {
    pool: PgPool,
}

impl PostgresOrderResetStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn reset_all_orders(&self) -> Result<OrderResetReport, OrderResetError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(OrderResetError::StoreFailed)?;

        sqlx::query(
            "SELECT pg_advisory_xact_lock(
                hashtextextended('mini-rs-erp:emergency-reset:orders', 0)
             )",
        )
        .execute(&mut *tx)
        .await
        .map_err(OrderResetError::StoreFailed)?;
        sqlx::query("SELECT set_config('mini_rs_erp.order_reset', 'on', true)")
            .execute(&mut *tx)
            .await
            .map_err(OrderResetError::StoreFailed)?;

        create_reset_targets(&mut tx).await?;

        let report = OrderResetReport {
            orders_deleted: count_rows(
                &mut tx,
                "SELECT COUNT(*)
                 FROM mini_orders
                 WHERE lower(id) IN (SELECT lower(id) FROM reset_order_ids)",
            )
            .await?,
            production_maps_deleted: count_rows(
                &mut tx,
                "SELECT COUNT(*) FROM mini_production_maps",
            )
            .await?,
            ..OrderResetReport::default()
        };

        let mut report = report;
        report.raw_material_rows_restored = tx
            .execute(sqlx::query(
                "UPDATE mini_raw_material_stock
                 SET status = 'available',
                     reserved_order_id = '',
                     payload_json = payload_json - ARRAY[
                         'in_use_order_id',
                         'consumed_order_id',
                         'reserved_order_id'
                     ]::text[],
                     updated_at = now()
                 WHERE lower(barcode) IN (SELECT barcode FROM reset_raw_barcodes)
                    OR lower(reserved_order_id) IN (
                        SELECT lower(id) FROM reset_order_ids
                    )",
            ))
            .await
            .map_err(OrderResetError::StoreFailed)?
            .rows_affected();

        report.qolip_locations_restored = tx
            .execute(sqlx::query(
                "INSERT INTO mini_qolip_locations (
                     id, block, warehouse, item_code, item_name, qolip_code,
                     size, quantity, row_letter, column_number, location_label,
                     created_by_role, created_by_ref, created_by_name, payload_json
                 )
                 SELECT checkout.location_id,
                        checkout.block,
                        checkout.warehouse,
                        checkout.item_code,
                        checkout.item_name,
                        checkout.qolip_code,
                        checkout.size,
                        checkout.quantity,
                        checkout.row_letter,
                        checkout.column_number,
                        checkout.location_label,
                        'system',
                        'order-reset',
                        'Order reset',
                        jsonb_build_object(
                            'restored_from_checkout', checkout.id,
                            'source', 'order_reset'
                        )
                 FROM mini_qolip_checkouts checkout
                 JOIN reset_qolip_codes code
                   ON lower(checkout.qolip_code) = code.qolip_code
                 WHERE lower(checkout.status) = 'open'
                 ON CONFLICT (id) DO UPDATE
                 SET quantity = mini_qolip_locations.quantity + EXCLUDED.quantity,
                     updated_at = now()",
            ))
            .await
            .map_err(OrderResetError::StoreFailed)?
            .rows_affected();

        report.progress_corrections_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_progress_batch_corrections
             WHERE batch_id IN (SELECT batch_id FROM reset_progress_batch_ids)",
        )
        .await?;
        report.qolip_order_notes_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_qolip_order_notes
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        report.qolip_checkouts_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_qolip_checkouts
             WHERE lower(qolip_code) IN (SELECT qolip_code FROM reset_qolip_codes)",
        )
        .await?;
        report.returned_paint_requests_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_returned_paint_requests
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        report.returned_paint_images_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_returned_paint_images
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        report.apparatus_transfers_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_apparatus_order_transfers
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        report.schedule_reservations_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_apparatus_schedule_reservations
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        report.finished_goods_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_finished_goods_stock
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        delete_rows(
            &mut tx,
            "DELETE FROM mini_laminatsiya_astatka_reports
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        delete_rows(
            &mut tx,
            "DELETE FROM mini_rezka_astatka_reports
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        report.raw_material_assignments_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_raw_material_assignments
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        report.raw_material_events_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_raw_material_events
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        report.queue_events_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_queue_action_events
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        report.queue_states_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_queue_states
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        tx.execute(sqlx::query(
            "UPDATE mini_queue_sequences
             SET order_ids = '[]'::jsonb,
                 updated_at = now()",
        ))
        .await
        .map_err(OrderResetError::StoreFailed)?;
        report.run_sessions_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_order_run_sessions
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        report.progress_events_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_order_progress_events
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        delete_rows(
            &mut tx,
            "DELETE FROM mini_paddon_items
             WHERE progress_batch_id IN (SELECT batch_id FROM reset_progress_batch_ids)",
        )
        .await?;
        report.paddons_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_paddons
             WHERE id IN (SELECT paddon_id FROM reset_paddon_ids)",
        )
        .await?;
        report.progress_batches_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_progress_batches
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        report.order_products_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_order_products
             WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        report.production_maps_deleted =
            delete_rows(&mut tx, "DELETE FROM mini_production_maps").await?;
        report.orders_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_orders
             WHERE lower(id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        report.engine_events_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_engine_events
             WHERE lower(entity_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;
        report.idempotency_keys_deleted = delete_rows(
            &mut tx,
            "DELETE FROM mini_idempotency_keys
             WHERE lower(entity_id) IN (SELECT lower(id) FROM reset_order_ids)",
        )
        .await?;

        sqlx::query("ALTER SEQUENCE IF EXISTS mini_production_order_number_seq RESTART WITH 1")
            .execute(&mut *tx)
            .await
            .map_err(OrderResetError::StoreFailed)?;

        let remaining = count_rows(
            &mut tx,
            "SELECT
                 (SELECT COUNT(*) FROM mini_orders
                  WHERE lower(id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_production_maps)
               + (SELECT COUNT(*) FROM mini_order_products
                  WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_queue_states
                  WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_queue_action_events
                  WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_order_run_sessions
                  WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_order_progress_events
                  WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_progress_batches
                  WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_raw_material_assignments
                  WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_raw_material_events
                  WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_qolip_order_notes
                  WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_returned_paint_requests
                  WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_returned_paint_images
                  WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_apparatus_order_transfers
                  WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_apparatus_schedule_reservations
                  WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_finished_goods_stock
                  WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_engine_events
                  WHERE lower(entity_id) IN (SELECT lower(id) FROM reset_order_ids))
               + (SELECT COUNT(*) FROM mini_idempotency_keys
                  WHERE lower(entity_id) IN (SELECT lower(id) FROM reset_order_ids))",
        )
        .await?;
        if remaining != 0 {
            return Err(OrderResetError::VerificationFailed);
        }

        tx.commit().await.map_err(OrderResetError::StoreFailed)?;
        Ok(report)
    }
}
