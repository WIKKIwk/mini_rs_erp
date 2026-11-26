use super::*;

use std::collections::HashMap;

pub(super) const BINS: &str = r#"
    SELECT b.code, b.name, b.tare_units, b.tolerance_units,
        ARRAY(SELECT a.apparatus_id FROM mini_waste_bin_apparatuses a
              WHERE a.bin_code = b.code ORDER BY a.apparatus_id COLLATE "C") AS apparatus_ids,
        COALESCE((SELECT jsonb_agg(jsonb_build_object('role', o.operator_role, 'ref', o.operator_ref)
            ORDER BY o.operator_role COLLATE "C", o.operator_ref COLLATE "C")
            FROM mini_waste_bin_operators o WHERE o.bin_code = b.code), '[]'::jsonb) AS operators
    FROM mini_waste_bins b"#;

const CYCLES: &str = "
    SELECT c.id, c.bin_code, c.apparatus_id,
        COALESCE((SELECT name FROM mini_apparatus WHERE id = c.apparatus_id), c.apparatus_id) AS apparatus_name,
        EXTRACT(EPOCH FROM c.opened_at)::bigint AS opened_at_unix,
        c.opened_by_role, c.opened_by_ref, c.opened_by_display_name, c.expected_units,
        COALESCE(c.expected_units, (SELECT COALESCE(sum(units), 0)::bigint
            FROM mini_waste_bin_entries WHERE cycle_id = c.id)) AS visible_units,
        EXTRACT(EPOCH FROM c.sealed_at)::bigint AS sealed_at_unix,
        c.sealed_by_ref, c.sealed_by_display_name, c.replacement_cycle_id,
        EXTRACT(EPOCH FROM c.reset_at)::bigint AS reset_at_unix,
        c.reset_by_ref, c.reset_by_display_name,
        CASE WHEN r.cycle_id IS NOT NULL THEN
            jsonb_build_object('gross_units', r.gross_units, 'tare_units', r.tare_units,
                'expected_units', r.expected_units, 'tolerance_units', r.tolerance_units,
                'measurement_source', r.measurement_source, 'note', r.note,
                'received_by_ref', r.received_by_ref, 'received_by_display_name', r.received_by_display_name,
                'received_at_unix', EXTRACT(EPOCH FROM r.received_at)::bigint,
                'warehouse_id', w.warehouse_id, 'warehouse', w.warehouse_name,
                'stocked_by_display_name', w.stocked_by_display_name,
                'stocked_at_unix', EXTRACT(EPOCH FROM w.stocked_at)::bigint)
        END AS receipt_payload
    FROM mini_waste_bin_cycles c
    LEFT JOIN mini_waste_bin_receipts r ON r.cycle_id = c.id
    LEFT JOIN mini_waste_warehouse_receipts w ON w.cycle_id = c.id";

#[derive(sqlx::FromRow)]
pub(super) struct Cycle {
    pub id: String,
    pub bin_code: String,
    pub apparatus_id: String,
    pub apparatus_name: String,
    pub opened_at_unix: i64,
    pub opened_by_role: Option<String>,
    pub opened_by_ref: String,
    pub opened_by_display_name: String,
    pub expected_units: Option<i64>,
    pub visible_units: i64,
    pub sealed_at_unix: Option<i64>,
    pub sealed_by_ref: Option<String>,
    pub sealed_by_display_name: Option<String>,
    pub replacement_cycle_id: Option<String>,
    pub reset_at_unix: Option<i64>,
    pub reset_by_ref: Option<String>,
    pub reset_by_display_name: Option<String>,
    pub receipt_payload: Option<Value>,
}

#[derive(serde::Deserialize)]
pub(super) struct Receipt {
    pub gross_units: i64,
    pub tare_units: i64,
    pub expected_units: i64,
    pub tolerance_units: i64,
    pub measurement_source: String,
    pub note: String,
    pub received_by_ref: String,
    pub received_by_display_name: String,
    pub received_at_unix: i64,
    pub warehouse_id: Option<String>,
    pub warehouse: Option<String>,
    pub stocked_by_display_name: Option<String>,
    pub stocked_at_unix: Option<i64>,
}

impl Receipt {
    fn snapshot(&self) -> Value {
        let net = self.gross_units - self.tare_units;
        let difference = net - self.expected_units;
        json!({
            "gross_kg": erp_quantity_from_units(self.gross_units),
            "tare_kg": erp_quantity_from_units(self.tare_units),
            "net_kg": erp_quantity_from_units(net),
            "expected_kg": erp_quantity_from_units(self.expected_units),
            "difference_kg": erp_quantity_from_units(difference),
            "outside_tolerance": difference.abs() > self.tolerance_units,
            "measurement_source": self.measurement_source,
            "note": self.note,
            "received_by_ref": self.received_by_ref,
            "received_by_display_name": self.received_by_display_name,
            "received_at_unix": self.received_at_unix,
            "warehouse_id": self.warehouse_id,
            "warehouse": self.warehouse,
            "stocked_by_display_name": self.stocked_by_display_name,
            "stocked_at_unix": self.stocked_at_unix,
        })
    }
}

impl Cycle {
    pub(super) fn is_legacy_holder(&self, actor: &QueueActionActor) -> bool {
        self.opened_by_role.is_none()
            && self.reset_at_unix.is_none()
            && self.opened_by_ref == actor.ref_
    }

    pub(super) fn held_by(&self, actor: &QueueActionActor) -> bool {
        self.is_legacy_holder(actor)
            || (self.opened_by_role.as_deref() == Some(actor.role.as_str())
                && self.opened_by_ref == actor.ref_)
    }

    pub(super) fn allows(&self, bin: &Bin, actor: &QueueActionActor) -> bool {
        self.is_legacy_holder(actor) || bin.allows(&self.apparatus_id, actor)
    }

    pub(super) fn status(&self) -> &'static str {
        if self.reset_at_unix.is_some() {
            "reset"
        } else if self.receipt_payload.is_some() {
            "received"
        } else if self.sealed_at_unix.is_some() {
            "sealed"
        } else {
            "open"
        }
    }

    pub(super) fn receipt(&self) -> Result<Option<Receipt>, WasteBinError> {
        self.receipt_payload
            .as_ref()
            .map(|value| {
                serde_json::from_value(value.clone()).map_err(|_| WasteBinError::StoreFailed)
            })
            .transpose()
    }

    fn summary(&self) -> Result<Value, WasteBinError> {
        Ok(json!({
            "id": self.id,
            "code": self.bin_code,
            "apparatus_id": self.apparatus_id,
            "apparatus_name": self.apparatus_name,
            "status": self.status(),
            "opened_at_unix": self.opened_at_unix,
            "opened_by_ref": self.opened_by_ref,
            "opened_by_display_name": self.opened_by_display_name,
            "sealed_at_unix": self.sealed_at_unix,
            "sealed_by_ref": self.sealed_by_ref,
            "sealed_by_display_name": self.sealed_by_display_name,
            "expected_kg": erp_quantity_from_units(self.visible_units),
            "receipt": self.receipt()?.map(|receipt| receipt.snapshot()),
            "reset_at_unix": self.reset_at_unix,
            "reset_by_ref": self.reset_by_ref,
            "reset_by_display_name": self.reset_by_display_name,
        }))
    }

    pub(super) async fn snapshot(
        &self,
        db: &mut PgConnection,
        with_entries: bool,
    ) -> Result<Value, WasteBinError> {
        let mut summary = self.summary()?;
        if with_entries {
            let entries = sqlx::query_scalar(&format!(
                "{ENTRIES} WHERE cycle_id=$1 ORDER BY created_at, id"
            ))
            .bind(&self.id)
            .fetch_all(db)
            .await?;
            summary["entries"] = Value::Array(entries);
        }
        Ok(summary)
    }
}

const ENTRIES: &str = "
    SELECT jsonb_build_object('id', id, 'request_id', request_id, 'apparatus_id', apparatus_id,
        'order_id', order_id, 'session_id', session_id, 'origin', origin,
        'kg', units::numeric / 1000000, 'reported_kg', reported_units::numeric / 1000000,
        'reason', reason, 'actor_ref', actor_ref, 'actor_display_name', actor_display_name,
        'created_at_unix', EXTRACT(EPOCH FROM created_at)::bigint)
    FROM mini_waste_bin_entries";

pub(super) async fn pending_entries(
    db: &mut PgConnection,
    apparatus: &str,
    actor: Option<&QueueActionActor>,
) -> Result<Vec<Value>, WasteBinError> {
    Ok(sqlx::query_scalar(&format!(
        "{ENTRIES} WHERE apparatus_id=$1 AND cycle_id IS NULL AND units>0
         AND ($2::text IS NULL OR (actor_role=$2 AND actor_ref=$3)) ORDER BY created_at, id"
    ))
    .bind(apparatus)
    .bind(actor.map(|actor| actor.role.as_str()))
    .bind(actor.map(|actor| actor.ref_.as_str()))
    .fetch_all(db)
    .await?)
}

pub(super) async fn cycle_units(
    db: &mut PgConnection,
    cycle_id: &str,
) -> Result<i64, WasteBinError> {
    Ok(sqlx::query_scalar(
        "SELECT COALESCE(sum(units), 0)::bigint FROM mini_waste_bin_entries WHERE cycle_id=$1",
    )
    .bind(cycle_id)
    .fetch_one(db)
    .await?)
}

pub(super) async fn load_bin(db: &mut PgConnection, code: &str) -> Result<Bin, WasteBinError> {
    sqlx::query_as(&format!("{BINS} WHERE b.code=$1"))
        .bind(code)
        .fetch_optional(db)
        .await?
        .ok_or(WasteBinError::NotFound)
}

pub(super) async fn load_cycle(db: &mut PgConnection, id: &str) -> Result<Cycle, WasteBinError> {
    sqlx::query_as(&format!("{CYCLES} WHERE c.id=$1"))
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or(WasteBinError::NotFound)
}

pub(super) async fn current_cycle(
    db: &mut PgConnection,
    code: &str,
) -> Result<Option<Cycle>, WasteBinError> {
    Ok(sqlx::query_as(&format!(
        "{CYCLES} WHERE c.bin_code=$1 AND c.reset_at IS NULL"
    ))
    .bind(code)
    .fetch_optional(db)
    .await?)
}

pub(super) async fn bin_snapshot(
    db: &mut PgConnection,
    code: &str,
    cycle_id: Option<&str>,
) -> Result<Value, WasteBinError> {
    bin_snapshot_with_scope(db, code, cycle_id, None).await
}

async fn bin_snapshot_with_scope(
    db: &mut PgConnection,
    code: &str,
    cycle_id: Option<&str>,
    scope: Option<(&str, &QueueActionActor)>,
) -> Result<Value, WasteBinError> {
    let bin = load_bin(db, code).await?;
    let current = current_cycle(db, code).await?;
    let current_status = current.as_ref().map_or("empty", Cycle::status);
    let selected = match cycle_id {
        Some(id) if !current.as_ref().is_some_and(|c| c.id == id) => {
            let cycle = load_cycle(db, id).await?;
            if cycle.bin_code != code {
                return Err(WasteBinError::NotFound);
            }
            Some(cycle)
        }
        _ => current,
    };
    let (status, allow) = match scope {
        Some((apparatus, actor)) => match selected.as_ref() {
            Some(cycle) => (
                cycle.status(),
                cycle.apparatus_id == apparatus && cycle.allows(&bin, actor),
            ),
            None => ("empty", bin.allows(apparatus, actor)),
        },
        None => (current_status, true),
    };
    if !allow {
        return Err(WasteBinError::NotFound);
    }
    let cycle = match selected {
        Some(cycle) => cycle.snapshot(db, true).await?,
        None => Value::Null,
    };
    let snapshot = bin.snapshot(status, cycle);
    Ok(if scope.is_some() {
        operator_snapshot(snapshot)
    } else {
        snapshot
    })
}

impl PostgresWasteBinStore {
    pub(crate) async fn cycle_matches_apparatus(
        &self,
        raw_code: &str,
        cycle_id: &str,
        apparatus: &str,
    ) -> Result<bool, WasteBinError> {
        let bin_code = code(raw_code)?;
        let mut db = self.pool.acquire().await?;
        Ok(cycle_apparatus(&mut db, &bin_code, cycle_id).await? == apparatus.trim())
    }

    pub async fn detail(&self, raw_code: &str) -> Result<Value, WasteBinError> {
        bin_snapshot(&mut *self.pool.acquire().await?, &code(raw_code)?, None).await
    }

    pub async fn detail_cycle(
        &self,
        raw_code: &str,
        cycle_id: &str,
    ) -> Result<Value, WasteBinError> {
        bin_snapshot(
            &mut *self.pool.acquire().await?,
            &code(raw_code)?,
            Some(identity(cycle_id)?),
        )
        .await
    }

    pub async fn detail_for_operator(
        &self,
        raw_code: &str,
        cycle_id: Option<&str>,
        apparatus: &str,
        actor: &QueueActionActor,
    ) -> Result<Value, WasteBinError> {
        let apparatus = identity(apparatus)?;
        let code = code(raw_code)?;
        let cycle_id = cycle_id.map(identity).transpose()?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let result =
            bin_snapshot_with_scope(&mut tx, &code, cycle_id, Some((apparatus, actor))).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn list(
        &self,
        status: Option<&str>,
        apparatus: Option<&str>,
    ) -> Result<Value, WasteBinError> {
        let apparatus = apparatus.map(identity).transpose()?;
        self.list_with_scope(status, apparatus, None).await
    }

    pub async fn list_for_operator(
        &self,
        status: Option<&str>,
        apparatus: &str,
        actor: &QueueActionActor,
    ) -> Result<Value, WasteBinError> {
        self.list_with_scope(status, Some(identity(apparatus)?), Some(actor))
            .await
    }

    async fn list_with_scope(
        &self,
        status: Option<&str>,
        apparatus: Option<&str>,
        actor: Option<&QueueActionActor>,
    ) -> Result<Value, WasteBinError> {
        if status.is_some_and(|s| !matches!(s, "empty" | "open" | "sealed" | "received")) {
            return Err(WasteBinError::InvalidInput);
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let bins: Vec<Bin> = sqlx::query_as(&format!("{BINS} ORDER BY b.code"))
            .fetch_all(&mut *tx)
            .await?;
        let active: Vec<Cycle> = sqlx::query_as(&format!("{CYCLES} WHERE c.reset_at IS NULL"))
            .fetch_all(&mut *tx)
            .await?;
        tx.commit().await?;
        let by_code: HashMap<_, _> = active
            .iter()
            .map(|cycle| (cycle.bin_code.as_str(), cycle))
            .collect();
        let mut snapshots = Vec::new();
        for bin in bins {
            let cycle = by_code.get(bin.code.as_str()).copied();
            let current_status = cycle.map_or("empty", Cycle::status);
            let visible = match (apparatus, actor, cycle) {
                (Some(id), Some(actor), Some(cycle)) => {
                    cycle.apparatus_id == id && cycle.allows(&bin, actor)
                }
                (Some(id), Some(actor), None) => bin.allows(id, actor),
                (Some(id), None, _) => cycle.is_some_and(|c| c.apparatus_id == id),
                _ => true,
            };
            if status.is_some_and(|expected| expected != current_status) || !visible {
                continue;
            }
            let snapshot = bin.snapshot(
                current_status,
                cycle
                    .map(Cycle::summary)
                    .transpose()?
                    .unwrap_or(Value::Null),
            );
            snapshots.push(if actor.is_some() {
                operator_snapshot(snapshot)
            } else {
                snapshot
            });
        }
        Ok(json!({"bins": snapshots}))
    }

    pub async fn history(&self, raw_code: &str) -> Result<Value, WasteBinError> {
        let code = code(raw_code)?;
        let mut db = self.pool.acquire().await?;
        load_bin(&mut db, &code).await?;
        let cycles: Vec<Cycle> = sqlx::query_as(&format!(
            "{CYCLES} WHERE c.bin_code=$1 ORDER BY c.opened_at DESC, c.id DESC"
        ))
        .bind(code)
        .fetch_all(&mut *db)
        .await?;
        let summaries = cycles
            .iter()
            .map(Cycle::summary)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(json!({"cycles": summaries}))
    }
}
