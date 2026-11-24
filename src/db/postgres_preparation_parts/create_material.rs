impl PostgresPreparationStore {
    pub async fn create_material(
        &self,
        actor: &Principal,
        input: MaterialCreate,
    ) -> Result<Value, PreparationError> {
        let name = input.name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() || name.chars().count() > 160 {
            return Err(PreparationError::Invalid(
                "Homashyo nomi 1–160 ta belgidan iborat bo‘lishi kerak",
            ));
        }
        if input.warehouse.trim().is_empty() {
            return Err(PreparationError::Invalid("Ombor tanlanmagan"));
        }
        let request = json!({"kind": "material", "input": &input});
        let mut tx = self.begin(&actor.ref_, &input.request_id).await?;
        if let Some(result) = replay(&mut tx, &actor.ref_, &input.request_id, &request).await? {
            return Ok(result);
        }
        let warehouse = exclusive_warehouse(&mut tx, &actor.ref_, &input.warehouse).await?;
        let warehouse_id: String = sqlx::query_scalar(
            "SELECT id FROM mini_warehouses WHERE lower(name) = lower($1) FOR SHARE",
        )
        .bind(&warehouse)
        .fetch_one(&mut *tx)
        .await?;
        let duplicate: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_preparation_materials WHERE owner_ref = $1 AND name_key = lower($2))")
            .bind(&actor.ref_).bind(&name).fetch_one(&mut *tx).await?;
        if duplicate {
            return Err(PreparationError::Conflict("Bunday homashyo nomi mavjud"));
        }
        let item_group = preparation_material_group(&mut tx).await?;
        let id = new_id();
        let code = format!("PREP-{id}");
        sqlx::query(
            "INSERT INTO mini_items (code, name, uom, item_group, payload_json)
             VALUES ($1, $2, 'kg', $3, $4)",
        )
        .bind(&code)
        .bind(&name)
        .bind(&item_group)
        .bind(json!({"source": "preparation", "owner_ref": actor.ref_,
            "item_group": item_group}))
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO mini_preparation_materials
                 (item_code, owner_ref, name_key, warehouse_name)
             VALUES ($1,$2,lower($3),$4)",
        )
        .bind(&code)
        .bind(&actor.ref_)
        .bind(&name)
        .bind(&warehouse)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO mini_preparation_material_warehouse_scopes
                 (item_code, warehouse_id, scope_kind, created_by_role, created_by_ref)
             VALUES ($1, $2, 'exclusive', 'tayyorlov_masteri', $3)",
        )
        .bind(&code)
        .bind(&warehouse_id)
        .bind(&actor.ref_)
        .execute(&mut *tx)
        .await?;
        let result = json!({"id": id, "kind": "material", "warehouse": warehouse,
            "item_group": item_group, "item_code": code, "name": name});
        record(
            &mut tx,
            &id,
            &actor.ref_,
            &input.request_id,
            "material",
            None,
            request,
            &result,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn receive(
        &self,
        actor: &Principal,
        input: ReceiptCreate,
    ) -> Result<Value, PreparationError> {
        let kg = decimal(&input.kg)?;
        let request = json!({"kind": "receipt", "input": &input});
        let mut tx = self.begin(&actor.ref_, &input.request_id).await?;
        if let Some(result) = replay(&mut tx, &actor.ref_, &input.request_id, &request).await? {
            return Ok(result);
        }
        let warehouse = exclusive_warehouse(&mut tx, &actor.ref_, &input.warehouse)
            .await
            .map_err(|error| match error {
                PreparationError::WarehouseNotExclusive => PreparationError::ReceiptRequiresQr,
                other => other,
            })?;
        let name =
            material_name_in_warehouse(&mut tx, &actor.ref_, &input.item_code, &warehouse).await?;
        let id = new_id();
        let stock_id = format!("raw:prep:{id}");
        let barcode = format!("PREP-{id}");
        sqlx::query(
            "INSERT INTO mini_raw_material_stock
            (id, warehouse, item_code, item_name, barcode, qty, source_receipt_id, payload_json)
            VALUES ($1,$2,$3,$4,$5,$6::text::numeric,$7,$8)",
        )
        .bind(&stock_id)
        .bind(&warehouse)
        .bind(&input.item_code)
        .bind(&name)
        .bind(&barcode)
        .bind(decimal_text(kg))
        .bind(&id)
        .bind(
            json!({"source":"preparation", "owner_ref":actor.ref_, "initial_kg":decimal_text(kg)}),
        )
        .execute(&mut *tx)
        .await?;
        let result = json!({"id":id, "kind":"receipt", "warehouse":warehouse, "item_code":input.item_code,
            "name":name, "kg":decimal_text(kg), "barcode":barcode});
        record(
            &mut tx,
            &id,
            &actor.ref_,
            &input.request_id,
            "receipt",
            None,
            request,
            &result,
        )
        .await?;
        sqlx::query("INSERT INTO mini_preparation_receipts(id,stock_id,item_code,owner_ref,initial_kg) VALUES ($1,$2,$3,$4,$5::text::numeric)")
            .bind(&id).bind(&stock_id).bind(&input.item_code).bind(&actor.ref_).bind(decimal_text(kg))
            .execute(&mut *tx).await?;
        event(
            &mut tx,
            actor,
            &id,
            &warehouse,
            &barcode,
            &input.item_code,
            &name,
            kg,
            None,
            "available",
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    /// GScale/Tarozi sahifasidagi masterga tegishli (exclusive) omborga QR'siz
    /// sodda kirim. Oddiy Tayyorlov kirimi uchun ishlatiladigan Seriyo scope'i
    /// bu yo'lga aralashtirilmaydi: faqat responsibility + Rulon item o'tadi.
    pub async fn receive_gscale_simple(
        &self,
        actor: &Principal,
        input: ReceiptCreate,
    ) -> Result<Value, PreparationError> {
        let kg = decimal(&input.kg)?;
        let item_code = input.item_code.trim().to_string();
        if item_code.is_empty() {
            return Err(PreparationError::Invalid("Homashyo tanlanmagan"));
        }
        let request = json!({"kind": "gscale_simple_receipt", "input": &input});
        let mut tx = self.begin(&actor.ref_, &input.request_id).await?;
        if let Some(result) = replay(&mut tx, &actor.ref_, &input.request_id, &request).await? {
            return Ok(result);
        }
        let warehouse = exclusive_warehouse(&mut tx, &actor.ref_, &input.warehouse)
            .await
            .map_err(|error| match error {
                PreparationError::WarehouseNotExclusive => PreparationError::ReceiptRequiresQr,
                other => other,
            })?;
        let (canonical_code, name) =
            assigned_rulon_item_for_receipt(&mut tx, &actor.ref_, &item_code).await?;
        let id = new_id();
        let stock_id = format!("raw:prep:{id}");
        // Shared stock requires an internal identity, but this value is never
        // returned to the client and is not printed as a QR label.
        let barcode = format!("PREP-{id}");
        sqlx::query(
            "INSERT INTO mini_raw_material_stock
            (id, warehouse, item_code, item_name, barcode, qty, source_receipt_id, payload_json)
            VALUES ($1,$2,$3,$4,$5,$6::text::numeric,$7,$8)",
        )
        .bind(&stock_id)
        .bind(&warehouse)
        .bind(&canonical_code)
        .bind(&name)
        .bind(&barcode)
        .bind(decimal_text(kg))
        .bind(&id)
        .bind(json!({
            "source": "gscale_simple",
            "owner_ref": actor.ref_,
            "initial_kg": decimal_text(kg)
        }))
        .execute(&mut *tx)
        .await?;
        let result = json!({
            "id": id,
            "kind": "receipt",
            "status": "received",
            "warehouse": warehouse,
            "item_code": canonical_code,
            "name": name,
            "kg": decimal_text(kg),
            "qr_printed": false
        });
        record(
            &mut tx,
            &id,
            &actor.ref_,
            &input.request_id,
            "receipt",
            None,
            request,
            &result,
        )
        .await?;
        sqlx::query("INSERT INTO mini_preparation_receipts(id,stock_id,item_code,owner_ref,initial_kg) VALUES ($1,$2,$3,$4,$5::text::numeric)")
            .bind(&id)
            .bind(&stock_id)
            .bind(&canonical_code)
            .bind(&actor.ref_)
            .bind(decimal_text(kg))
            .execute(&mut *tx)
            .await?;
        event(
            &mut tx,
            actor,
            &id,
            &warehouse,
            &barcode,
            &canonical_code,
            &name,
            kg,
            None,
            "available",
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn reverse_receipt(
        &self,
        actor: &Principal,
        mut input: ReceiptReversalCreate,
    ) -> Result<Value, PreparationError> {
        input.receipt_id = input.receipt_id.trim().to_string();
        input.reason = input.reason.split_whitespace().collect::<Vec<_>>().join(" ");
        if input.receipt_id.is_empty() {
            return Err(PreparationError::Invalid("Kirim topilmadi"));
        }
        if input.reason.is_empty() || input.reason.chars().count() > 500 {
            return Err(PreparationError::Invalid(
                "Bekor qilish sababi 1–500 ta belgidan iborat bo‘lishi kerak",
            ));
        }

        let request = json!({"kind": "receipt_reversal", "input": &input});
        let mut tx = self.begin(&actor.ref_, &input.request_id).await?;
        if let Some(result) = replay(&mut tx, &actor.ref_, &input.request_id, &request).await? {
            return Ok(result);
        }

        let row = sqlx::query(
            "SELECT receipt.id AS receipt_id, receipt.stock_id, receipt.item_code,
                    receipt.initial_kg::text AS initial_kg,
                    stock.warehouse, stock.item_name, stock.barcode,
                    stock.qty::text AS current_kg, stock.status,
                    stock.reserved_order_id,
                    EXISTS (
                        SELECT 1 FROM mini_raw_material_assignments assignment
                        WHERE lower(assignment.barcode) = lower(stock.barcode)
                    ) AS assignment_exists,
                    EXISTS (
                        SELECT 1 FROM mini_preparation_allocations allocation
                        WHERE allocation.receipt_id = receipt.id
                    ) AS allocation_exists,
                    EXISTS (
                        SELECT 1 FROM mini_raw_material_splits split
                        WHERE split.parent_stock_id = stock.id
                    ) OR EXISTS (
                        SELECT 1 FROM mini_raw_material_split_outputs split_output
                        WHERE split_output.stock_id = stock.id
                    ) AS split_exists,
                    EXISTS (
                        SELECT 1
                        FROM mini_inventory_placements placement
                        JOIN mini_inventory_locations location
                          ON location.id = placement.physical_location_id
                        LEFT JOIN mini_warehouses warehouse
                          ON warehouse.id = location.warehouse_id
                        WHERE placement.asset_kind = 'raw_material'
                          AND lower(placement.asset_ref) = lower(stock.id)
                          AND (
                              location.kind <> 'warehouse'
                              OR warehouse.name IS NULL
                              OR lower(warehouse.name) <> lower(stock.warehouse)
                          )
                    ) AS placement_outside_warehouse,
                    EXISTS (
                        SELECT 1
                        FROM mini_raw_material_events event
                        WHERE lower(event.barcode) = lower(stock.barcode)
                          AND event.event_type <> 'receipt_posted'
                    ) OR stock.payload_json->>'paddon_issue_id' IS NOT NULL AS downstream_event_exists,
                    EXISTS (
                        SELECT 1
                        FROM mini_preparation_receipt_reversals reversal
                        WHERE reversal.receipt_id = receipt.id
                    ) AS reversal_exists
             FROM mini_preparation_receipts receipt
             JOIN mini_raw_material_stock stock ON stock.id = receipt.stock_id
             WHERE receipt.id = $1 AND receipt.owner_ref = $2
             FOR UPDATE OF receipt, stock",
        )
        .bind(&input.receipt_id)
        .bind(&actor.ref_)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(PreparationError::Invalid("Kirim topilmadi"))?;

        let receipt_id: String = row.try_get("receipt_id")?;
        let stock_id: String = row.try_get("stock_id")?;
        let item_code: String = row.try_get("item_code")?;
        let initial_kg_text: String = row.try_get("initial_kg")?;
        let current_kg_text: String = row.try_get("current_kg")?;
        let initial_kg = decimal(&initial_kg_text)?;
        let current_kg = decimal(&current_kg_text)?;
        let warehouse: String = row.try_get("warehouse")?;
        let item_name: String = row.try_get("item_name")?;
        let barcode: String = row.try_get("barcode")?;
        let status: String = row.try_get("status")?;
        let reserved_order_id: String = row.try_get("reserved_order_id")?;
        let reversal_exists: bool = row.try_get("reversal_exists")?;
        if reversal_exists {
            return Err(PreparationError::Conflict(
                "Bu kirim allaqachon bekor qilingan",
            ));
        }

        assigned_warehouse(&mut tx, &actor.ref_, &warehouse).await?;
        let locked = !status.eq_ignore_ascii_case("available")
            || !reserved_order_id.trim().is_empty()
            || current_kg != initial_kg
            || row.try_get::<bool, _>("assignment_exists")?
            || row.try_get::<bool, _>("allocation_exists")?
            || row.try_get::<bool, _>("split_exists")?
            || row.try_get::<bool, _>("placement_outside_warehouse")?
            || row.try_get::<bool, _>("downstream_event_exists")?;
        if locked {
            return Err(PreparationError::Conflict(
                "Bu kirim ishlatilgan yoki orderga ulangan, bekor qilib bo‘lmaydi",
            ));
        }

        let reversal_id = new_id();
        sqlx::query(
            "UPDATE mini_raw_material_stock
             SET status = 'deleted', reserved_order_id = '',
                 payload_json = payload_json || jsonb_build_object(
                     'soft_delete', true,
                     'receipt_reversal', true,
                     'receipt_reversal_operation_id', $2::text,
                     'receipt_reversal_reason', $3::text,
                     'receipt_reversed_by_role', 'tayyorlov_masteri',
                     'receipt_reversed_by_ref', $4::text,
                     'receipt_reversed_by_display_name', $5::text,
                     'receipt_reversed_at', now()
                 ),
                 updated_at = now()
             WHERE id = $1 AND status = 'available'",
        )
        .bind(&stock_id)
        .bind(&reversal_id)
        .bind(&input.reason)
        .bind(&actor.ref_)
        .bind(&actor.display_name)
        .execute(&mut *tx)
        .await?;

        let event_id = format!("prep:receipt-reversal:{reversal_id}:{barcode}");
        sqlx::query(
            "INSERT INTO mini_raw_material_events
                (event_id, idempotency_key, event_type, warehouse, barcode, item_code,
                 item_name, qty_delta, stock_status_before, stock_status_after, order_id,
                 actor_role, actor_ref, actor_display_name, owner_role, owner_ref,
                 owner_display_name, source_type, source_id, source_line_ref,
                 correlation_id, payload_json)
             VALUES ($1, $1, 'stock_deleted', $2, $3, $4, $5, $6::text::numeric,
                     'available', 'deleted', NULL, 'tayyorlov_masteri', $7, $8,
                     'tayyorlov_masteri', $7, $8, 'stock_delete', $9, $10, $10, $11)",
        )
        .bind(&event_id)
        .bind(&warehouse)
        .bind(&barcode)
        .bind(&item_code)
        .bind(&item_name)
        .bind(format!("-{}", decimal_text(initial_kg)))
        .bind(&actor.ref_)
        .bind(&actor.display_name)
        .bind(&reversal_id)
        .bind(&receipt_id)
        .bind(json!({
            "source": "preparation_receipt_reversal",
            "receipt_id": receipt_id.clone(),
            "stock_id": stock_id.clone(),
            "reversal_operation_id": reversal_id.clone(),
            "reversed_kg": decimal_text(initial_kg),
            "reason": input.reason.clone(),
        }))
        .execute(&mut *tx)
        .await?;

        let result = json!({
            "id": reversal_id,
            "kind": "receipt_reversal",
            "status": "cancelled",
            "receipt_id": receipt_id,
            "warehouse": warehouse,
            "item_code": item_code,
            "name": item_name,
            "kg": decimal_text(initial_kg),
            "reason": input.reason,
        });
        record(
            &mut tx,
            &reversal_id,
            &actor.ref_,
            &input.request_id,
            "receipt_reversal",
            None,
            request,
            &result,
        )
        .await?;
        sqlx::query(
            "INSERT INTO mini_preparation_receipt_reversals
                (operation_id, receipt_id, stock_id, owner_ref, reversed_kg, reason)
             VALUES ($1, $2, $3, $4, $5::text::numeric, $6)",
        )
        .bind(&reversal_id)
        .bind(&receipt_id)
        .bind(&stock_id)
        .bind(&actor.ref_)
        .bind(decimal_text(initial_kg))
        .bind(&input.reason)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(result)
    }
}
