impl PostgresGscaleReceiptStore {
    async fn update_stock_entry(
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
                    qty::float8 AS qty, width_mm::float8 AS width_mm,
                    micron::float8 AS micron, length_m::float8 AS length_m, uom,
                    status, reserved_order_id, source_receipt_id, is_metalayzer
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
                 qty = ($4::double precision)::numeric(18,6),
                 payload_json = payload_json || jsonb_build_object(
                     'item_code', $2::text,
                     'item_name', $3::text,
                     'qty', ($4::double precision)::numeric(18,6),
                     'corrected_by_role', $5::text,
                     'corrected_by_ref', $6::text,
                     'corrected_by_display_name', $7::text,
                     'corrected_at', now()
                 ),
                 updated_at = now()
             WHERE lower(barcode) = lower($1)
             RETURNING id, warehouse, item_code, item_name, barcode,
                       qty::float8 AS qty, width_mm::float8 AS width_mm,
                       micron::float8 AS micron, length_m::float8 AS length_m, uom,
                       status, reserved_order_id, source_receipt_id, is_metalayzer",
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
                 qty = ($3::double precision)::numeric(18,6),
                 payload_json = payload_json || jsonb_build_object(
                     'item_code', $2::text,
                     'item_name', $4::text,
                     'qty', ($3::double precision)::numeric(18,6),
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

    async fn soft_delete_stock_entry(
        &self,
        input: RawMaterialStockDeleteInput,
    ) -> Result<RawMaterialStockEntry, GscalePortError> {
        let barcode = input.barcode.trim();
        let expected_warehouse = input.expected_warehouse.trim();
        if barcode.is_empty() || expected_warehouse.is_empty() {
            return Err(GscalePortError::InvalidInput(
                "raw_material_stock_delete_invalid".to_string(),
            ));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        let previous = sqlx::query_as::<_, RawMaterialStockRow>(
            "SELECT id, warehouse, item_code, item_name, barcode,
                    qty::float8 AS qty, width_mm::float8 AS width_mm,
                    micron::float8 AS micron, length_m::float8 AS length_m, uom,
                    status, reserved_order_id, source_receipt_id, is_metalayzer
             FROM mini_raw_material_stock
             WHERE lower(barcode) = lower($1)
             FOR UPDATE",
        )
        .bind(barcode)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?
        .ok_or_else(|| GscalePortError::InvalidInput("raw_material_stock_not_found".to_string()))?;
        if previous.status.trim().eq_ignore_ascii_case("deleted") {
            return Err(GscalePortError::InvalidInput(
                "raw_material_stock_not_found".to_string(),
            ));
        }

        let (assignment_exists, transfer_id, placement_scope): (bool, String, Option<String>) =
            sqlx::query_as(
                "SELECT EXISTS(
                     SELECT 1
                     FROM mini_raw_material_assignments assignment
                     WHERE lower(assignment.barcode) = lower($1)
                 ),
                 COALESCE(stock.payload_json->>'inventory_transfer_id', ''),
                 (
                     SELECT CASE
                         WHEN location.kind = 'warehouse'
                             THEN COALESCE(warehouse.name, '')
                         ELSE location.kind
                     END
                     FROM mini_inventory_placements placement
                     JOIN mini_inventory_locations location
                       ON location.id = placement.physical_location_id
                     LEFT JOIN mini_warehouses warehouse
                       ON warehouse.id = location.warehouse_id
                     WHERE placement.asset_kind = 'raw_material'
                       AND lower(placement.asset_ref) = lower(stock.id)
                 )
                 FROM mini_raw_material_stock stock
                 WHERE stock.id = $2",
            )
            .bind(barcode)
            .bind(&previous.id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        let outside_expected_warehouse = placement_scope
            .as_deref()
            .is_some_and(|scope| !scope.eq_ignore_ascii_case(expected_warehouse));
        if !previous
            .warehouse
            .trim()
            .eq_ignore_ascii_case(expected_warehouse)
            || !previous.status.trim().eq_ignore_ascii_case("available")
            || !previous.reserved_order_id.trim().is_empty()
            || assignment_exists
            || !transfer_id.trim().is_empty()
            || outside_expected_warehouse
        {
            return Err(GscalePortError::InvalidInput(
                "raw_material_stock_locked".to_string(),
            ));
        }

        let deleted = sqlx::query_as::<_, RawMaterialStockRow>(
            "UPDATE mini_raw_material_stock
             SET status = 'deleted',
                 reserved_order_id = '',
                 payload_json = payload_json || jsonb_build_object(
                     'deleted_by_role', $2::text,
                     'deleted_by_ref', $3::text,
                     'deleted_by_display_name', $4::text,
                     'deleted_at', now(),
                     'soft_delete', true
                 ),
                 updated_at = now()
             WHERE id = $1
             RETURNING id, warehouse, item_code, item_name, barcode,
                       qty::float8 AS qty, width_mm::float8 AS width_mm,
                       micron::float8 AS micron, length_m::float8 AS length_m, uom,
                       status, reserved_order_id, source_receipt_id, is_metalayzer",
        )
        .bind(&previous.id)
        .bind(input.actor_role.trim())
        .bind(input.actor_ref.trim())
        .bind(input.actor_display_name.trim())
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;

        sqlx::query(
            "UPDATE mini_gscale_receipts
             SET payload_json = payload_json || jsonb_build_object(
                     'stock_deleted_by_role', $2::text,
                     'stock_deleted_by_ref', $3::text,
                     'stock_deleted_by_display_name', $4::text,
                     'stock_deleted_at', now(),
                     'stock_soft_deleted', true
                 ),
                 updated_at = now()
             WHERE lower(barcode) = lower($1)
               AND ($5 = '' OR name = $5)",
        )
        .bind(barcode)
        .bind(input.actor_role.trim())
        .bind(input.actor_ref.trim())
        .bind(input.actor_display_name.trim())
        .bind(previous.source_receipt_id.trim())
        .execute(&mut *tx)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;

        insert_raw_material_event_tx(
            &mut tx,
            raw_material_stock_delete_event(&previous, &deleted, &input),
        )
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        tx.commit()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        Ok(row_to_stock(deleted))
    }
}
