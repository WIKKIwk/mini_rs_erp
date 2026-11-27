mod access;
mod entries;
mod read;
#[cfg(test)]
mod tests;
mod warehouse;

use crate::core::production_map::QueueActionActor;
use crate::core::quantity::{erp_quantity_from_units, erp_quantity_to_units};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};

pub use access::WasteBinAccessInput;
pub(crate) use entries::{WasteEntry, record_production_waste};

const QR_PREFIX: &str = "ACCORD-WASTE:";
const DEFAULT_TOLERANCE_KG: f64 = 0.5;
const RECEIVER_ROLE: &str = "chiqindi_qabulchi";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WasteBinError {
    #[error("invalid waste bin input")]
    InvalidInput,
    #[error("waste bin or cycle not found")]
    NotFound,
    #[error("waste bin state or request changed")]
    Conflict,
    #[error("waste bin operation not allowed")]
    Forbidden,
    #[error("record a reason for measurement outside tolerance")]
    DiscrepancyNoteRequired,
    #[error("select the waste warehouse for this receipt")]
    WarehouseRequired,
    #[error("waste warehouse is not assigned to this receiver")]
    WarehouseNotAssigned,
    #[error("waste bin storage operation failed")]
    StoreFailed,
}

impl WasteBinError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidInput => "waste_bin_invalid_input",
            Self::NotFound => "waste_bin_not_found",
            Self::Conflict => "waste_bin_conflict",
            Self::Forbidden => "waste_bin_forbidden",
            Self::DiscrepancyNoteRequired => "waste_discrepancy_note_required",
            Self::WarehouseRequired => "waste_warehouse_required",
            Self::WarehouseNotAssigned => "waste_warehouse_not_assigned",
            Self::StoreFailed => "waste_bin_store_failed",
        }
    }
}

impl From<sqlx::Error> for WasteBinError {
    fn from(error: sqlx::Error) -> Self {
        if error
            .as_database_error()
            .is_some_and(|e| e.is_unique_violation())
        {
            return Self::Conflict;
        }
        tracing::warn!(%error, "waste bin database operation failed");
        Self::StoreFailed
    }
}

#[derive(Clone)]
pub struct PostgresWasteBinStore {
    pool: PgPool,
}

#[derive(sqlx::FromRow)]
struct Bin {
    code: String,
    name: String,
    tare_units: i64,
    tolerance_units: i64,
    apparatus_ids: Vec<String>,
}

impl Bin {
    fn allows(&self, apparatus: &str) -> bool {
        self.apparatus_ids.iter().any(|id| id == apparatus)
    }

    fn matches_access(&self, access: &WasteBinAccessInput) -> bool {
        self.name == access.name && self.apparatus_ids == access.apparatus_ids
    }

    fn snapshot(&self, current_status: &str, cycle: Value) -> Value {
        json!({
            "code": self.code,
            "name": self.name,
            "apparatus_ids": self.apparatus_ids,
            "configured": !self.apparatus_ids.is_empty(),
            "qr_payload": format!("{QR_PREFIX}{}", self.code),
            "tare_kg": erp_quantity_from_units(self.tare_units),
            "tolerance_kg": erp_quantity_from_units(self.tolerance_units),
            "status": current_status,
            "cycle": cycle
        })
    }
}

fn operator_snapshot(mut snapshot: Value) -> Value {
    if let Some(fields) = snapshot.as_object_mut() {
        fields.remove("apparatus_ids");
        if let Some(replacement) = fields.get_mut("replacement").and_then(Value::as_object_mut) {
            replacement.remove("apparatus_ids");
        }
    }
    snapshot
}

fn code(raw: &str) -> Result<String, WasteBinError> {
    let upper = raw.trim().to_ascii_uppercase();
    let code = upper.strip_prefix(QR_PREFIX).unwrap_or(&upper);
    if !(1..=40).contains(&code.len())
        || !code.as_bytes()[0].is_ascii_alphanumeric()
        || !code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(WasteBinError::InvalidInput);
    }
    Ok(code.into())
}

fn identity(raw: &str) -> Result<&str, WasteBinError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 1024 {
        return Err(WasteBinError::InvalidInput);
    }
    Ok(trimmed)
}

fn units(kg: f64) -> Result<i64, WasteBinError> {
    erp_quantity_to_units(kg)
        .filter(|_| kg >= 0.0)
        .ok_or(WasteBinError::InvalidInput)
}

fn new_id() -> String {
    format!("{:032x}", rand::random::<u128>())
}

fn receiver(actor: &QueueActionActor) -> Result<(), WasteBinError> {
    if actor.role != RECEIVER_ROLE {
        return Err(WasteBinError::Forbidden);
    }
    Ok(())
}

async fn lock_apparatus(db: &mut PgConnection, apparatus: &str) -> Result<(), WasteBinError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('waste-bin:' || $1, 0))")
        .bind(apparatus)
        .execute(db)
        .await?;
    Ok(())
}

async fn lock_bin_keys(db: &mut PgConnection, codes: &[String]) -> Result<(), WasteBinError> {
    let mut ordered = codes.to_vec();
    ordered.sort_unstable();
    for code in &ordered {
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtextextended('waste-physical-bin:' || $1, 0))",
        )
        .bind(code)
        .execute(&mut *db)
        .await?;
    }
    Ok(())
}

async fn lock_bins(db: &mut PgConnection, codes: &[String]) -> Result<Vec<Bin>, WasteBinError> {
    lock_bin_keys(db, codes).await?;
    let rows = sqlx::query_as(&format!(
        "{} WHERE b.code = ANY($1) ORDER BY b.code",
        read::BINS
    ))
    .bind(codes)
    .fetch_all(db)
    .await?;
    if codes.len() != rows.len() {
        return Err(WasteBinError::NotFound);
    }
    Ok(rows)
}

async fn open_cycle_id(
    db: &mut PgConnection,
    apparatus: &str,
) -> Result<Option<String>, WasteBinError> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM mini_waste_bin_cycles WHERE apparatus_id = $1 AND sealed_at IS NULL",
    )
    .bind(apparatus)
    .fetch_optional(db)
    .await?)
}

async fn create_cycle(
    db: &mut PgConnection,
    code: &str,
    apparatus: &str,
    actor: &QueueActionActor,
) -> Result<String, WasteBinError> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO mini_waste_bin_cycles(id, bin_code, apparatus_id, opened_by_role, opened_by_ref, opened_by_display_name)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&id)
    .bind(code)
    .bind(apparatus)
    .bind(&actor.role)
    .bind(&actor.ref_)
    .bind(&actor.display_name)
    .execute(db)
    .await?;
    Ok(id)
}

async fn cycle_apparatus(
    db: &mut PgConnection,
    code: &str,
    cycle_id: &str,
) -> Result<String, WasteBinError> {
    sqlx::query_scalar(
        "SELECT apparatus_id FROM mini_waste_bin_cycles WHERE bin_code = $1 AND id = $2",
    )
    .bind(code)
    .bind(identity(cycle_id)?)
    .fetch_optional(db)
    .await?
    .ok_or(WasteBinError::NotFound)
}

impl PostgresWasteBinStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn register(
        &self,
        request_id: &str,
        tare_kg: f64,
        tolerance_kg: Option<f64>,
        access: &WasteBinAccessInput,
        actor: &QueueActionActor,
    ) -> Result<Value, WasteBinError> {
        receiver(actor)?;
        let request_id = identity(request_id)?;
        let access = access.normalized()?;
        let tare = units(tare_kg)?;
        let tolerance = units(tolerance_kg.unwrap_or(DEFAULT_TOLERANCE_KG))?;
        let mut tx = self.pool.begin().await?;
        let code = loop {
            let previous: Option<String> = sqlx::query_scalar(
                "SELECT code FROM mini_waste_bins
                 WHERE registered_by_ref = $1 AND registration_request_id = $2",
            )
            .bind(&actor.ref_)
            .bind(request_id)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(code) = previous {
                lock_bin_keys(&mut tx, std::slice::from_ref(&code)).await?;
                let bin = read::load_bin(&mut tx, &code).await?;
                if bin.tare_units != tare
                    || bin.tolerance_units != tolerance
                    || !bin.matches_access(&access)
                {
                    return Err(WasteBinError::Conflict);
                }
                break code;
            }

            let number: i64 = sqlx::query_scalar("SELECT nextval('mini_waste_bin_code_seq')")
                .fetch_one(&mut *tx)
                .await?;
            let code = format!("CQ-{number:03}");
            // A legacy code or a concurrent replay can win the insert.
            let inserted = sqlx::query(
                "INSERT INTO mini_waste_bins
                     (code, name, tare_units, tolerance_units, registered_by_ref, registration_request_id)
                 VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
            )
            .bind(&code)
            .bind(&access.name)
            .bind(tare)
            .bind(tolerance)
            .bind(&actor.ref_)
            .bind(request_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if inserted == 1 {
                access::write(&mut tx, &code, &access).await?;
                break code;
            }
        };
        let result = read::bin_snapshot(&mut tx, &code, None).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn configure(
        &self,
        raw_code: &str,
        access: &WasteBinAccessInput,
        actor: &QueueActionActor,
    ) -> Result<Value, WasteBinError> {
        receiver(actor)?;
        let access = access.normalized()?;
        let code = code(raw_code)?;
        let mut tx = self.pool.begin().await?;
        let bins = lock_bins(&mut tx, std::slice::from_ref(&code)).await?;
        if !bins[0].matches_access(&access) {
            if read::current_cycle(&mut tx, &code).await?.is_some() {
                return Err(WasteBinError::Conflict);
            }
            access::write(&mut tx, &code, &access).await?;
        }
        let result = read::bin_snapshot(&mut tx, &code, None).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn active(&self, apparatus: &str) -> Result<Value, WasteBinError> {
        self.active_with_scope(apparatus, None).await
    }

    pub async fn active_for_operator(
        &self,
        apparatus: &str,
        actor: &QueueActionActor,
    ) -> Result<Value, WasteBinError> {
        self.active_with_scope(apparatus, Some(actor)).await
    }

    async fn active_with_scope(
        &self,
        apparatus: &str,
        actor: Option<&QueueActionActor>,
    ) -> Result<Value, WasteBinError> {
        let apparatus = identity(apparatus)?;
        let mut tx = self.pool.begin().await?;
        lock_apparatus(&mut tx, apparatus).await?;
        let cycle_id = match actor {
            Some(_) => access::open_cycle(&mut tx, apparatus).await?,
            None => open_cycle_id(&mut tx, apparatus).await?,
        };
        let bin = match cycle_id {
            Some(id) => {
                let cycle = read::load_cycle(&mut tx, &id).await?;
                let summary = read::load_bin(&mut tx, &cycle.bin_code)
                    .await?
                    .snapshot("open", cycle.snapshot(&mut tx, true).await?);
                if actor.is_some() {
                    operator_snapshot(summary)
                } else {
                    summary
                }
            }
            None => Value::Null,
        };
        let pending = read::pending_entries(&mut tx, apparatus, actor).await?;
        tx.commit().await?;
        Ok(json!({"bin": bin, "pending": pending}))
    }

    pub async fn assign(
        &self,
        raw_code: &str,
        apparatus: &str,
        actor: &QueueActionActor,
    ) -> Result<Value, WasteBinError> {
        let code = code(raw_code)?;
        let apparatus = identity(apparatus)?;
        let mut tx = self.pool.begin().await?;
        lock_apparatus(&mut tx, apparatus).await?;
        let bins = lock_bins(&mut tx, std::slice::from_ref(&code)).await?;
        let current = read::current_cycle(&mut tx, &code).await?;
        let at_apparatus = current
            .as_ref()
            .is_some_and(|cycle| cycle.apparatus_id == apparatus && cycle.allows(&bins[0]));
        if !bins[0].allows(apparatus) && !at_apparatus {
            return Err(WasteBinError::Forbidden);
        }
        let id = match current {
            Some(cycle) if at_apparatus && cycle.status() == "open" => cycle.id,
            Some(_) => return Err(WasteBinError::Conflict),
            None => create_cycle(&mut tx, &code, apparatus, actor).await?,
        };
        let result = read::bin_snapshot(&mut tx, &code, Some(&id)).await?;
        tx.commit().await?;
        Ok(operator_snapshot(result))
    }

    pub async fn submit(
        &self,
        raw_code: &str,
        cycle_id: &str,
        replacement_code: Option<&str>,
        actor: &QueueActionActor,
    ) -> Result<Value, WasteBinError> {
        let code = code(raw_code)?;
        let next = replacement_code.map(self::code).transpose()?;
        if next.as_ref() == Some(&code) {
            return Err(WasteBinError::InvalidInput);
        }
        let mut tx = self.pool.begin().await?;
        let apparatus = cycle_apparatus(&mut tx, &code, cycle_id).await?;
        lock_apparatus(&mut tx, &apparatus).await?;
        let mut codes = vec![code.clone()];
        codes.extend(next.iter().cloned());
        let bins = lock_bins(&mut tx, &codes).await?;
        let bin = bins
            .iter()
            .find(|bin| bin.code == code)
            .ok_or(WasteBinError::NotFound)?;
        let old = read::load_cycle(&mut tx, cycle_id).await?;
        if !old.allows(bin) {
            return Err(WasteBinError::Forbidden);
        }
        if let Some(next) = &next {
            let replacement = bins
                .iter()
                .find(|bin| bin.code == *next)
                .ok_or(WasteBinError::NotFound)?;
            if !replacement.allows(&apparatus) {
                return Err(WasteBinError::Forbidden);
            }
        }
        let replacement_id = if old.status() == "open" {
            let total = read::cycle_units(&mut tx, cycle_id).await?;
            sqlx::query(
                "UPDATE mini_waste_bin_cycles SET expected_units=$2, sealed_at=now(), sealed_by_ref=$3,
                    sealed_by_display_name=$4 WHERE id=$1",
            )
            .bind(cycle_id)
            .bind(total)
            .bind(&actor.ref_)
            .bind(&actor.display_name)
            .execute(&mut *tx)
            .await?;
            if let Some(next) = &next {
                let next_id = create_cycle(&mut tx, next, &apparatus, actor).await?;
                sqlx::query("UPDATE mini_waste_bin_cycles SET replacement_cycle_id=$2 WHERE id=$1")
                    .bind(cycle_id)
                    .bind(&next_id)
                    .execute(&mut *tx)
                    .await?;
                Some(next_id)
            } else {
                None
            }
        } else {
            let earlier_code = match &old.replacement_cycle_id {
                Some(id) => {
                    let replacement = read::load_cycle(&mut tx, id).await?;
                    if !replacement.held_by(actor) {
                        return Err(WasteBinError::Conflict);
                    }
                    Some(replacement.bin_code)
                }
                None => None,
            };
            if earlier_code != next {
                return Err(WasteBinError::Conflict);
            }
            old.replacement_cycle_id
        };
        let mut result = read::bin_snapshot(&mut tx, &code, Some(cycle_id)).await?;
        result["replacement"] = match (next, replacement_id) {
            (Some(next), Some(id)) => read::bin_snapshot(&mut tx, &next, Some(&id)).await?,
            _ => Value::Null,
        };
        tx.commit().await?;
        Ok(operator_snapshot(result))
    }

    pub async fn receive(
        &self,
        raw_code: &str,
        cycle_id: &str,
        warehouse_id: &str,
        gross_kg: f64,
        measurement_source: &str,
        note: &str,
        actor: &QueueActionActor,
    ) -> Result<Value, WasteBinError> {
        receiver(actor)?;
        let code = code(raw_code)?;
        let warehouse_id = warehouse::destination_id(warehouse_id)?;
        let gross = units(gross_kg)?;
        let note = note.trim();
        if !matches!(measurement_source, "gscale" | "manual") || note.len() > 1000 {
            return Err(WasteBinError::InvalidInput);
        }
        let mut tx = self.pool.begin().await?;
        let bins = lock_bins(&mut tx, std::slice::from_ref(&code)).await?;
        let bin = &bins[0];
        cycle_apparatus(&mut tx, &code, cycle_id).await?;
        let cycle = read::load_cycle(&mut tx, cycle_id).await?;
        if let Some(receipt) = cycle.receipt()? {
            if receipt.gross_units != gross
                || receipt.measurement_source != measurement_source
                || receipt.note != note
                || receipt.received_by_ref != actor.ref_
                || receipt.warehouse_id.as_deref() != Some(warehouse_id)
            {
                return Err(WasteBinError::Conflict);
            }
        } else {
            let expected = cycle.expected_units.ok_or(WasteBinError::Conflict)?;
            let net = gross
                .checked_sub(bin.tare_units)
                .filter(|n| *n >= 0)
                .ok_or(WasteBinError::InvalidInput)?;
            if (net - expected).abs() > bin.tolerance_units && note.is_empty() {
                return Err(WasteBinError::DiscrepancyNoteRequired);
            }
            let warehouse =
                warehouse::assigned_warehouse(&mut tx, warehouse_id, &actor.ref_).await?;
            sqlx::query(
                "INSERT INTO mini_waste_bin_receipts(cycle_id, gross_units, tare_units, expected_units,
                    tolerance_units, measurement_source, note, received_by_ref, received_by_display_name)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(cycle_id)
            .bind(gross)
            .bind(bin.tare_units)
            .bind(expected)
            .bind(bin.tolerance_units)
            .bind(measurement_source)
            .bind(note)
            .bind(&actor.ref_)
            .bind(&actor.display_name)
            .execute(&mut *tx)
            .await?;
            warehouse::stock_receipt(&mut tx, cycle_id, &warehouse, actor).await?;
        }
        let result = read::bin_snapshot(&mut tx, &code, Some(cycle_id)).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn reset(
        &self,
        raw_code: &str,
        cycle_id: &str,
        actor: &QueueActionActor,
    ) -> Result<Value, WasteBinError> {
        receiver(actor)?;
        let code = code(raw_code)?;
        let mut tx = self.pool.begin().await?;
        lock_bins(&mut tx, std::slice::from_ref(&code)).await?;
        cycle_apparatus(&mut tx, &code, cycle_id).await?;
        let cycle = read::load_cycle(&mut tx, cycle_id).await?;
        let receipt = cycle.receipt()?.ok_or(WasteBinError::Conflict)?;
        if cycle.reset_at_unix.is_none() {
            let warehouse_id = receipt
                .warehouse_id
                .ok_or(WasteBinError::WarehouseRequired)?;
            warehouse::assigned_warehouse(&mut tx, &warehouse_id, &actor.ref_).await?;
            sqlx::query(
                "UPDATE mini_waste_bin_cycles SET reset_at=now(), reset_by_ref=$2,
                    reset_by_display_name=$3 WHERE id=$1",
            )
            .bind(cycle_id)
            .bind(&actor.ref_)
            .bind(&actor.display_name)
            .execute(&mut *tx)
            .await?;
        }
        let result = read::bin_snapshot(&mut tx, &code, Some(cycle_id)).await?;
        tx.commit().await?;
        Ok(result)
    }
}
