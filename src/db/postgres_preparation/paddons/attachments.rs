use super::*;

const ROLL_STOCK: &str = "FROM mini_gscale_receipts r
    JOIN mini_raw_material_stock s ON s.source_receipt_id = r.name
        AND lower(s.barcode) = lower(r.barcode)";
const SELECTED_RECEIPTS: &str = "WHERE r.name = ANY($2::text[]) AND r.status = 'submitted'
      AND r.payload_json->>'actor_role' = 'tayyorlov_masteri'
      AND r.payload_json->>'actor_ref' = $1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaddonRollOptions {
    receipt_names: Vec<String>,
    #[serde(default)]
    offset: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaddonRollAttachment {
    code: String,
    request_id: String,
    receipt_names: Vec<String>,
}

fn validate_roll_selection(names: &mut [String]) -> Result<(), PreparationError> {
    if !(1..=100).contains(&names.len())
        || names
            .iter()
            .any(|name| name.trim().is_empty() || name.len() > 1024 || name.contains('\0'))
    {
        return Err(PreparationError::Invalid("1–100 ta rulon tanlang"));
    }
    names.sort_unstable();
    if names.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(PreparationError::Invalid("Rulon faqat bir marta tanlanadi"));
    }
    Ok(())
}

impl PostgresPreparationStore {
    pub async fn production_paddon_options(
        &self,
        actor: &Principal,
        mut input: PaddonRollOptions,
    ) -> Result<Value, PreparationError> {
        require_preparation_master(actor)?;
        validate_roll_selection(&mut input.receipt_names)?;
        let mut tx = self.pool.begin().await?;
        let (warehouse_id, _) = eligible_rolls(&mut tx, actor, &input.receipt_names).await?;
        tx.commit().await?;
        self.read_raw_material_paddons(
            actor,
            PaddonQuery {
                offset: input.offset,
                ..PaddonQuery::default()
            },
            50,
            Some(&warehouse_id),
        )
        .await
    }

    pub async fn attach_prepared_rolls(
        &self,
        actor: &Principal,
        mut input: PaddonRollAttachment,
    ) -> Result<Value, PreparationError> {
        require_preparation_master(actor)?;
        validate_roll_selection(&mut input.receipt_names)?;
        let code = paddon_code(&input.code)?;
        if !(8..=128).contains(&input.request_id.len())
            || !input
                .request_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        {
            return Err(PreparationError::Invalid("Biriktirish kaliti noto‘g‘ri"));
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("paddon-attach:{}:{}", actor.ref_, input.request_id))
            .execute(&mut *tx)
            .await?;
        lock_paddon(&mut tx, &code).await?;
        let (paddon_id, warehouse_id, warehouse) = scoped_paddon(&mut tx, actor, &code).await?;
        let result = json!({
            "paddon_code": code, "warehouse": warehouse, "receipt_names": input.receipt_names
        });
        let previous: Option<(String, Vec<String>)> = sqlx::query_as(
            "SELECT paddon_id, receipt_names FROM mini_raw_material_paddon_attachments
             WHERE owner_ref = $1 AND request_id = $2",
        )
        .bind(&actor.ref_)
        .bind(&input.request_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((saved_paddon, saved_rolls)) = previous {
            if saved_paddon != paddon_id || saved_rolls != input.receipt_names {
                return Err(PreparationError::Conflict(
                    "Biriktirish kaliti avval ishlatilgan",
                ));
            }
            return Ok(result);
        }

        // Stock writers share these locks. Check its location again after acquiring them.
        sqlx::query(&format!(
            "SELECT s.id {ROLL_STOCK} {SELECTED_RECEIPTS} ORDER BY s.id FOR UPDATE OF s"
        ))
        .bind(&actor.ref_)
        .bind(&input.receipt_names)
        .fetch_all(&mut *tx)
        .await?;
        let (origin, stock_ids) = eligible_rolls(&mut tx, actor, &input.receipt_names).await?;
        if origin != warehouse_id {
            return Err(PreparationError::Conflict(
                "Paddon va rulonlar bitta omborda bo‘lishi kerak",
            ));
        }

        sqlx::query(
            "INSERT INTO mini_raw_material_paddon_units
                 (paddon_id, line_no, unit_no, stock_id, kind, lot_number, unit_kg,
                  item_name, width_mm, micron, prepared_roll)
             SELECT $1, last.line_no + row_number() OVER (ORDER BY s.id)::int, 1, s.id,
                    'roll', COALESCE(s.payload_json->>'lot_number', ''), s.qty,
                    s.item_name, s.width_mm, s.micron, true
             FROM mini_raw_material_stock s
             CROSS JOIN (
                SELECT COALESCE(max(line_no), 0) AS line_no
                FROM mini_raw_material_paddon_units WHERE paddon_id = $1
             ) last
             WHERE s.id = ANY($2::text[])",
        )
        .bind(&paddon_id)
        .bind(&stock_ids)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE mini_raw_material_stock SET status = 'on_paddon', updated_at = now()
                    WHERE id = ANY($1::text[])",
        )
        .bind(&stock_ids)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO mini_raw_material_paddon_attachments
                 (owner_ref, request_id, paddon_id, receipt_names) VALUES ($1,$2,$3,$4)",
        )
        .bind(&actor.ref_)
        .bind(&input.request_id)
        .bind(&paddon_id)
        .bind(&input.receipt_names)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(result)
    }
}

fn require_preparation_master(actor: &Principal) -> Result<(), PreparationError> {
    if actor.role == PrincipalRole::TayyorlovMasteri && !actor.ref_.trim().is_empty() {
        Ok(())
    } else {
        Err(PreparationError::Forbidden)
    }
}

async fn eligible_rolls(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Principal,
    names: &[String],
) -> Result<(String, Vec<String>), PreparationError> {
    let rows: Vec<(String, String)> = sqlx::query_as(&format!(
        "SELECT s.id, w.id {ROLL_STOCK}
          JOIN mini_warehouses w ON lower(w.name) = lower(s.warehouse) AND NOT w.is_group
        {SELECTED_RECEIPTS}
          AND s.status = 'available' AND s.qty > 0 AND s.uom = 'kg'
          AND {}
          AND EXISTS (SELECT 1 FROM mini_warehouse_assignments a
              WHERE lower(a.warehouse_name) = lower(w.name) AND a.assignment_kind = 'warehouse'
                AND a.principal_role = 'tayyorlov_masteri' AND a.principal_ref = $1)
          AND NOT EXISTS (SELECT 1 FROM mini_raw_material_paddon_units u
              WHERE u.stock_id = s.id AND u.issue_id IS NULL)",
        physically_in_warehouse!(),
    ))
    .bind(&actor.ref_)
    .bind(names)
    .fetch_all(&mut **tx)
    .await?;
    if rows.len() != names.len() {
        return Err(PreparationError::Conflict(
            "Tanlangan rulonlardan ayrimlarini paddonga qo‘shib bo‘lmaydi. Ombordagi holatini tekshiring",
        ));
    }
    let warehouse_id = rows[0].1.clone();
    if rows.iter().any(|(_, id)| id != &warehouse_id) {
        return Err(PreparationError::Conflict(
            "Bitta ombordagi rulonlarni tanlang",
        ));
    }
    let stock_ids = rows.into_iter().map(|(id, _)| id).collect();
    Ok((warehouse_id, stock_ids))
}
