use super::*;
use crate::core::auth::models::PrincipalRole;
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, insert_raw_material_event_tx,
};
use serde::{Deserialize, Serialize};

mod attachments;
mod issues;
mod printing;
pub use attachments::{PaddonRollAttachment, PaddonRollOptions};
pub use issues::{PaddonIdentity, PaddonIssueCreate};
pub use printing::PaddonLabelPrint;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unit_kg: Option<String>,
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
            if let Some(ref kg) = line.unit_kg {
                decimal(kg)?;
            }
            if matches!(line.kind, UnitKind::Roll) {
                for dimension in [line.width_mm.as_deref(), line.micron.as_deref()]
                    .into_iter()
                    .flatten()
                {
                    decimal(dimension)?;
                }
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
    code: Option<String>,
    status: BalanceFilter,
    offset: u32,
    limit: Option<u32>,
}

// Packed preparation rolls may still be linked to their production order.
const PREPARED_PADDON_STOCK: &str =
    concat!("s.status = 'on_paddon' AND ", physically_in_warehouse!());

fn paddon_unit_stock_sql(sql: &str) -> String {
    sql.replace("{PADDON_UNIT_STOCK}", &format!(
        "(CASE WHEN u.prepared_roll THEN ({PREPARED_PADDON_STOCK}) ELSE ({PADDON_STOCK}) END)"
    ))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BalanceFilter {
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
        let limit = query.limit.unwrap_or(50);
        if !(1..=100).contains(&limit) {
            return Err(PreparationError::Invalid(
                "Paddon ro‘yxati filtri noto‘g‘ri",
            ));
        }
        self.read_raw_material_paddons(actor, query, limit, None).await
    }

    pub(crate) async fn preparation_journal_paddons(
        &self,
        actor: &Principal,
        q: String,
        status: BalanceFilter,
        limit: u32,
    ) -> Result<Value, PreparationError> {
        let query = PaddonQuery { q, status, ..PaddonQuery::default() };
        self.read_raw_material_paddons(actor, query, limit, None).await
    }

    async fn read_raw_material_paddons(
        &self,
        actor: &Principal,
        query: PaddonQuery,
        limit: u32,
        warehouse_ids: Option<&[String]>,
    ) -> Result<Value, PreparationError> {
        let role = role_code(actor)?;
        if query.q.chars().count() > 200 {
            return Err(PreparationError::Invalid("Paddon ro‘yxati filtri noto‘g‘ri"));
        }
        let search = query.q.trim().to_lowercase();
        let search = search
            .strip_prefix("raw-material-paddon:")
            .unwrap_or(&search);
        let code = query.code.as_deref().map(paddon_code).transpose()?;
        let status = match query.status {
            BalanceFilter::All => "all",
            BalanceFilter::Available => "available",
            BalanceFilter::Empty => "empty",
        };
        let sql = paddon_unit_stock_sql(include_str!("paddons.sql"));
        let mut paddons: Vec<Value> = sqlx::query_scalar(&sql)
            .bind(role)
            .bind(&actor.ref_)
            .bind(search)
            .bind(status)
            .bind(i64::from(limit) + 1)
            .bind(i64::from(query.offset))
            .bind(code)
            .bind(warehouse_ids)
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
            let unit_kg = line.unit_kg.as_deref().map(decimal).transpose()?;
            let material = PaddonMaterial {
                item_code,
                item_name,
                lot_number: line.lot_number.trim().into(),
                width_mm: line.width_mm.clone(),
                micron: line.micron.clone(),
            };
            for unit in 1..=line.count {
                let stock_id = if let Some(kg) = unit_kg {
                    let stock = receive_paddon_unit(
                        &mut tx,
                        actor,
                        &id,
                        &warehouse,
                        &material,
                        kg,
                        PaddonEntry::Packed,
                    )
                    .await?;
                    Some(stock.id)
                } else {
                    None
                };
                let unweighed = stock_id.is_none();
                sqlx::query(
                    "INSERT INTO mini_raw_material_paddon_units
                     (paddon_id, line_no, unit_no, stock_id, kind, unit_kg, lot_number,
                      item_code, item_name, width_mm, micron, unweighed_warehouse_id)
                     VALUES ($1,$2,$3,$4,$5,$6::text::numeric,$7,$8,$9,$10::text::numeric,$11::text::numeric,$12)",
                )
                .bind(&id)
                .bind(index as i32 + 1)
                .bind(unit)
                .bind(stock_id)
                .bind(line.kind.code())
                .bind(unit_kg.map(decimal_text))
                .bind(&material.lot_number)
                .bind(unweighed.then_some(&material.item_code))
                .bind(&material.item_name)
                .bind(material.width_mm.as_deref().map(str::trim))
                .bind(material.micron.as_deref().map(str::trim))
                .bind(unweighed.then_some(&warehouse_id))
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok(json!({"paddon_id": id, "paddon_code": code, "warehouse": warehouse}))
    }
}

async fn lock_paddon(tx: &mut Transaction<'_, Postgres>, code: &str) -> Result<(), PreparationError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("paddon-issue:{code}"))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn scoped_paddon(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Principal,
    code: &str,
) -> Result<(String, String, String), PreparationError> {
    sqlx::query_as(
        "SELECT p.id, w.id, w.name
         FROM mini_raw_material_paddons p
         JOIN mini_warehouses w ON w.id = p.warehouse_id
         JOIN mini_warehouse_assignments a ON lower(a.warehouse_name) = lower(w.name)
         WHERE p.code = $1 AND p.owner_role = $2 AND p.owner_ref = $3 AND NOT w.is_group
           AND a.assignment_kind = 'warehouse' AND a.principal_role = $2 AND a.principal_ref = $3
         FOR SHARE OF w, a",
    )
    .bind(code)
    .bind(role_code(actor)?)
    .bind(&actor.ref_)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PreparationError::Forbidden)
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

fn paddon_code(raw: &str) -> Result<String, PreparationError> {
    let normalized = raw.trim().to_ascii_uppercase();
    let code = normalized
        .strip_prefix("RAW-MATERIAL-PADDON:")
        .unwrap_or(&normalized);
    if !code
        .strip_prefix("XP-")
        .is_some_and(|id| id.len() == 16 && id.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err(PreparationError::Invalid(
            "Xomashyo paddonining QR kodini skanerlang",
        ));
    }
    Ok(code.to_owned())
}

#[derive(sqlx::FromRow)]
struct PaddonMaterial {
    item_code: String,
    item_name: String,
    lot_number: String,
    width_mm: Option<String>,
    micron: Option<String>,
}

struct PaddonStock {
    id: String,
    barcode: String,
}

#[derive(Clone, Copy)]
enum PaddonEntry<'a> {
    Packed,
    Whole(&'a str),
    Portion(&'a str),
}

async fn receive_paddon_unit(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Principal,
    paddon_id: &str,
    warehouse: &str,
    material: &PaddonMaterial,
    kg: i64,
    entry: PaddonEntry<'_>,
) -> Result<PaddonStock, PreparationError> {
    use crate::core::gscale::{epc::GscaleEpcGenerator, ports::EpcSource};
    let receipt_id = new_id();
    let barcode = GscaleEpcGenerator::new().next_epc();
    let stock_id = format!("raw:{}", barcode.to_ascii_lowercase());
    let kg_text = decimal_text(kg);
    let packed = matches!(entry, PaddonEntry::Packed);
    let issue_id = match entry {
        PaddonEntry::Packed => None,
        PaddonEntry::Whole(id) | PaddonEntry::Portion(id) => Some(id),
    };
    sqlx::query(
        "INSERT INTO mini_raw_material_stock
         (id, warehouse, item_code, item_name, barcode, qty, source_receipt_id, width_mm, micron, payload_json, status)
         VALUES ($1,$2,$3,$4,$5,$6::text::numeric,$7,$8::text::numeric,$9::text::numeric,$10,$11)",
    )
    .bind(&stock_id)
    .bind(warehouse)
    .bind(&material.item_code)
    .bind(&material.item_name)
    .bind(&barcode)
    .bind(&kg_text)
    .bind(&receipt_id)
    .bind(material.width_mm.as_deref().map(str::trim))
    .bind(material.micron.as_deref().map(str::trim))
    .bind(json!({"source":"raw_material_paddon","paddon_id":paddon_id,"owner_ref":actor.ref_,
                 "lot_number":material.lot_number, "paddon_issue_id":issue_id}))
    .bind(if packed { "on_paddon" } else { "available" })
    .execute(&mut **tx)
    .await?;
    if actor.role == PrincipalRole::TayyorlovMasteri {
        let output = json!({
            "id": receipt_id, "paddon_id": paddon_id, "kind": "receipt", "warehouse": warehouse,
            "item_code": material.item_code, "name": material.item_name, "kg": kg_text,
            "qr_printed": false,
        });
        record(
            tx,
            &receipt_id,
            &actor.ref_,
            &format!("paddon-unit-{receipt_id}"),
            "receipt",
            None,
            json!({"kind": if matches!(entry, PaddonEntry::Portion(_)) { "paddon_portion" } else { "paddon_unit" }, "paddon_id": paddon_id}),
            &output,
        )
        .await?;
        sqlx::query(
            "INSERT INTO mini_preparation_receipts (id, stock_id, item_code, owner_ref, initial_kg)
             VALUES ($1,$2,$3,$4,$5::text::numeric)",
        )
        .bind(&receipt_id)
        .bind(&stock_id)
        .bind(&material.item_code)
        .bind(&actor.ref_)
        .bind(&kg_text)
        .execute(&mut **tx)
        .await?;
    }
    let event = match entry {
        PaddonEntry::Portion(_) => PaddonEvent::SplitIn,
        _ => PaddonEvent::Receipt { packed },
    };
    record_paddon_event(
        tx,
        actor,
        warehouse,
        material,
        &barcode,
        kg,
        event,
        issue_id.unwrap_or(&receipt_id),
        paddon_id,
    )
    .await?;
    Ok(PaddonStock {
        id: stock_id,
        barcode,
    })
}

include!("paddons_parts/received_preparation_lot.rs");
