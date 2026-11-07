use super::*;

impl PostgresPreparationStore {
    /// The same available-stock predicate as the warehouse balance, with a full,
    /// paged material ledger rather than the master's last 100 operations.
    pub async fn material_stock(
        &self,
        owner: &str,
        scopes: &PreparationWarehouseMaterialScopes,
        query: &MaterialStockQuery,
    ) -> Result<Value, PreparationError> {
        query.validate()?;
        let (warehouse, scope) = scopes
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(query.warehouse.trim()))
            .ok_or(PreparationError::Forbidden)?;
        let own_seriyo = matches!(scope, PreparationWarehouseMaterialScope::OwnSeriyo);
        let groups = match scope {
            PreparationWarehouseMaterialScope::OwnSeriyo => Vec::new(),
            PreparationWarehouseMaterialScope::AssignedItemGroups(groups) => groups
                .iter()
                .map(|group| group.trim().to_lowercase())
                .collect::<Vec<_>>(),
        };
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        // Keep the catalog, ownership, explicit warehouse bindings and shared
        // warehouse group scope identical to snapshot_inner.
        let material: Option<(String, String)> = sqlx::query_as(
            "WITH RECURSIVE raw_groups AS (
                SELECT name FROM mini_item_groups WHERE lower(btrim(name))=lower($6) AND is_group
                UNION SELECT child.name FROM mini_item_groups child JOIN raw_groups parent
                  ON lower(btrim(child.parent_item_group))=lower(btrim(parent.name)) WHERE child.is_group
             ), seriyo_groups AS (
                SELECT name FROM mini_item_groups WHERE lower(btrim(name))=lower($7) AND is_group
                UNION SELECT child.name FROM mini_item_groups child JOIN seriyo_groups parent
                  ON lower(btrim(child.parent_item_group))=lower(btrim(parent.name)) WHERE child.is_group
             )
             SELECT i.code,i.name FROM mini_items i
             LEFT JOIN mini_preparation_materials own ON own.item_code=i.code AND own.owner_ref=$1
             WHERE lower(i.code)=lower($3)
               AND (own.item_code IS NOT NULL OR EXISTS(SELECT 1 FROM raw_groups g WHERE lower(btrim(g.name))=lower(btrim(i.item_group))))
               AND (($4 AND own.item_code IS NOT NULL AND EXISTS(SELECT 1 FROM seriyo_groups g WHERE lower(btrim(g.name))=lower(btrim(i.item_group))))
                    OR (NOT $4 AND lower(btrim(i.item_group))=ANY($5)))
               AND (EXISTS(SELECT 1 FROM mini_preparation_material_warehouse_scopes s
                        JOIN mini_warehouses w ON w.id=s.warehouse_id
                        WHERE s.item_code=i.code AND s.active AND lower(w.name)=lower($2))
                    OR (NOT EXISTS(SELECT 1 FROM mini_preparation_material_warehouse_scopes s WHERE s.item_code=i.code)
                        AND (own.warehouse_name IS NULL OR lower(own.warehouse_name)=lower($2))))
               AND EXISTS(SELECT 1 FROM mini_warehouse_assignments a JOIN mini_warehouses w ON lower(w.name)=lower(a.warehouse_name)
                    WHERE a.assignment_kind='warehouse' AND a.principal_role='tayyorlov_masteri'
                      AND a.principal_ref=$1 AND w.name=$2 AND NOT w.is_group)")
            .bind(owner).bind(warehouse).bind(query.item_code.trim()).bind(own_seriyo).bind(groups)
            .bind(PREPARATION_ITEM_GROUP).bind(PREPARATION_MATERIAL_CHILD_GROUP)
            .fetch_optional(&mut *tx).await?;
        let (code, name) = material.ok_or(PreparationError::Forbidden)?;
        let base = format!(
            "WITH available AS (
                SELECT s.* FROM mini_raw_material_stock s
                WHERE s.warehouse=$1 AND lower(s.item_code)=lower($2) AND s.qty>0 AND {AVAILABLE_STOCK}
             ), movements AS (
                SELECT e.* FROM mini_raw_material_events e
                WHERE lower(e.item_code)=lower($2) AND e.qty_delta<>0
                  AND (e.warehouse_key=lower($1) OR EXISTS(
                    SELECT 1 FROM mini_preparation_warehouse_history_names link
                    JOIN mini_warehouses w ON w.id=link.warehouse_id
                    WHERE link.operation_id=e.source_id AND w.name=$1))
             )");
        let summary: Value = sqlx::query_scalar(&format!(
            "{base} SELECT jsonb_build_object(
                'available_kg',COALESCE((SELECT sum(qty) FROM available),0)::text,
                'receipt_kg',COALESCE((SELECT sum(qty_delta) FROM movements WHERE qty_delta>0),0)::text,
                'issue_kg',COALESCE((SELECT sum(-qty_delta) FROM movements WHERE qty_delta<0),0)::text,
                'available_count',(SELECT count(*) FROM available),
                'receipt_count',(SELECT count(*) FROM movements WHERE qty_delta>0),
                'issue_count',(SELECT count(*) FROM movements WHERE qty_delta<0))"))
            .bind(warehouse).bind(&code).fetch_one(&mut *tx).await?;
        let selection = match query.tab {
            MaterialStockTab::Available =>
                "SELECT jsonb_build_object('id',s.id,'barcode',s.barcode,'has_qr',s.barcode NOT LIKE 'PREP-%',
                    'kg',s.qty::text,'created_at',s.created_at,'event_type','available','actor_name','',
                    'order_code','','width_mm',s.width_mm::text,'micron',s.micron::text,'length_m',s.length_m::text)
                 FROM available s ORDER BY s.created_at DESC,s.id DESC LIMIT $3 OFFSET $4",
            MaterialStockTab::Receipts | MaterialStockTab::Issues => {
                let sign = if query.tab == MaterialStockTab::Receipts { ">" } else { "<" };
                // The sign is selected from the enum, never supplied as SQL by a client.
                let sql = format!("{base} SELECT jsonb_build_object('id',e.event_id,'barcode',e.barcode,
                    'has_qr',e.barcode NOT LIKE 'PREP-%','kg',abs(e.qty_delta)::text,'created_at',e.occurred_at,
                    'event_type',e.event_type,'actor_name',e.actor_display_name,
                    'order_code',COALESCE(NULLIF(m.code,''),e.order_id,''),'width_mm',NULL,'micron',NULL,'length_m',NULL)
                    FROM movements e LEFT JOIN mini_production_maps m ON m.id=e.order_id
                    WHERE e.qty_delta {sign} 0 ORDER BY e.occurred_at DESC,e.id DESC LIMIT $3 OFFSET $4");
                return finish_page(tx, sql, warehouse, &code, &name, query, summary).await;
            }
        };
        finish_page(
            tx,
            format!("{base} {selection}"),
            warehouse,
            &code,
            &name,
            query,
            summary,
        )
        .await
    }
}

async fn finish_page(
    mut tx: Transaction<'_, Postgres>,
    sql: String,
    warehouse: &str,
    code: &str,
    name: &str,
    query: &MaterialStockQuery,
    summary: Value,
) -> Result<Value, PreparationError> {
    let mut rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(warehouse)
        .bind(code)
        .bind((query.limit + 1) as i64)
        .bind(query.offset as i64)
        .fetch_all(&mut *tx)
        .await?;
    let has_more = rows.len() > query.limit;
    rows.truncate(query.limit);
    tx.commit().await?;
    Ok(
        json!({"item_code":code,"item_name":name,"warehouse":warehouse,"tab":query.tab,
        "summary":summary,"rows":rows,"offset":query.offset,"limit":query.limit,"has_more":has_more}),
    )
}
