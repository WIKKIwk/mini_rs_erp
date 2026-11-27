impl PostgresPreparationStore {
    pub async fn list_responsibilities(&self, owner: &str) -> Result<Value, PreparationError> {
        let rows: Vec<Value> = sqlx::query_scalar(
            "SELECT jsonb_build_object('material_id', material_id, 'material_name', material_name)
             FROM mini_preparation_material_responsibilities
             WHERE principal_role = 'tayyorlov_masteri' AND principal_ref = $1
             ORDER BY lower(material_name), lower(material_id)",
        )
        .bind(owner.trim())
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({"principal_ref": owner.trim(), "materials": rows}))
    }

    pub async fn assign_responsibility(
        &self,
        input: MaterialResponsibilityAssign,
    ) -> Result<Value, PreparationError> {
        let principal_ref = input.principal_key()?;
        let material_id = input.material_key()?;
        // Faqat mavjud tayyorlov_masteri system user'ga biriktirish.
        let is_master: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mini_system_users WHERE id = $1 AND role = 'tayyorlov_masteri')",
        )
        .bind(&principal_ref)
        .fetch_one(&self.pool)
        .await?;
        if !is_master {
            return Err(PreparationError::Invalid("Tayyorlov masteri topilmadi"));
        }
        // material_id calculate katalogidan bo'lishi shart (micron'siz, oila).
        // Katalog list() dagi kabi: avval DB override, keyin builtin defaultlar.
        let material_name = calculate_material_name_by_id(&self.pool, &material_id)
            .await?
            .ok_or(PreparationError::Invalid("Homashyo katalogda topilmadi"))?;
        sqlx::query(
            "INSERT INTO mini_preparation_material_responsibilities
                 (principal_role, principal_ref, material_id, material_name)
             VALUES ('tayyorlov_masteri', $1, $2, $3)
             ON CONFLICT (principal_role, principal_ref, material_id)
             DO UPDATE SET material_name = EXCLUDED.material_name",
        )
        .bind(&principal_ref)
        .bind(material_id.trim())
        .bind(&material_name)
        .execute(&self.pool)
        .await?;
        Ok(
            json!({"principal_ref": principal_ref, "material_id": material_id.trim(), "material_name": material_name}),
        )
    }

    pub async fn unassign_responsibility(        &self,
        input: MaterialResponsibilityDelete,
    ) -> Result<Value, PreparationError> {
        let principal_ref = input.principal_key()?;
        let material_id = input.material_key()?;
        let deleted: bool = sqlx::query_scalar(
            "DELETE FROM mini_preparation_material_responsibilities
             WHERE principal_role = 'tayyorlov_masteri' AND principal_ref = $1
               AND lower(material_id) = lower($2) RETURNING TRUE",
        )
        .bind(&principal_ref)
        .bind(&material_id)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(false);
        if !deleted {
            return Err(PreparationError::Invalid("Biriktirish topilmadi"));
        }
        Ok(json!({"principal_ref": principal_ref, "material_id": material_id, "deleted": true}))
    }

    /// Master'ga biriktirilgan homashyo oilalari (kichik harfda id + nom).
    /// GScale katalog filtri uchun.
    pub async fn assigned_material_names(
        &self,
        owner: &str,
    ) -> Result<Vec<(String, String)>, PreparationError> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT lower(material_id), lower(material_name)
             FROM mini_preparation_material_responsibilities
             WHERE principal_role = 'tayyorlov_masteri' AND principal_ref = $1",
        )
        .bind(owner.trim())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Tayyorlov masteri kirimi uchun faqat o'zi yaratgan Seriyo guruhidagi
    /// itemlar. Bu katalog Rulon/ERP umumiy katalogidan mustaqil bo'lib,
    /// oddiy kirim va tarozi kirimida bir xil scope beradi.
    pub async fn owned_seriyo_items(
        &self,
        owner: &str,
        search: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, PreparationError> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "WITH RECURSIVE seriyo_groups AS (
                SELECT g.name
                FROM mini_item_groups g
                WHERE lower(btrim(g.name)) = lower($1) AND g.is_group
                UNION
                SELECT child.name
                FROM mini_item_groups child
                JOIN seriyo_groups parent
                  ON lower(btrim(child.parent_item_group)) = lower(btrim(parent.name))
                WHERE child.is_group
            )
            SELECT i.code, i.name, i.uom, i.item_group
            FROM mini_preparation_materials owned
            JOIN mini_items i ON i.code = owned.item_code
            WHERE owned.owner_ref = $2
              AND EXISTS (
                  SELECT 1 FROM seriyo_groups allowed
                  WHERE lower(btrim(allowed.name)) = lower(btrim(i.item_group))
              )
              AND (
                  btrim($3) = ''
                  OR lower(i.code) LIKE '%' || lower(btrim($3)) || '%'
                  OR lower(i.name) LIKE '%' || lower(btrim($3)) || '%'
              )
            ORDER BY lower(i.name), i.code
            LIMIT $4 OFFSET $5",
        )
        .bind(PREPARATION_MATERIAL_CHILD_GROUP)
        .bind(owner.trim())
        .bind(search.trim())
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(code, name, uom, item_group)| SupplierItem {
                code,
                name,
                uom,
                warehouse: String::new(),
                item_group,
                customer_names: Vec::new(),
            })
            .collect())
    }

    /// GScale/Tarozi kirimi uchun admin biriktirgan calculate-material
    /// oilalariga mos ERP Rulon itemlari. Seriyo va boshqa guruhlar bu scope'ga
    /// kirmaydi.
    pub async fn assigned_rulon_items(
        &self,
        owner: &str,
        search: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, PreparationError> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "WITH RECURSIVE rulon_groups AS (
                SELECT g.name
                FROM mini_item_groups g
                WHERE lower(btrim(g.name)) IN ('rulon', 'rulon materiallari')
                  AND g.is_group
                UNION
                SELECT child.name
                FROM mini_item_groups child
                JOIN rulon_groups parent
                  ON lower(btrim(child.parent_item_group)) = lower(btrim(parent.name))
                WHERE child.is_group
            )
            SELECT i.code, i.name, i.uom, i.item_group
            FROM mini_items i
            WHERE EXISTS (
                SELECT 1
                FROM mini_preparation_material_responsibilities r
                WHERE r.principal_role = 'tayyorlov_masteri'
                  AND r.principal_ref = $1
                  AND (
                      lower(btrim(r.material_name)) = lower(btrim(i.code))
                      OR lower(btrim(r.material_name)) = lower(btrim(i.name))
                      OR lower(btrim(r.material_id)) = lower(btrim(i.code))
                      OR lower(btrim(r.material_id)) = lower(btrim(i.name))
                  )
            )
              AND EXISTS (
                  SELECT 1
                  FROM rulon_groups allowed
                  WHERE lower(btrim(allowed.name)) = lower(btrim(i.item_group))
              )
              AND (
                  btrim($2) = ''
                  OR lower(i.code) LIKE '%' || lower(btrim($2)) || '%'
                  OR lower(i.name) LIKE '%' || lower(btrim($2)) || '%'
              )
            ORDER BY lower(i.name), i.code
            LIMIT $3 OFFSET $4",
        )
        .bind(owner.trim())
        .bind(search.trim())
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(code, name, uom, item_group)| SupplierItem {
                code,
                name,
                uom,
                warehouse: String::new(),
                item_group,
                customer_names: Vec::new(),
            })
            .collect())
    }

    /// GScale kirimida yuborilgan itemni server-side responsibility scope'da
    /// qayta tekshirish uchun canonical item.
    pub async fn assigned_rulon_item(
        &self,
        owner: &str,
        item_code: &str,
    ) -> Result<Option<SupplierItem>, PreparationError> {
        let row: Option<(String, String, String, String)> = sqlx::query_as(
            "WITH RECURSIVE rulon_groups AS (
                SELECT g.name
                FROM mini_item_groups g
                WHERE lower(btrim(g.name)) IN ('rulon', 'rulon materiallari')
                  AND g.is_group
                UNION
                SELECT child.name
                FROM mini_item_groups child
                JOIN rulon_groups parent
                  ON lower(btrim(child.parent_item_group)) = lower(btrim(parent.name))
                WHERE child.is_group
            )
            SELECT i.code, i.name, i.uom, i.item_group
            FROM mini_items i
            WHERE lower(btrim(i.code)) = lower(btrim($2))
              AND EXISTS (
                  SELECT 1
                  FROM mini_preparation_material_responsibilities r
                  WHERE r.principal_role = 'tayyorlov_masteri'
                    AND r.principal_ref = $1
                    AND (
                        lower(btrim(r.material_name)) = lower(btrim(i.code))
                        OR lower(btrim(r.material_name)) = lower(btrim(i.name))
                        OR lower(btrim(r.material_id)) = lower(btrim(i.code))
                        OR lower(btrim(r.material_id)) = lower(btrim(i.name))
                    )
              )
              AND EXISTS (
                  SELECT 1
                  FROM rulon_groups allowed
                  WHERE lower(btrim(allowed.name)) = lower(btrim(i.item_group))
              )
            LIMIT 1",
        )
        .bind(owner.trim())
        .bind(item_code.trim())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(code, name, uom, item_group)| SupplierItem {
            code,
            name,
            uom,
            warehouse: String::new(),
            item_group,
            customer_names: Vec::new(),
        }))
    }

    /// Order shu master'ning biriktirilgan homashyolaridan birini
    /// o'z ichiga oladimi (raw-material ulash scope tekshiruvi uchun).
    pub async fn order_in_scope(        &self,
        owner: &str,
        order_id: &str,
    ) -> Result<bool, PreparationError> {
        let mut tx = self.pool.begin().await?;
        let matched = order_matches_responsibility(&mut tx, owner, order_id).await?;
        tx.rollback().await?;
        Ok(matched)
    }

    async fn begin(
        &self,
        owner: &str,
        request_id: &str,
    ) -> Result<Transaction<'_, Postgres>, PreparationError> {
        if !(8..=128).contains(&request_id.len())
            || !request_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_:".contains(&c))
        {
            return Err(PreparationError::Invalid("request_id noto‘g‘ri"));
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '5s'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("preparation:{owner}"))
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }
}

fn new_id() -> String {
    data_encoding::HEXLOWER.encode(&rand::random::<[u8; 16]>())
}

async fn replay(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    key: &str,
    request: &Value,
) -> Result<Option<Value>, PreparationError> {
    let row = sqlx::query("SELECT request_json, response_json FROM mini_preparation_operations WHERE owner_ref=$1 AND request_id=$2")
        .bind(owner).bind(key).fetch_optional(&mut **tx).await?;
    match row {
        Some(row) if row.try_get::<Value, _>("request_json")? != *request => Err(
            PreparationError::Conflict("request_id boshqa operatsiya uchun ishlatilgan"),
        ),
        Some(row) => Ok(Some(row.try_get("response_json")?)),
        None => Ok(None),
    }
}

async fn assigned_warehouse(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    name: &str,
) -> Result<String, PreparationError> {
    let (_, name, _) = assigned_warehouse_binding(tx, owner, name).await?;
    Ok(name)
}

async fn assigned_warehouse_binding(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    name: &str,
) -> Result<(String, String, bool), PreparationError> {
    sqlx::query_as(
        "SELECT w.id, w.name, NOT EXISTS (
            SELECT 1 FROM mini_warehouse_assignments other
            WHERE other.assignment_kind='warehouse'
              AND lower(other.warehouse_name)=lower(w.name)
              AND (other.principal_role <> 'tayyorlov_masteri' OR other.principal_ref <> $1)
        ) FROM mini_warehouses w JOIN mini_warehouse_assignments a
            ON lower(a.warehouse_name)=lower(w.name)
        WHERE a.assignment_kind='warehouse' AND a.principal_role='tayyorlov_masteri'
          AND a.principal_ref=$1 AND lower(w.name)=lower($2) AND NOT w.is_group FOR SHARE OF w, a",
    )
    .bind(owner)
    .bind(name.trim())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PreparationError::Forbidden)
}

async fn exclusive_warehouse(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    name: &str,
) -> Result<String, PreparationError> {
    // Lock the assignment set, including absent competing rows: a concurrent
    // assignment must not slip between the exclusivity check and stock write.
    sqlx::query("LOCK TABLE mini_warehouse_assignments IN SHARE MODE")
        .execute(&mut **tx)
        .await?;
    match assigned_warehouse_binding(tx, owner, name).await {
        Ok((_, warehouse, true)) => Ok(warehouse),
        Ok(_) | Err(PreparationError::Forbidden) => Err(PreparationError::WarehouseNotExclusive),
        Err(error) => Err(error),
    }
}

async fn preparation_material_group(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<String, PreparationError> {
    // The group is shared by all Tayyorlov masters. Serialize the ensure path
    // so two first-time material creations cannot race on the same group.
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
             hashtextextended('preparation:seriyo-material-group', 0)
         )",
    )
    .execute(&mut **tx)
    .await?;

    if let Some((name, parent, is_group)) = sqlx::query_as::<_, (String, String, bool)>(
        "SELECT name, COALESCE(parent_item_group, ''), is_group
         FROM mini_item_groups
         WHERE lower(name) = lower($1)
         ORDER BY (name = $1) DESC, name
         LIMIT 1
         FOR UPDATE",
    )
    .bind(PREPARATION_MATERIAL_CHILD_GROUP)
    .fetch_optional(&mut **tx)
    .await?
    {
        if !is_group || !parent.eq_ignore_ascii_case(PREPARATION_ITEM_GROUP) {
            return Err(PreparationError::Conflict(
                "Seriyo guruhi Homashyo guruhi ostida emas",
            ));
        }
        return Ok(name);
    }

    let parent = sqlx::query_scalar::<_, String>(
        "SELECT name FROM mini_item_groups
         WHERE lower(name) = lower($1) AND is_group
         ORDER BY (name = $1) DESC, name
         LIMIT 1
         FOR SHARE",
    )
    .bind(PREPARATION_ITEM_GROUP)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PreparationError::StoreFailed)?;
    let name = sqlx::query_scalar::<_, String>(
        "INSERT INTO mini_item_groups
             (name, parent_item_group, is_group, payload_json, updated_at)
         VALUES ($1, $2, TRUE,
             jsonb_build_object(
                 'name', $1,
                 'item_group_name', $1,
                 'parent_item_group', $2,
                 'is_group', TRUE
             ), now())
         RETURNING name",
    )
    .bind(PREPARATION_MATERIAL_CHILD_GROUP)
    .bind(parent)
    .fetch_one(&mut **tx)
    .await?;
    Ok(name)
}

async fn material_name(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    code: &str,
) -> Result<String, PreparationError> {
    sqlx::query_scalar(
        "SELECT i.name FROM mini_preparation_materials m JOIN mini_items i ON i.code=m.item_code
        WHERE m.owner_ref=$1 AND m.item_code=$2 FOR SHARE OF i",
    )
    .bind(owner)
    .bind(code)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PreparationError::Forbidden)
}
