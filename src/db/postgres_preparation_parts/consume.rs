impl PostgresPreparationStore {
    pub async fn consume(
        &self,
        actor: &Principal,
        input: ConsumptionCreate,
    ) -> Result<Value, PreparationError> {
        let expected_kg = decimal(&input.expected_order_kg)?;
        let quantities = input.quantities(expected_kg)?;
        let request = json!({"kind": "consumption", "input": &input});
        let mut tx = self.begin(&actor.ref_, &input.request_id).await?;
        if let Some(result) = replay(&mut tx, &actor.ref_, &input.request_id, &request).await? {
            return Ok(result);
        }
        let warehouse = assigned_warehouse(&mut tx, &actor.ref_, &input.warehouse).await?;
        let order = sqlx::query(
            "SELECT code, title, lifecycle_status,
                round((map_json->>'order_kg')::numeric, 6)::text AS kg
             FROM mini_production_maps WHERE id = $1 FOR UPDATE",
        )
        .bind(&input.order_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(PreparationError::Invalid("Order topilmadi"))?;
        let status: String = order.try_get("lifecycle_status")?;
        if !matches!(status.as_str(), "released" | "in_progress") {
            return Err(PreparationError::Conflict(
                "Bu orderga hozir sarf yozib bo‘lmaydi",
            ));
        }
        let actual: Option<String> = order.try_get("kg")?;
        let actual = actual.ok_or(PreparationError::Invalid("Order KG miqdori kiritilmagan"))?;
        if decimal(&actual)? != expected_kg {
            return Err(PreparationError::Conflict(
                "Order KG o‘zgargan. Yangilab, qayta tekshiring",
            ));
        }
        let saved: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_preparation_operations WHERE owner_ref=$1 AND order_id=$2 AND kind='consumption')")
            .bind(&actor.ref_).bind(&input.order_id).fetch_one(&mut *tx).await?;
        if saved {
            return Err(PreparationError::Conflict(
                "Bu order uchun tayyorlov sarfi avval saqlangan",
            ));
        }
        // Scope: order'dagi calculate-qatlamlardan kamida bittasi shu master'ga
        // biriktirilgan bo'lishi shart. UI filtrni chetlab o'tishdan himoya.
        if !order_matches_responsibility(&mut tx, &actor.ref_, &input.order_id).await? {
            return Err(PreparationError::Forbidden);
        }
        let id = new_id();
        let mut lines = Vec::new();
        let mut allocations = Vec::new();
        for (code, percent, required) in quantities {
            let name = material_name_in_warehouse(&mut tx, &actor.ref_, &code, &warehouse).await?;
            let lots = sqlx::query(&format!(
                "SELECT r.id AS receipt_id, s.id, s.barcode, s.qty::text AS kg
                FROM mini_raw_material_stock s LEFT JOIN mini_preparation_receipts r ON s.id=r.stock_id
                WHERE s.item_code=$2 AND s.warehouse=$3 AND (r.owner_ref=$1 OR (
                    s.payload_json->>'source'='raw_material_paddon' AND EXISTS (
                       SELECT 1 FROM mini_inventory_transfer_lines tl
                       JOIN mini_inventory_transfers t ON t.id=tl.transfer_id
                       JOIN mini_warehouses w ON w.id=t.destination_warehouse_id
                       WHERE tl.asset_kind='raw_material' AND tl.asset_ref=s.id
                         AND t.status='received' AND lower(w.name)=lower(s.warehouse))))
                  AND {AVAILABLE_STOCK}
                ORDER BY coalesce(r.created_at,s.created_at), coalesce(r.id,s.id) FOR UPDATE OF s"
            ))
            .bind(&actor.ref_)
            .bind(&code)
            .bind(&warehouse)
            .fetch_all(&mut *tx)
            .await?;
            let mut needed = required;
            for lot in lots {
                if needed == 0 {
                    break;
                }
                let available = decimal(&lot.try_get::<String, _>("kg")?)?;
                let used = available.min(needed);
                let remaining = available - used;
                let stock_id: String = lot.try_get("id")?;
                let barcode: String = lot.try_get("barcode")?;
                let receipt_id: Option<String> = lot.try_get("receipt_id")?;
                // READ COMMITTED takes a fresh snapshot here after any lock
                // wait. Assigning a barcode can insert a link without changing
                // the stock row; the pre-lock NOT EXISTS alone is insufficient.
                let eligible: bool = sqlx::query_scalar(&format!("SELECT EXISTS(SELECT 1 FROM mini_raw_material_stock s WHERE s.id=$1 AND {AVAILABLE_STOCK})"))
                    .bind(&stock_id).fetch_one(&mut *tx).await?;
                if !eligible {
                    continue;
                }
                let receipt_id = match receipt_id {
                    Some(id) => id,
                    None => paddons::received_preparation_lot(&mut tx, actor, &stock_id, &warehouse)
                        .await?.ok_or(PreparationError::Insufficient)?,
                };
                let after = if remaining == 0 {
                    "consumed"
                } else {
                    "available"
                };
                // Existing ERP whole-asset convention retains qty on consumed
                // rows. Effective balance is zero by status; partial rows hold
                // the remainder. Original receipt quantity stays immutable.
                sqlx::query("UPDATE mini_raw_material_stock SET qty=$2::text::numeric, status=$3, updated_at=now() WHERE id=$1")
                    .bind(&stock_id).bind(decimal_text(if remaining == 0 { available } else { remaining }))
                    .bind(after).execute(&mut *tx).await?;
                event(
                    &mut tx,
                    actor,
                    &id,
                    &warehouse,
                    &barcode,
                    &code,
                    &name,
                    -used,
                    Some(&input.order_id),
                    after,
                )
                .await?;
                allocations.push((receipt_id, used));
                needed -= used;
            }
            if needed != 0 {
                return Err(PreparationError::Insufficient);
            }
            lines.push(json!({"item_code":code,"name":name,"percent":decimal_text(percent),"kg":decimal_text(required)}));
        }
        let result = json!({"id":id,"kind":"consumption","warehouse":warehouse,
            "order_id":input.order_id,"order_code":order.try_get::<String,_>("code")?,
            "order_title":order.try_get::<String,_>("title")?,"order_kg":decimal_text(expected_kg),"lines":lines});
        record(
            &mut tx,
            &id,
            &actor.ref_,
            &input.request_id,
            "consumption",
            Some(&input.order_id),
            request,
            &result,
        )
        .await?;
        for (receipt, kg) in allocations {
            sqlx::query("INSERT INTO mini_preparation_allocations(operation_id,receipt_id,kg) VALUES ($1,$2,$3::text::numeric)")
                .bind(&id).bind(receipt).bind(decimal_text(kg)).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(result)
    }

    pub async fn upsert_formula(
        &self,
        actor: &Principal,
        input: FormulaUpsert,
    ) -> Result<Value, PreparationError> {
        self.upsert_formula_variant(actor, input, "").await
    }

    pub async fn upsert_formula_variant(&self, actor: &Principal, input: FormulaUpsert, variant_id: &str)
        -> Result<Value, PreparationError> {
        self.upsert_formula_in_order(actor, input, variant_id, "").await
    }

    pub async fn upsert_formula_in_order(
        &self, actor: &Principal, input: FormulaUpsert, variant_id: &str, order: &str,
    ) -> Result<Value, PreparationError> {
        let order = order.trim();
        let product_code = input.product_key()?;
        let name = input.formula_name()?;
        let material_id = input.material_key()?;
        let normalized = input.normalized_lines()?;
        let mut tx = self.formula_write_transaction(&actor.ref_).await?;
        let scope = formula_variant_orders::OrderRecipeScope::new(&actor.ref_, order, &product_code, &material_id);
        let selected = order_formula_contents::scoped_selection(&mut tx, &scope, variant_id.trim())
            .await?.filter(|recipe| recipe.is_pinned);
        // Scope: formula faqat o'ziga biriktirilgan homashyoga yoziladi.
        let scope_material_name =
            responsibility_material_name(&mut tx, &actor.ref_, &material_id).await?;
        // Resolve names in code order (deadlock-safe), then sort lines
        // alphabetically by material name for stable display.
        let mut lines: Vec<(String, String, i64)> = Vec::new();
        for (code, percent) in normalized {
            let material = material_name(&mut tx, &actor.ref_, &code).await?;
            lines.push((code, material, percent));
        }
        lines.sort_by(|a, b| {
            a.1.to_lowercase()
                .cmp(&b.1.to_lowercase())
                .then_with(|| a.0.cmp(&b.0))
        });
        let payload: Vec<Value> = lines
            .iter()
            .map(|(code, material, percent)| {
                json!({"item_code": code, "name": material, "percent": decimal_text(*percent)})
            })
            .collect();
        if let Some(recipe) = selected {
            let mut formula = order_formula_contents::upsert_order_component(
                &mut tx, &scope, variant_id.trim(), recipe, &name, json!(payload),
            ).await?;
            formula["product_code"] = json!(product_code);
            formula["material_id"] = json!(material_id);
            formula["material_name"] = json!(scope_material_name);
            tx.commit().await?;
            return Ok(formula);
        }
        let variant = formula_variants::variant_scope(&mut tx, &actor.ref_, &product_code, &material_id, variant_id, true).await?;
        let (formula_code, share): (String, Option<String>) = sqlx::query_as(
            "INSERT INTO mini_preparation_formulas(owner_ref, product_code, material_id, material_name, name, lines, updated_at, variant_id, share_percent)
             VALUES ($1,$2,$3,$4,$5,$6,now(),$7, CASE WHEN NOT EXISTS (
                 SELECT 1 FROM mini_preparation_formulas WHERE owner_ref=$1
                 AND product_code=$2 AND lower(material_id)=lower($3) AND variant_id=$7) THEN 100 ELSE NULL END)
             ON CONFLICT (owner_ref, product_code, material_id, variant_id, name)
             DO UPDATE SET material_name = EXCLUDED.material_name, lines = EXCLUDED.lines, updated_at = now()
             RETURNING formula_code,share_percent::text",
        )
        .bind(&actor.ref_)
        .bind(&product_code)
        .bind(&material_id)
        .bind(&scope_material_name)
        .bind(&name)
        .bind(json!(payload))
        .bind(&variant)
        .fetch_one(&mut *tx)
        .await?;
        order_formula_contents::record_catalog_revision(&mut tx, &variant).await?;
        tx.commit().await?;
        Ok(
            json!({"formula_code":formula_code,"product_code": product_code, "material_id": material_id,
            "material_name": scope_material_name, "name": name, "lines": payload, "share_percent": share, "variant_id":variant}),
        )
    }

    pub async fn list_formulas(
        &self,
        owner: &str,
        product_code: &str,
        material_id: &str,
    ) -> Result<Value, PreparationError> {
        self.list_formulas_variant(owner, product_code, material_id, "").await
    }

    pub async fn list_formulas_variant(&self, owner: &str, product_code: &str, material_id: &str, variant_id: &str)
        -> Result<Value, PreparationError> {
        let code = product_code.trim();
        if code.is_empty() || code.chars().count() > 160 {
            return Err(PreparationError::Invalid("Mahsulot kodi noto‘g‘ri"));
        }
        let material = material_id.trim();
        if material.is_empty() || material.chars().count() > 128 {
            return Err(PreparationError::Invalid("Homashyo tanlanmadi"));
        }
        let formulas_value: Value = sqlx::query_scalar(
            "SELECT COALESCE(jsonb_agg(f ORDER BY lower(f->>'name'), f->>'name') FILTER (WHERE f IS NOT NULL), '[]'::jsonb)
             FROM (SELECT jsonb_build_object('formula_code',formula_code,'name', name, 'lines', lines, 'share_percent', share_percent::text, 'variant_id',variant_id) AS f
                   FROM mini_preparation_formulas
                   WHERE owner_ref=$1 AND product_code=$2 AND lower(material_id)=lower($3)
                   AND variant_id=COALESCE(NULLIF($4,''),(SELECT id FROM mini_preparation_formula_variants
                       WHERE owner_ref=$1 AND product_code=$2 AND material_id=$3 AND is_default))) s",
        )
        .bind(owner)
        .bind(code)
        .bind(material)
        .bind(variant_id.trim())
        .fetch_one(&self.pool)
        .await
        .map_err(PreparationError::from)?;
        let formulas = formulas_value.as_array().cloned().unwrap_or_default();
        // Biriktirilmagan homashyo so'rovi bo'sh qaytadi (fail-closed).
        let assigned_name: Option<String> = sqlx::query_scalar(
            "SELECT material_name FROM mini_preparation_material_responsibilities
             WHERE principal_role='tayyorlov_masteri' AND principal_ref=$1
               AND lower(material_id)=lower($2)",
        )
        .bind(owner)
        .bind(material)
        .fetch_optional(&self.pool)
        .await?;
        let Some(assigned_name) = assigned_name else {
            return Ok(json!({"product_code": code, "material_id": material,
                "material_name": "", "formulas": []}));
        };
        Ok(json!({"product_code": code, "material_id": material,
            "material_name": assigned_name, "formulas": formulas}))
    }

    pub async fn delete_formula(
        &self,
        owner: &str,
        product_code: &str,
        name: &str,
        material_id: &str,
    ) -> Result<Value, PreparationError> {
        self.delete_formula_variant(owner, product_code, name, material_id, "").await
    }

    pub async fn delete_formula_variant(&self, owner: &str, product_code: &str, name: &str,
        material_id: &str, variant_id: &str) -> Result<Value, PreparationError> {
        self.delete_formula_in_order(owner, product_code, name, material_id, variant_id, "").await
    }

    pub async fn delete_formula_in_order(
        &self, owner: &str, product_code: &str, name: &str, material_id: &str,
        variant_id: &str, order: &str,
    ) -> Result<Value, PreparationError> {
        let order = order.trim();
        let code = product_code.trim();
        let formula = name.trim();
        let material = material_id.trim();
        if code.is_empty() || code.chars().count() > 160 {
            return Err(PreparationError::Invalid("Mahsulot kodi noto‘g‘ri"));
        }
        if formula.is_empty() || formula.chars().count() > 80 {
            return Err(PreparationError::Invalid(
                "Formula nomi 1–80 ta belgidan iborat bo‘lishi kerak",
            ));
        }
        if material.is_empty() || material.chars().count() > 128 {
            return Err(PreparationError::Invalid("Homashyo tanlanmadi"));
        }
        // Scope: faqat o'ziga biriktirilgan homashyo formulasi o'chadi.
        responsibility_material_name_tx(&self.pool, owner, material)
            .await?
            .ok_or(PreparationError::Forbidden)?;
        let mut tx = self.formula_write_transaction(owner).await?;
        let scope = formula_variant_orders::OrderRecipeScope::new(owner, order, code, material);
        if let Some(mut recipe) = order_formula_contents::scoped_selection(&mut tx, &scope, variant_id.trim())
            .await?.filter(|recipe| recipe.is_pinned)
        {
            let formulas = recipe.formulas.as_array_mut().expect("recipe formulas array");
            let before = formulas.len();
            formulas.retain(|entry| entry["name"] != formula);
            if formulas.len() == before {
                return Err(PreparationError::Invalid("Formula topilmadi"));
            }
            formula_revisions::update_order_revision(&mut tx, &scope, variant_id.trim(), &recipe.formulas).await?;
            tx.commit().await?;
            return Ok(json!({"product_code":code,"material_id":material,"name":formula,"deleted":true}));
        }
        let variant = formula_variants::variant_scope(&mut tx, owner, code, material, variant_id, false).await?;
        let deleted: bool = sqlx::query_scalar(
            "DELETE FROM mini_preparation_formulas
             WHERE owner_ref=$1 AND product_code=$2 AND name=$3
               AND lower(material_id)=lower($4) AND variant_id=$5
             RETURNING TRUE",
        )
        .bind(owner)
        .bind(code)
        .bind(formula)
        .bind(material)
        .bind(&variant)
        .fetch_optional(&mut *tx)
        .await?
        .unwrap_or(false);
        if !deleted {
            return Err(PreparationError::Invalid("Formula topilmadi"));
        }
        order_formula_contents::record_catalog_revision(&mut tx, &variant).await?;
        tx.commit().await?;
        Ok(json!({"product_code": code, "material_id": material,
            "name": formula, "deleted": true}))
    }

    /// Order qatlamlaridagi homashyolar (calculate-material oilalari).
    /// Formula tanlash picker'i uchun: master o'z biriktirilganlari bilan
    /// kesishmasini ko'rsatadi.
    pub async fn order_materials(&self, order_id: &str) -> Result<Value, PreparationError> {
        let id = order_id.trim();
        if id.is_empty() {
            return Err(PreparationError::Invalid("Order topilmadi"));
        }
        let pairs: Vec<(String, String)> = sqlx::query_as(
            "WITH source_templates AS (
               SELECT payload_json FROM mini_quick_order_templates
               WHERE btrim(COALESCE(payload_json->>'source_map_id','')) = $1
                 AND NOT EXISTS (SELECT 1 FROM mini_order_products WHERE order_id = $1)
             )
             SELECT lower(COALESCE(l->>'material_id','')) AS mid,
                    COALESCE(l->>'material','') AS mname
             FROM mini_order_products p,
                  jsonb_array_elements(COALESCE(p.layers_json, '[]'::jsonb)) l
             WHERE p.order_id = $1
             UNION
             SELECT lower(COALESCE(l->>'material_id','')),
                    COALESCE(l->>'material','')
             FROM source_templates t,
                  jsonb_array_elements(COALESCE(t.payload_json->'layers', '[]'::jsonb)) l
             UNION
             SELECT '', COALESCE(p.first_layer_material,'') FROM mini_order_products p
             WHERE p.order_id = $1 AND btrim(COALESCE(p.first_layer_material,'')) <> ''
             UNION
             SELECT '', COALESCE(p.second_layer_material,'') FROM mini_order_products p
             WHERE p.order_id = $1 AND btrim(COALESCE(p.second_layer_material,'')) <> ''
             UNION
             SELECT '', COALESCE(p.third_layer_material,'') FROM mini_order_products p
             WHERE p.order_id = $1 AND btrim(COALESCE(p.third_layer_material,'')) <> ''
             UNION
             SELECT '', COALESCE(t.payload_json->>'first_layer_material','')
             FROM source_templates t
             WHERE btrim(COALESCE(t.payload_json->>'first_layer_material','')) <> ''
             UNION
             SELECT '', COALESCE(t.payload_json->>'second_layer_material','')
             FROM source_templates t
             WHERE btrim(COALESCE(t.payload_json->>'second_layer_material','')) <> ''
             UNION
             SELECT '', COALESCE(t.payload_json->>'third_layer_material','')
             FROM source_templates t
             WHERE btrim(COALESCE(t.payload_json->>'third_layer_material','')) <> ''",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        let mut out: Vec<Value> = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for (mid, mname) in pairs {
            let resolved = if mid.trim().is_empty() {
                if mname.trim().is_empty() {
                    continue;
                }
                calculate_material_id_by_name(&self.pool, &mname).await?
            } else {
                let name = calculate_material_name_by_id(&self.pool, &mid)
                    .await?
                    .unwrap_or_else(|| mname.trim().to_string());
                if name.is_empty() {
                    continue;
                }
                Some((mid.trim().to_string(), name))
            };
            if let Some((resolved_id, resolved_name)) = resolved
                && seen.insert(resolved_id.to_lowercase())
            {
                out.push(json!({"material_id": resolved_id,
                    "material_name": resolved_name}));
            }
        }
        out.sort_by(|a, b| {
            a["material_name"]
                .as_str()
                .unwrap_or_default()
                .to_lowercase()
                .cmp(
                    &b["material_name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_lowercase(),
                )
        });
        Ok(json!({"order_id": id, "materials": out}))
    }

    /// Ownerga biriktirilgan omborning kanonik nomi (bola ombor ochishda
    /// ota-ombor tekshiruvi uchun). Biriktirilmagan bo'lsa Forbidden.
    pub async fn owned_warehouse_name(
        &self,
        owner: &str,
        name: &str,
    ) -> Result<String, PreparationError> {
        let mut tx = self.pool.begin().await?;
        let canonical = assigned_warehouse(&mut tx, owner, name).await?;
        tx.rollback().await?;
        Ok(canonical)
    }

    pub async fn warehouse_name_exists(&self, name: &str) -> Result<bool, PreparationError> {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_warehouses WHERE lower(name) = lower($1))")
            .bind(name.trim())
            .fetch_one(&self.pool)
            .await
            .map_err(PreparationError::from)
    }
}
