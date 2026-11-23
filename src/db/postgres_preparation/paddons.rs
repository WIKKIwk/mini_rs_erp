use super::*;
use crate::core::auth::models::PrincipalRole;
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, insert_raw_material_event_tx,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum UnitKind {
    Bag,
    Roll,
}

impl UnitKind {
    fn code(self) -> &'static str {
        match self {
            Self::Bag => "bag",
            Self::Roll => "roll",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PaddonLine {
    item_code: String,
    #[serde(default)]
    lot_number: String,
    kind: UnitKind,
    count: i32,
    unit_kg: String,
    #[serde(default)]
    width_mm: Option<String>,
    #[serde(default)]
    micron: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaddonCreate {
    pub request_id: String,
    pub warehouse: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    contents: Vec<PaddonLine>,
}

impl PaddonCreate {
    fn validate(&self) -> Result<(), PreparationError> {
        if self.name.chars().count() > 80 || self.name.chars().any(char::is_control) {
            return Err(PreparationError::Invalid(
                "Paddon nomini bir qatorda 80 belgigacha kiriting",
            ));
        }
        if !(8..=128).contains(&self.request_id.len())
            || !self
                .request_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
            || self.warehouse.trim().is_empty()
            || self.contents.is_empty()
            || self
                .contents
                .iter()
                .map(|line| i64::from(line.count))
                .sum::<i64>()
                > 200
        {
            return Err(PreparationError::Invalid(
                "Paddon uchun 1–200 ta birlik kiriting",
            ));
        }
        for line in &self.contents {
            if line.count <= 0
                || line.item_code.trim().is_empty()
                || line.lot_number.chars().count() > 120
            {
                return Err(PreparationError::Invalid("Paddon tarkibi noto‘g‘ri"));
            }
            decimal(&line.unit_kg)?;
            if matches!(line.kind, UnitKind::Roll) {
                decimal(line.width_mm.as_deref().unwrap_or_default())?;
                decimal(line.micron.as_deref().unwrap_or_default())?;
            } else if line.width_mm.is_some() || line.micron.is_some() {
                return Err(PreparationError::Invalid("Eni va mikron faqat rulon uchun"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PaddonQuery {
    q: String,
    status: BalanceFilter,
    offset: u32,
    limit: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BalanceFilter {
    #[default]
    All,
    Available,
    Empty,
}

fn role_code(actor: &Principal) -> Result<&'static str, PreparationError> {
    match actor.role {
        PrincipalRole::TayyorlovMasteri => Ok("tayyorlov_masteri"),
        PrincipalRole::MaterialTaminotchi => Ok("material_taminotchi"),
        _ => Err(PreparationError::Forbidden),
    }
}

impl PostgresPreparationStore {
    pub async fn raw_material_paddons(
        &self,
        actor: &Principal,
        query: PaddonQuery,
    ) -> Result<Value, PreparationError> {
        let role = role_code(actor)?;
        let limit = query.limit.unwrap_or(50);
        if !(1..=100).contains(&limit) || query.q.chars().count() > 200 {
            return Err(PreparationError::Invalid(
                "Paddon ro‘yxati filtri noto‘g‘ri",
            ));
        }
        let search = query.q.trim().to_lowercase();
        let search = search
            .strip_prefix("raw-material-paddon:")
            .unwrap_or(&search);
        let status = match query.status {
            BalanceFilter::All => "all",
            BalanceFilter::Available => "available",
            BalanceFilter::Empty => "empty",
        };
        let sql = include_str!("paddons.sql").replace("{AVAILABLE_STOCK}", AVAILABLE_STOCK);
        let mut paddons: Vec<Value> = sqlx::query_scalar(&sql)
            .bind(role)
            .bind(&actor.ref_)
            .bind(search)
            .bind(status)
            .bind(i64::from(limit) + 1)
            .bind(i64::from(query.offset))
            .fetch_all(&self.pool)
            .await?;
        let has_more = paddons.len() > limit as usize;
        paddons.truncate(limit as usize);
        Ok(json!({
            "paddons": paddons,
            "has_more": has_more,
            "next_offset": has_more.then(|| u64::from(query.offset) + u64::from(limit))
        }))
    }

    pub async fn receive_raw_material_paddon(
        &self,
        actor: &Principal,
        mut input: PaddonCreate,
        material_groups: &[String],
    ) -> Result<Value, PreparationError> {
        let role = role_code(actor)?;
        input.name = input.name.trim().to_owned();
        input.validate()?;
        let mut tx = self.pool.begin().await?;
        // Serialize retries for this owner's command before checking for a receipt.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!(
                "raw_material_paddon:{role}:{}:{}",
                actor.ref_, input.request_id
            ))
            .execute(&mut *tx)
            .await?;
        if actor.role == PrincipalRole::TayyorlovMasteri {
            exclusive_warehouse(&mut tx, &actor.ref_, &input.warehouse).await?;
        }
        let (warehouse_id, warehouse): (String, String) = sqlx::query_as(
            "SELECT w.id, w.name FROM mini_warehouses w
             JOIN mini_warehouse_assignments a ON lower(a.warehouse_name) = lower(w.name)
             WHERE lower(w.name) = lower($1) AND NOT w.is_group
               AND a.assignment_kind = 'warehouse' AND a.principal_role = $2
               AND a.principal_ref = $3 FOR SHARE OF w, a",
        )
        .bind(input.warehouse.trim())
        .bind(role)
        .bind(&actor.ref_)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(PreparationError::Forbidden)?;
        let request = json!(&input);
        let previous: Option<(String, String, Value)> = sqlx::query_as(
            "SELECT id, code, request_json FROM mini_raw_material_paddons
             WHERE owner_role = $1 AND owner_ref = $2 AND request_id = $3",
        )
        .bind(role)
        .bind(&actor.ref_)
        .bind(&input.request_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((id, code, saved)) = previous {
            if saved != request {
                return Err(PreparationError::Conflict(
                    "Bu kirim kaliti avval ishlatilgan",
                ));
            }
            return Ok(json!({"paddon_id": id, "paddon_code": code, "warehouse": warehouse}));
        }
        let id = new_id();
        let code = format!("XP-{}", id[..16].to_ascii_uppercase());
        sqlx::query(
            "INSERT INTO mini_raw_material_paddons
             (id, code, owner_role, owner_ref, warehouse_id, request_id, request_json, name)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(&id)
        .bind(&code)
        .bind(role)
        .bind(&actor.ref_)
        .bind(&warehouse_id)
        .bind(&input.request_id)
        .bind(&request)
        .bind(&input.name)
        .execute(&mut *tx)
        .await?;
        for (index, line) in input.contents.iter().enumerate() {
            let (item_code, item_name) =
                paddon_material(&mut tx, actor, &warehouse, line, material_groups).await?;
            let unit_kg = decimal(&line.unit_kg)?;
            for unit in 1..=line.count {
                let receipt_id = new_id();
                let stock_id = format!("raw:paddon:{receipt_id}");
                let barcode = format!("PREP-PADDON-{receipt_id}");
                sqlx::query(
                    "INSERT INTO mini_raw_material_stock
                     (id, warehouse, item_code, item_name, barcode, qty, source_receipt_id,
                      width_mm, micron, payload_json)
                     VALUES ($1,$2,$3,$4,$5,$6::text::numeric,$7,$8::text::numeric,
                             $9::text::numeric,$10)",
                )
                .bind(&stock_id)
                .bind(&warehouse)
                .bind(&item_code)
                .bind(&item_name)
                .bind(&barcode)
                .bind(decimal_text(unit_kg))
                .bind(&receipt_id)
                .bind(line.width_mm.as_deref().map(str::trim))
                .bind(line.micron.as_deref().map(str::trim))
                .bind(json!({"source":"raw_material_paddon","paddon_id":id,"owner_ref":actor.ref_}))
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    "INSERT INTO mini_raw_material_paddon_units
                     (paddon_id, line_no, unit_no, stock_id, kind, lot_number, unit_kg)
                     VALUES ($1,$2,$3,$4,$5,$6,$7::text::numeric)",
                )
                .bind(&id)
                .bind(index as i32 + 1)
                .bind(unit)
                .bind(&stock_id)
                .bind(line.kind.code())
                .bind(line.lot_number.trim())
                .bind(decimal_text(unit_kg))
                .execute(&mut *tx)
                .await?;
                if actor.role == PrincipalRole::TayyorlovMasteri {
                    let output = json!({
                        "id": receipt_id, "paddon_id": id, "kind": "receipt", "warehouse": warehouse,
                        "item_code": item_code, "name": item_name, "kg": decimal_text(unit_kg),
                        "qr_printed": false,
                    });
                    record(
                        &mut tx,
                        &receipt_id,
                        &actor.ref_,
                        &format!("paddon-unit-{receipt_id}"),
                        "receipt",
                        None,
                        json!({"kind": "paddon_unit", "paddon_id": id}),
                        &output,
                    )
                    .await?;
                    sqlx::query(
                        "INSERT INTO mini_preparation_receipts
                         (id, stock_id, item_code, owner_ref, initial_kg)
                         VALUES ($1,$2,$3,$4,$5::text::numeric)",
                    )
                    .bind(&receipt_id)
                    .bind(&stock_id)
                    .bind(&item_code)
                    .bind(&actor.ref_)
                    .bind(decimal_text(unit_kg))
                    .execute(&mut *tx)
                    .await?;
                }
                insert_raw_material_event_tx(
                    &mut tx,
                    RawMaterialEventDraft {
                        idempotency_key: format!("paddon-receipt:{receipt_id}"),
                        event_type: "receipt_posted".into(),
                        warehouse: warehouse.clone(),
                        barcode,
                        item_code: item_code.clone(),
                        item_name: item_name.clone(),
                        qty_delta: unit_kg as f64 / SCALE as f64,
                        uom: "kg".into(),
                        stock_status_before: None,
                        stock_status_after: Some("available".into()),
                        order_id: None,
                        apparatus: None,
                        actor_role: role.into(),
                        actor_ref: actor.ref_.clone(),
                        actor_display_name: actor.display_name.clone(),
                        owner_role: role.into(),
                        owner_ref: actor.ref_.clone(),
                        owner_display_name: actor.display_name.clone(),
                        source_type: "system".into(),
                        source_id: receipt_id,
                        source_line_ref: None,
                        correlation_id: Some(id.clone()),
                        payload_json: json!({"source":"raw_material_paddon", "paddon_id":id}),
                    },
                )
                .await?;
            }
        }
        tx.commit().await?;
        Ok(json!({"paddon_id": id, "paddon_code": code, "warehouse": warehouse}))
    }
}

async fn paddon_material(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Principal,
    warehouse: &str,
    line: &PaddonLine,
    material_groups: &[String],
) -> Result<(String, String), PreparationError> {
    let code = line.item_code.trim();
    if actor.role == PrincipalRole::TayyorlovMasteri {
        return match material_name_in_warehouse(tx, &actor.ref_, code, warehouse).await {
            Ok(name) => Ok((code.into(), name)),
            Err(PreparationError::MaterialNotInWarehouse)
                if matches!(line.kind, UnitKind::Roll) =>
            {
                assigned_rulon_item_for_receipt(tx, &actor.ref_, code).await
            }
            Err(error) => Err(error),
        };
    }
    sqlx::query_as(
        "SELECT code, name FROM mini_items WHERE lower(code) = lower($1)
         AND lower(btrim(item_group)) = ANY($2) FOR SHARE",
    )
    .bind(code)
    .bind(material_groups)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PreparationError::Forbidden)
}
