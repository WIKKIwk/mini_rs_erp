#[async_trait]
impl MaterialReceiptStorePort for PostgresGscaleReceiptStore {
    async fn create_material_receipt_draft(
        &self,
        input: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError> {
        let item_code = input.item_code.trim();
        let warehouse = input.warehouse.trim();
        let barcode = input.barcode.trim();
        if item_code.is_empty() || warehouse.is_empty() || barcode.is_empty() {
            return Err(GscalePortError::InvalidInput(
                "item_code_warehouse_barcode_and_qty_required".to_string(),
            ));
        }
        let qty = positive_erp_quantity(input.qty).ok_or_else(|| {
            GscalePortError::InvalidInput(
                "item_code_warehouse_barcode_and_qty_required".to_string(),
            )
        })?;
        let name = receipt_name(barcode);
        sqlx::query_as::<_, MaterialReceiptRow>(
            "INSERT INTO mini_gscale_receipts (
                 name, status, item_code, warehouse, qty, uom, barcode, payload_json
             )
             VALUES ($1, 'draft', $2, $3,
                     ($4::double precision)::numeric(24,9), 'kg', $5, $6)
             ON CONFLICT (barcode) DO UPDATE SET
               name = excluded.name,
               status = 'draft',
               item_code = excluded.item_code,
               warehouse = excluded.warehouse,
               qty = excluded.qty,
               uom = excluded.uom,
               payload_json = excluded.payload_json,
               updated_at = now(),
               submitted_at = NULL
             RETURNING name, item_code, warehouse, qty::float8 AS qty,
                       uom, barcode, payload_json",
        )
        .bind(name)
        .bind(item_code)
        .bind(warehouse)
        .bind(qty)
        .bind(barcode)
        .bind(serde_json::json!({
            "item_code": item_code,
            "item_name": input.item_name.trim(),
            "warehouse": warehouse,
            "qty": qty,
            "uom": "kg",
            "barcode": barcode,
            "actor_role": input.actor_role.trim(),
            "actor_ref": input.actor_ref.trim(),
            "actor_display_name": input.actor_display_name.trim(),
        }))
        .fetch_one(&self.pool)
        .await
        .map(row_to_draft)
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))
    }

    async fn submit_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        let row = sqlx::query_as::<_, MaterialReceiptRow>(
            "UPDATE mini_gscale_receipts
             SET status = 'submitted', submitted_at = now(), updated_at = now()
             WHERE name = $1 AND status = 'draft'
             RETURNING name, item_code, warehouse, qty::float8 AS qty,
                       uom, barcode, payload_json",
        )
        .bind(name.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        let Some(row) = row else {
            return Err(GscalePortError::StoreWrite(
                "material receipt draft not found".to_string(),
            ));
        };
        let previous_status = raw_material_stock_status_tx(&mut tx, &row.barcode).await?;
        upsert_raw_material_stock_tx(&mut tx, &row).await?;
        insert_raw_material_event_tx(&mut tx, receipt_event_draft(&row, previous_status))
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        tx.commit()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        Ok(())
    }

    async fn material_receipt_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<MaterialReceiptDraft>, GscalePortError> {
        let barcode = barcode.trim();
        if barcode.is_empty() {
            return Err(GscalePortError::InvalidInput(
                "barcode is required".to_string(),
            ));
        }
        sqlx::query_as::<_, MaterialReceiptRow>(
            "SELECT name, item_code, warehouse, qty::float8 AS qty,
                    uom, barcode, payload_json
             FROM mini_gscale_receipts
             WHERE lower(barcode) = lower($1)
             ORDER BY updated_at DESC
             LIMIT 1",
        )
        .bind(barcode)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(row_to_draft))
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))
    }

    async fn raw_material_stock_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<RawMaterialStockEntry>, GscalePortError> {
        let barcode = barcode.trim();
        if barcode.is_empty() {
            return Err(GscalePortError::InvalidInput(
                "barcode is required".to_string(),
            ));
        }
        sqlx::query_as::<_, RawMaterialStockRow>(
            "SELECT id, warehouse, item_code, item_name, barcode,
                    qty::float8 AS qty, uom,
                    status, reserved_order_id, source_receipt_id
             FROM mini_raw_material_stock
             WHERE lower(barcode) = lower($1)
             ORDER BY updated_at DESC
             LIMIT 1",
        )
        .bind(barcode)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(row_to_stock))
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))
    }

    async fn raw_material_stock(
        &self,
        warehouse: &str,
        limit: usize,
    ) -> Result<Vec<RawMaterialStockEntry>, GscalePortError> {
        let warehouse = warehouse.trim().to_lowercase();
        sqlx::query_as::<_, RawMaterialStockRow>(
            "SELECT id, warehouse, item_code, item_name, barcode,
                    qty::float8 AS qty, uom,
                    status, reserved_order_id, source_receipt_id
             FROM mini_raw_material_stock
             WHERE $1 = '' OR lower(warehouse) = $1
             ORDER BY lower(warehouse), lower(item_code), updated_at DESC
             LIMIT $2",
        )
        .bind(warehouse)
        .bind(limit.clamp(1, 500) as i64)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(row_to_stock).collect())
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))
    }

    async fn update_raw_material_stock(
        &self,
        input: RawMaterialStockUpdateInput,
    ) -> Result<RawMaterialStockEntry, GscalePortError> {
        let barcode = input.barcode.trim();
        let item_code = input.item_code.trim();
        let item_name = input.item_name.trim();
        let qty = positive_erp_quantity(input.qty).ok_or_else(|| {
            GscalePortError::InvalidInput("raw_material_stock_qty_invalid".to_string())
        })?;
        if barcode.is_empty() || item_code.is_empty() || item_name.is_empty() {
            return Err(GscalePortError::InvalidInput(
                "raw_material_stock_update_invalid".to_string(),
            ));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        let previous = sqlx::query_as::<_, RawMaterialStockRow>(
            "SELECT id, warehouse, item_code, item_name, barcode,
                    qty::float8 AS qty, uom,
                    status, reserved_order_id, source_receipt_id
             FROM mini_raw_material_stock
             WHERE lower(barcode) = lower($1)
             FOR UPDATE",
        )
        .bind(barcode)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?
        .ok_or_else(|| GscalePortError::InvalidInput("raw_material_stock_not_found".to_string()))?;

        let assignment_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                 SELECT 1
                 FROM mini_raw_material_assignments
                 WHERE lower(barcode) = lower($1)
             )",
        )
        .bind(barcode)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        if !previous.status.trim().eq_ignore_ascii_case("available")
            || !previous.reserved_order_id.trim().is_empty()
            || assignment_exists
        {
            return Err(GscalePortError::InvalidInput(
                "raw_material_stock_locked".to_string(),
            ));
        }

        let updated = sqlx::query_as::<_, RawMaterialStockRow>(
            "UPDATE mini_raw_material_stock
             SET item_code = $2,
                 item_name = $3,
                 qty = ($4::double precision)::numeric(24,9),
                 payload_json = payload_json || jsonb_build_object(
                     'item_code', $2::text,
                     'item_name', $3::text,
                     'qty', ($4::double precision)::numeric(24,9),
                     'corrected_by_role', $5::text,
                     'corrected_by_ref', $6::text,
                     'corrected_by_display_name', $7::text,
                     'corrected_at', now()
                 ),
                 updated_at = now()
             WHERE lower(barcode) = lower($1)
             RETURNING id, warehouse, item_code, item_name, barcode,
                       qty::float8 AS qty, uom,
                       status, reserved_order_id, source_receipt_id",
        )
        .bind(barcode)
        .bind(item_code)
        .bind(item_name)
        .bind(qty)
        .bind(input.actor_role.trim())
        .bind(input.actor_ref.trim())
        .bind(input.actor_display_name.trim())
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;

        sqlx::query(
            "UPDATE mini_gscale_receipts
             SET item_code = $2,
                 qty = ($3::double precision)::numeric(24,9),
                 payload_json = payload_json || jsonb_build_object(
                     'item_code', $2::text,
                     'item_name', $4::text,
                     'qty', ($3::double precision)::numeric(24,9),
                     'corrected_by_role', $5::text,
                     'corrected_by_ref', $6::text,
                     'corrected_by_display_name', $7::text,
                     'corrected_at', now()
                 ),
                 updated_at = now()
             WHERE lower(barcode) = lower($1)
               AND ($8 = '' OR name = $8)",
        )
        .bind(barcode)
        .bind(item_code)
        .bind(qty)
        .bind(item_name)
        .bind(input.actor_role.trim())
        .bind(input.actor_ref.trim())
        .bind(input.actor_display_name.trim())
        .bind(previous.source_receipt_id.trim())
        .execute(&mut *tx)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;

        insert_raw_material_event_tx(
            &mut tx,
            raw_material_stock_correction_event(&previous, &updated, &input),
        )
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        tx.commit()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        Ok(row_to_stock(updated))
    }

    async fn mark_raw_material_stock_in_use(
        &self,
        barcodes: &[String],
        order_id: &str,
    ) -> Result<Vec<RawMaterialStockEntry>, GscalePortError> {
        let barcodes = normalized_unique_barcodes(barcodes);
        let order_id = order_id.trim();
        if barcodes.is_empty() {
            return Ok(Vec::new());
        }
        if order_id.is_empty() {
            return Err(GscalePortError::InvalidInput(
                "order_id is required".to_string(),
            ));
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        let before = raw_material_stock_rows_for_update_tx(&mut tx, &barcodes).await?;
        let rows = sqlx::query_as::<_, RawMaterialStockRow>(
            "UPDATE mini_raw_material_stock
             SET status = 'in_use',
                 reserved_order_id = $2,
                 payload_json = jsonb_set(payload_json, '{in_use_order_id}', to_jsonb($2::text), true),
                 updated_at = now()
             WHERE lower(barcode) = ANY($1)
               AND (status = 'available' OR (status = 'in_use' AND reserved_order_id = $2))
             RETURNING id, warehouse, item_code, item_name, barcode,
                       qty::float8 AS qty, uom,
                       status, reserved_order_id, source_receipt_id",
        )
        .bind(&barcodes)
        .bind(order_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        if rows.len() != barcodes.len() {
            return Err(GscalePortError::InvalidInput(
                "raw_material_stock_unavailable".to_string(),
            ));
        }
        for row in &rows {
            let previous = before.get(&stock_key(&row.barcode));
            insert_raw_material_event_tx(
                &mut tx,
                stock_status_event_draft(
                    "usage_started",
                    row,
                    previous.map(|row| row.status.clone()),
                    Some("in_use".to_string()),
                    order_id,
                    ("system", "system", ""),
                ),
            )
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        Ok(rows.into_iter().map(row_to_stock).collect())
    }

    async fn mark_raw_material_stock_consumed(
        &self,
        barcodes: &[String],
        order_id: &str,
    ) -> Result<Vec<RawMaterialStockEntry>, GscalePortError> {
        let barcodes = normalized_unique_barcodes(barcodes);
        let order_id = order_id.trim();
        if barcodes.is_empty() {
            return Ok(Vec::new());
        }
        if order_id.is_empty() {
            return Err(GscalePortError::InvalidInput(
                "order_id is required".to_string(),
            ));
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        let before = raw_material_stock_rows_for_update_tx(&mut tx, &barcodes).await?;
        let rows = sqlx::query_as::<_, RawMaterialStockRow>(
            "UPDATE mini_raw_material_stock
             SET status = 'consumed',
                 payload_json = jsonb_set(payload_json, '{consumed_order_id}', to_jsonb($2::text), true),
                 updated_at = now()
             WHERE lower(barcode) = ANY($1)
               AND reserved_order_id = $2
               AND status IN ('in_use', 'consumed')
             RETURNING id, warehouse, item_code, item_name, barcode,
                       qty::float8 AS qty, uom,
                       status, reserved_order_id, source_receipt_id",
        )
        .bind(&barcodes)
        .bind(order_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        if rows.len() != barcodes.len() {
            return Err(GscalePortError::InvalidInput(
                "raw_material_stock_unavailable".to_string(),
            ));
        }
        for row in &rows {
            let previous = before.get(&stock_key(&row.barcode));
            insert_raw_material_event_tx(
                &mut tx,
                stock_status_event_draft(
                    "consumption_posted",
                    row,
                    previous.map(|row| row.status.clone()),
                    Some("consumed".to_string()),
                    order_id,
                    ("system", "system", ""),
                ),
            )
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        Ok(rows.into_iter().map(row_to_stock).collect())
    }

    async fn delete_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        sqlx::query("DELETE FROM mini_gscale_receipts WHERE name = $1 AND status = 'draft'")
            .bind(name.trim())
            .execute(&self.pool)
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        Ok(())
    }
}

