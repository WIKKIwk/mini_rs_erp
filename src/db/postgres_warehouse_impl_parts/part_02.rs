impl PostgresWarehouseStore {

    async fn put_warehouse_assignment(
        &self,
        assignment: WarehouseAssignment,
    ) -> Result<WarehouseAssignment, WarehouseError> {
        let warehouse = assignment.warehouse.trim();
        let principal_ref = assignment.principal_ref.trim();
        if warehouse.is_empty() {
            return Err(WarehouseError::MissingWarehouse);
        }
        if principal_ref.is_empty() {
            return Err(WarehouseError::MissingPrincipalRef);
        }
        let assignment_kind = assignment.assignment_kind.trim();
        let conflict_target = match assignment_kind {
            "warehouse" => {
                "(warehouse_name, principal_role, principal_ref) WHERE assignment_kind = 'warehouse'"
            }
            "apparatus" => {
                "(apparatus_id, principal_role, principal_ref) WHERE assignment_kind = 'apparatus'"
            }
            _ => return Err(WarehouseError::StoreFailed),
        };
        let query = format!(
            "INSERT INTO mini_warehouse_assignments (
                 assignment_kind, warehouse, warehouse_name, apparatus_id,
                 principal_role, principal_ref, display_name, payload_json
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT {conflict_target} DO UPDATE SET
               warehouse = excluded.warehouse,
               warehouse_name = excluded.warehouse_name,
               apparatus_id = excluded.apparatus_id,
               display_name = excluded.display_name,
               payload_json = excluded.payload_json,
               updated_at = now()
             WHERE mini_warehouse_assignments.assignment_kind = excluded.assignment_kind
               AND (
                    (excluded.assignment_kind = 'warehouse'
                     AND mini_warehouse_assignments.warehouse_name = excluded.warehouse_name)
                    OR
                    (excluded.assignment_kind = 'apparatus'
                     AND mini_warehouse_assignments.apparatus_id = excluded.apparatus_id)
               )
             RETURNING assignment_kind, warehouse, warehouse_name, apparatus_id,
                       principal_role, principal_ref, display_name"
        );
        sqlx::query_as::<_, WarehouseAssignmentRow>(&query)
            .bind(assignment_kind)
            .bind(warehouse)
            .bind(assignment.warehouse_name.as_deref().map(str::trim))
            .bind(assignment.apparatus_id.as_deref().map(str::trim))
            .bind(role_as_str(&assignment.principal_role))
            .bind(principal_ref)
            .bind(assignment.display_name.trim())
            .bind(serde_json::json!({
                "assignment_kind": assignment_kind,
                "warehouse": warehouse,
                "warehouse_name": assignment.warehouse_name.as_deref().map(str::trim),
                "apparatus_id": assignment.apparatus_id.as_deref().map(str::trim),
                "principal_role": role_as_str(&assignment.principal_role),
                "principal_ref": principal_ref,
                "display_name": assignment.display_name.trim(),
            }))
            .fetch_one(&self.pool)
            .await
            .map_err(|_| WarehouseError::StoreFailed)
            .and_then(row_to_assignment)
    }

    async fn delete_warehouse_assignment(
        &self,
        identity: &WarehouseAssignmentIdentity,
        principal_role: &PrincipalRole,
        principal_ref: &str,
    ) -> Result<Option<WarehouseAssignment>, WarehouseError> {
        let (query, identity_value) = match identity {
            WarehouseAssignmentIdentity::WarehouseName(warehouse) => (
                "DELETE FROM mini_warehouse_assignments
                 WHERE assignment_kind = 'warehouse'
                   AND lower(warehouse_name) = lower($1)
                   AND principal_role = $2
                   AND principal_ref = $3
                 RETURNING assignment_kind, warehouse, warehouse_name, apparatus_id,
                           principal_role, principal_ref, display_name",
                warehouse.as_str(),
            ),
            WarehouseAssignmentIdentity::ApparatusId(apparatus_id) => (
                "DELETE FROM mini_warehouse_assignments
                 WHERE assignment_kind = 'apparatus'
                   AND apparatus_id = $1
                   AND principal_role = $2
                   AND principal_ref = $3
                 RETURNING assignment_kind, warehouse, warehouse_name, apparatus_id,
                           principal_role, principal_ref, display_name",
                apparatus_id.as_str(),
            ),
        };
        let row = sqlx::query_as::<_, WarehouseAssignmentRow>(query)
            .bind(identity_value)
            .bind(role_as_str(principal_role))
            .bind(principal_ref.trim())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;

        row.map(row_to_assignment).transpose()
    }

    async fn delete_warehouse(
        &self,
        warehouse: &str,
        delete_products: bool,
    ) -> Result<WarehouseDeleteResult, WarehouseError> {
        let warehouse = warehouse.trim();
        if warehouse.is_empty() {
            return Err(WarehouseError::MissingWarehouse);
        }
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        // Transfer creation holds FOR KEY SHARE on these same rows until its
        // transaction commits, so the delete recheck cannot miss a new transfer.
        let (warehouse_id, warehouse_name) = sqlx::query_as::<_, (String, String)>(
            "SELECT id, name
             FROM mini_warehouses
             WHERE lower(name) = lower($1)
             FOR UPDATE",
        )
        .bind(warehouse)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| WarehouseError::StoreFailed)?
        .ok_or(WarehouseError::NotFound)?;
        let has_children = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                 SELECT 1 FROM mini_warehouses
                 WHERE lower(parent_warehouse) = lower($1)
             )",
        )
        .bind(&warehouse_name)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| WarehouseError::StoreFailed)?;
        if has_children {
            return Err(WarehouseError::HasChildren);
        }
        let snapshot =
            warehouse_delete_snapshot_tx(&mut transaction, &warehouse_id, &warehouse_name).await?;
        let product_count = count_as_usize(snapshot.product_count);
        let reserved_count = count_as_usize(snapshot.reserved_count);
        let assignment_count = count_as_usize(snapshot.assignment_count);
        if reserved_count > 0 {
            return Err(WarehouseError::HasActiveReservations(reserved_count));
        }
        if product_count > 0 && !delete_products {
            return Err(WarehouseError::NotEmpty(product_count));
        }
        for table in ["mini_qolip_cell_qrs", "mini_qolip_locations"] {
            sqlx::query(&format!(
                "DELETE FROM {table}
                 WHERE lower(warehouse) = lower($1) OR lower(block) = lower($1)"
            ))
            .bind(&warehouse_name)
            .execute(&mut *transaction)
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        }
        for table in ["mini_raw_material_stock", "mini_finished_goods_stock"] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE lower(warehouse) = lower($1)"
            ))
            .bind(&warehouse_name)
            .execute(&mut *transaction)
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        }
        sqlx::query(
            "DELETE FROM mini_warehouse_assignments
                     WHERE assignment_kind = 'warehouse'
                       AND lower(warehouse_name) = lower($1)",
        )
        .bind(&warehouse_name)
        .execute(&mut *transaction)
        .await
        .map_err(|_| WarehouseError::StoreFailed)?;
        let deleted = sqlx::query("DELETE FROM mini_warehouses WHERE id = $1")
            .bind(&warehouse_id)
            .execute(&mut *transaction)
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        if deleted.rows_affected() != 1 {
            return Err(WarehouseError::NotFound);
        }
        transaction
            .commit()
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        Ok(WarehouseDeleteResult {
            warehouse: warehouse_name,
            deleted_product_count: product_count,
            deleted_assignment_count: assignment_count,
        })
    }
}
