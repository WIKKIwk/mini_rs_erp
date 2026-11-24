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
        let length_m = match input.length_m {
            None => None,
            Some(length_m) if length_m.is_finite() && length_m > 0.0 => Some(length_m),
            Some(_) => {
                return Err(GscalePortError::InvalidInput(
                    "length_m_invalid".to_string(),
                ));
            }
        };
        let deleted_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                 SELECT 1
                 FROM mini_raw_material_stock
                 WHERE lower(barcode) = lower($1)
                   AND status = 'deleted'
             )",
        )
        .bind(barcode)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        if deleted_exists {
            return Err(GscalePortError::InvalidInput(
                "raw_material_stock_deleted".to_string(),
            ));
        }
        let name = receipt_name(barcode);
        sqlx::query_as::<_, MaterialReceiptRow>(
            "INSERT INTO mini_gscale_receipts (
                 name, status, item_code, warehouse, qty, width_mm, micron, length_m,
                 uom, barcode, payload_json
             )
             VALUES ($1, 'draft', $2, $3,
                     ($4::double precision)::numeric(18,6),
                     ($5::double precision)::numeric(18,6),
                     ($6::double precision)::numeric(18,6),
                     ($7::double precision)::numeric(18,6), 'kg', $8, $9)
             ON CONFLICT (barcode) DO UPDATE SET
               name = excluded.name,
               status = 'draft',
               item_code = excluded.item_code,
               warehouse = excluded.warehouse,
               qty = excluded.qty,
               width_mm = excluded.width_mm,
               micron = excluded.micron,
               length_m = excluded.length_m,
               uom = excluded.uom,
               payload_json = excluded.payload_json,
               updated_at = now(),
               submitted_at = NULL
             RETURNING name, item_code, warehouse, qty::float8 AS qty,
                       width_mm::float8 AS width_mm, micron::float8 AS micron,
                       length_m::float8 AS length_m,
                       uom, barcode, payload_json",
        )
        .bind(name)
        .bind(item_code)
        .bind(warehouse)
        .bind(qty)
        .bind(input.width_mm)
        .bind(input.micron)
        .bind(length_m)
        .bind(barcode)
        .bind(serde_json::json!({
            "item_code": item_code,
            "item_name": input.item_name.trim(),
            "warehouse": warehouse,
            "qty": qty,
            "width_mm": input.width_mm,
            "micron": input.micron,
            "length_m": length_m,
            "uom": "kg",
            "barcode": barcode,
            "actor_role": input.actor_role.trim(),
            "actor_ref": input.actor_ref.trim(),
            "actor_display_name": input.actor_display_name.trim(),
            "order_assignment": input.order_assignment,
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
                       width_mm::float8 AS width_mm, micron::float8 AS micron,
                       length_m::float8 AS length_m,
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
        if let Some(mut assignment) = row.payload_json.get("order_assignment").filter(|value| value.is_object()).cloned() {
            assignment["barcode"] = serde_json::json!(row.barcode);
            assignment["item_name"] = serde_json::json!(row_item_name(&row));
            sqlx::query("INSERT INTO mini_raw_material_assignments
                (barcode,order_id,apparatus,canonical_apparatus_id,item_code,item_group,payload_json)
                VALUES($1,$2,$3,$3,$4,$5,$6)")
                .bind(&row.barcode).bind(assignment["order_id"].as_str())
                .bind(assignment["apparatus"].as_str()).bind(&row.item_code)
                .bind(assignment["item_group"].as_str()).bind(&assignment)
                .execute(&mut *tx).await
                .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        }
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
                    width_mm::float8 AS width_mm, micron::float8 AS micron,
                    length_m::float8 AS length_m,
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
                    qty::float8 AS qty, width_mm::float8 AS width_mm,
                    micron::float8 AS micron, length_m::float8 AS length_m, uom,
                    status, reserved_order_id, source_receipt_id, is_metalayzer
             FROM mini_raw_material_stock
             WHERE lower(barcode) = lower($1)
               AND status <> 'deleted'
             ORDER BY updated_at DESC
             LIMIT 1",
        )
        .bind(barcode)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(row_to_stock))
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))
    }

    async fn mark_raw_material_metalayzer(
        &self,
        barcode: &str,
        warehouses: &[String],
    ) -> Result<RawMaterialStockEntry, GscalePortError> {
        let mut tx = self.pool.begin().await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        let mut row = sqlx::query_as::<_, RawMaterialStockRow>(
            "SELECT id, warehouse, item_code, item_name, barcode,
                    qty::float8 AS qty, width_mm::float8 AS width_mm,
                    micron::float8 AS micron, length_m::float8 AS length_m, uom,
                    status, reserved_order_id, source_receipt_id, is_metalayzer
             FROM mini_raw_material_stock WHERE lower(barcode) = lower($1) FOR UPDATE"
        ).bind(barcode.trim()).fetch_optional(&mut *tx).await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?
            .ok_or_else(|| GscalePortError::InvalidInput("raw_material_stock_not_found".into()))?;
        if !warehouses.iter().any(|warehouse| warehouse.trim().eq_ignore_ascii_case(row.warehouse.trim())) {
            return Err(GscalePortError::InvalidInput("metalayzer_warehouse_forbidden".into()));
        }
        if row.status == "deleted" {
            return Err(GscalePortError::InvalidInput("raw_material_stock_not_found".into()));
        }
        if row.status == "consumed" || row.qty <= 0.0 {
            return Err(GscalePortError::InvalidInput("metalayzer_material_unavailable".into()));
        }
        // A repeated request succeeds without rewriting the row or emitting live changes.
        if !row.is_metalayzer {
            sqlx::query("UPDATE mini_raw_material_stock SET is_metalayzer = true, updated_at = now() WHERE id = $1")
                .bind(&row.id).execute(&mut *tx).await
                .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
            row.is_metalayzer = true;
        }
        tx.commit().await.map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        Ok(row_to_stock(row))
    }

    async fn raw_material_stock(
        &self,
        warehouse: &str,
        limit: usize,
    ) -> Result<Vec<RawMaterialStockEntry>, GscalePortError> {
        let warehouse = warehouse.trim().to_lowercase();
        sqlx::query_as::<_, RawMaterialStockRow>(
            "SELECT id, warehouse, item_code, item_name, barcode,
                    qty::float8 AS qty, width_mm::float8 AS width_mm,
                    micron::float8 AS micron, length_m::float8 AS length_m, uom,
                    status, reserved_order_id, source_receipt_id, is_metalayzer
             FROM mini_raw_material_stock
             WHERE status <> 'deleted'
               AND ($1 = '' OR lower(warehouse) = $1)
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

    async fn metalayzer_materials(
        &self,
        warehouses: &[String],
        after_id: &str,
        limit: usize,
    ) -> Result<Vec<RawMaterialStockEntry>, GscalePortError> {
        let warehouses: Vec<String> = warehouses
            .iter()
            .map(|name| name.trim().to_lowercase())
            .filter(|name| !name.is_empty())
            .collect();
        if warehouses.is_empty() {
            return Ok(Vec::new());
        }
        sqlx::query_as::<_, RawMaterialStockRow>(
            "SELECT id, warehouse, item_code, item_name, barcode,
                    qty::float8 AS qty, width_mm::float8 AS width_mm,
                    micron::float8 AS micron, length_m::float8 AS length_m, uom,
                    status, reserved_order_id, source_receipt_id, is_metalayzer
             FROM mini_raw_material_stock
             WHERE is_metalayzer = true AND status <> 'deleted'
               AND lower(btrim(warehouse)) = ANY($1::text[])
               AND id COLLATE \"C\" > $2 COLLATE \"C\"
             ORDER BY id COLLATE \"C\"
             LIMIT $3",
        )
        .bind(&warehouses)
        .bind(after_id)
        .bind(limit.clamp(1, 101) as i64)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(row_to_stock).collect())
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))
    }

    async fn update_raw_material_stock(
        &self,
        input: RawMaterialStockUpdateInput,
    ) -> Result<RawMaterialStockEntry, GscalePortError> {
        self.update_stock_entry(input).await
    }

    async fn soft_delete_raw_material_stock(
        &self,
        input: RawMaterialStockDeleteInput,
    ) -> Result<RawMaterialStockEntry, GscalePortError> {
        self.soft_delete_stock_entry(input).await
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
                       qty::float8 AS qty, width_mm::float8 AS width_mm,
                       micron::float8 AS micron, length_m::float8 AS length_m, uom,
                       status, reserved_order_id, source_receipt_id, is_metalayzer",
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
                       qty::float8 AS qty, width_mm::float8 AS width_mm,
                       micron::float8 AS micron, length_m::float8 AS length_m, uom,
                       status, reserved_order_id, source_receipt_id, is_metalayzer",
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

include!("postgres_gscale_receipt_store_parts/update_stock_entry.rs");
