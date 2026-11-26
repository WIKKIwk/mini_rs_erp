use super::*;
use sqlx::{Postgres, Transaction};

#[derive(Clone, Copy)]
pub struct WasteEntry<'a> {
    pub request_id: &'a str,
    pub apparatus_id: &'a str,
    pub order_id: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub expected_cycle_id: Option<&'a str>,
    pub kg: f64,
    pub reason: &'a str,
    pub actor: &'a QueueActionActor,
    pub reconcile_manual: bool,
}

impl<'a> WasteEntry<'a> {
    fn normalized(self, origin: Origin) -> Result<Self, WasteBinError> {
        let reason = self.reason.trim();
        let max_request_len = match origin {
            Origin::Manual => 128,
            Origin::Production => 1024,
        };
        let request_id = identity(self.request_id)?;
        if request_id.len() > max_request_len
            || reason.is_empty()
            || reason.len() > 240
            || units(self.kg)? == 0
        {
            return Err(WasteBinError::InvalidInput);
        }
        identity(&self.actor.role)?;
        identity(&self.actor.ref_)?;
        Ok(Self {
            request_id,
            apparatus_id: identity(self.apparatus_id)?,
            order_id: self.order_id.map(identity).transpose()?,
            session_id: self.session_id.map(identity).transpose()?,
            expected_cycle_id: self.expected_cycle_id.map(identity).transpose()?,
            reason,
            reconcile_manual: origin == Origin::Production && self.reconcile_manual,
            ..self
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Manual,
    Production,
}

impl Origin {
    fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Production => "production",
        }
    }
}

#[derive(sqlx::FromRow)]
struct RecordedRequest {
    apparatus_id: String,
    expected_cycle_id: Option<String>,
    order_id: Option<String>,
    session_id: Option<String>,
    reported_units: i64,
    reconcile_manual: bool,
    reason: String,
}

async fn previously_recorded(
    db: &mut PgConnection,
    entry: WasteEntry<'_>,
    origin: Origin,
    reported: i64,
) -> Result<bool, WasteBinError> {
    let existing: Option<RecordedRequest> = sqlx::query_as(
        "SELECT apparatus_id, expected_cycle_id, order_id, session_id, reported_units, reconcile_manual, reason
         FROM mini_waste_bin_entries WHERE origin=$1 AND actor_role=$2 AND actor_ref=$3 AND request_id=$4",
    )
    .bind(origin.as_str())
    .bind(&entry.actor.role)
    .bind(&entry.actor.ref_)
    .bind(entry.request_id)
    .fetch_optional(db)
    .await?;
    let Some(stored) = existing else {
        return Ok(false);
    };
    if stored.apparatus_id != entry.apparatus_id
        || stored.expected_cycle_id.as_deref() != entry.expected_cycle_id
        || stored.order_id.as_deref() != entry.order_id
        || stored.session_id.as_deref() != entry.session_id
        || stored.reported_units != reported
        || stored.reconcile_manual != entry.reconcile_manual
        || stored.reason != entry.reason
    {
        return Err(WasteBinError::Conflict);
    }
    Ok(true)
}

/// Called in the same transaction as the authoritative production event. A
/// retry of that event cannot move an entry to a later active bin.
pub(crate) async fn record_production_waste(
    tx: &mut Transaction<'_, Postgres>,
    entry: &WasteEntry<'_>,
) -> Result<(), WasteBinError> {
    record(tx, *entry, Origin::Production).await
}

async fn record(
    db: &mut PgConnection,
    entry: WasteEntry<'_>,
    origin: Origin,
) -> Result<(), WasteBinError> {
    let entry = entry.normalized(origin)?;
    let reported = units(entry.kg)?;
    lock_apparatus(db, entry.apparatus_id).await?;
    if previously_recorded(db, entry, origin, reported).await? {
        return Ok(());
    }

    let mut cycle_id = access::open_cycle_for_actor(db, entry.apparatus_id, entry.actor).await?;
    if entry.expected_cycle_id.is_some() && entry.expected_cycle_id != cycle_id.as_deref() {
        if origin == Origin::Manual {
            return Err(WasteBinError::Conflict);
        }
        cycle_id = None;
    }

    let entry_id = new_id();
    let created = sqlx::query(
        "INSERT INTO mini_waste_bin_entries
          (id, origin, request_id, apparatus_id, expected_cycle_id, order_id, session_id, reported_units,
           units, reconcile_manual, reason, actor_role, actor_ref, actor_display_name)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,0,$9,$10,$11,$12,$13)
         ON CONFLICT (origin, actor_role, actor_ref, request_id) DO NOTHING",
    )
    .bind(&entry_id)
    .bind(origin.as_str())
    .bind(entry.request_id)
    .bind(entry.apparatus_id)
    .bind(entry.expected_cycle_id)
    .bind(entry.order_id)
    .bind(entry.session_id)
    .bind(reported)
    .bind(entry.reconcile_manual)
    .bind(entry.reason)
    .bind(&entry.actor.role)
    .bind(&entry.actor.ref_)
    .bind(&entry.actor.display_name)
    .execute(&mut *db)
    .await?.rows_affected();

    if created == 0 {
        return if previously_recorded(db, entry, origin, reported).await? {
            Ok(())
        } else {
            Err(WasteBinError::Conflict)
        };
    }

    let unrecorded = if entry.reconcile_manual
        && let Some(order) = entry.order_id
    {
        cover_manual(db, &entry_id, entry, order, reported).await?
    } else {
        reported
    };
    let assigned_cycle = cycle_id.filter(|_| unrecorded > 0);
    sqlx::query(
        "UPDATE mini_waste_bin_entries SET units=$2, cycle_id=$3,
            assigned_at=CASE WHEN $3 IS NULL THEN NULL ELSE now() END,
            assigned_by_ref=CASE WHEN $3 IS NULL THEN NULL ELSE $4 END WHERE id=$1",
    )
    .bind(entry_id)
    .bind(unrecorded)
    .bind(assigned_cycle)
    .bind(&entry.actor.ref_)
    .execute(db)
    .await?;
    Ok(())
}

async fn cover_manual(
    db: &mut PgConnection,
    report_id: &str,
    entry: WasteEntry<'_>,
    order: &str,
    reported: i64,
) -> Result<i64, WasteBinError> {
    let manual: Vec<(String, i64)> = sqlx::query_as(
        "SELECT e.id, e.units - COALESCE((SELECT sum(c.units)::bigint FROM mini_waste_bin_manual_coverage c
            WHERE c.manual_entry_id=e.id), 0) AS uncovered
         FROM mini_waste_bin_entries e
         WHERE e.origin='manual' AND e.apparatus_id=$1 AND e.order_id=$2
            AND e.actor_role=$3 AND e.actor_ref=$4 AND (e.session_id IS NULL OR e.session_id=$5)
         ORDER BY e.created_at, e.id",
    )
    .bind(entry.apparatus_id)
    .bind(order)
    .bind(&entry.actor.role)
    .bind(&entry.actor.ref_)
    .bind(entry.session_id)
    .fetch_all(&mut *db)
    .await?;
    let mut remaining = reported;
    for (id, uncovered) in manual.into_iter().filter(|(_, n)| *n > 0) {
        let claimed = uncovered.min(remaining);
        sqlx::query("INSERT INTO mini_waste_bin_manual_coverage(report_id, manual_entry_id, units) VALUES ($1,$2,$3)")
            .bind(report_id)
            .bind(id)
            .bind(claimed)
            .execute(&mut *db)
            .await?;
        remaining -= claimed;
        if remaining == 0 {
            break;
        }
    }
    Ok(remaining)
}

impl PostgresWasteBinStore {
    pub async fn add(
        &self,
        raw_code: &str,
        cycle_id: &str,
        entry: &WasteEntry<'_>,
    ) -> Result<Value, WasteBinError> {
        let code = code(raw_code)?;
        let mut tx = self.pool.begin().await?;
        let apparatus = cycle_apparatus(&mut tx, &code, cycle_id).await?;
        if apparatus != entry.apparatus_id
            || entry.expected_cycle_id.is_some_and(|id| id != cycle_id)
        {
            return Err(WasteBinError::Conflict);
        }
        lock_apparatus(&mut tx, &apparatus).await?;
        let bins = lock_bins(&mut tx, std::slice::from_ref(&code)).await?;
        if !read::load_cycle(&mut tx, cycle_id)
            .await?
            .allows(&bins[0], entry.actor)
        {
            return Err(WasteBinError::Forbidden);
        }
        let entry = WasteEntry {
            expected_cycle_id: Some(cycle_id),
            ..*entry
        };
        record(&mut tx, entry, Origin::Manual).await?;
        let result = read::bin_snapshot(&mut tx, &code, Some(cycle_id)).await?;
        tx.commit().await?;
        Ok(operator_snapshot(result))
    }

    pub async fn reconcile(
        &self,
        raw_code: &str,
        cycle_id: &str,
        entry_ids: &[String],
        actor: &QueueActionActor,
    ) -> Result<Value, WasteBinError> {
        if entry_ids.is_empty() || entry_ids.len() > 200 {
            return Err(WasteBinError::InvalidInput);
        }
        let code = code(raw_code)?;
        let ids = entry_ids
            .iter()
            .map(|id| identity(id))
            .collect::<Result<Vec<_>, _>>()?;
        let distinct: std::collections::HashSet<_> = ids.iter().copied().collect();
        if ids.len() != distinct.len() {
            return Err(WasteBinError::InvalidInput);
        }
        let mut tx = self.pool.begin().await?;
        let apparatus = cycle_apparatus(&mut tx, &code, cycle_id).await?;
        lock_apparatus(&mut tx, &apparatus).await?;
        let bins = lock_bins(&mut tx, std::slice::from_ref(&code)).await?;
        if !read::load_cycle(&mut tx, cycle_id)
            .await?
            .allows(&bins[0], actor)
        {
            return Err(WasteBinError::Forbidden);
        }
        let rows: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, cycle_id FROM mini_waste_bin_entries
             WHERE id=ANY($1) AND apparatus_id=$2 AND units>0
                AND ($3='admin' OR (actor_role=$3 AND actor_ref=$4))",
        )
        .bind(ids)
        .bind(&apparatus)
        .bind(&actor.role)
        .bind(&actor.ref_)
        .fetch_all(&mut *tx)
        .await?;
        if rows.len() != distinct.len() {
            return Err(WasteBinError::NotFound);
        }
        if rows
            .iter()
            .any(|(_, assigned)| assigned.as_deref().is_some_and(|id| id != cycle_id))
        {
            return Err(WasteBinError::Conflict);
        }
        let pending: Vec<_> = rows
            .iter()
            .filter_map(|(id, assigned)| assigned.is_none().then_some(id.as_str()))
            .collect();
        if !pending.is_empty() {
            if access::open_cycle_for_actor(&mut tx, &apparatus, actor)
                .await?
                .as_deref()
                != Some(cycle_id)
            {
                return Err(WasteBinError::Conflict);
            }
            sqlx::query(
                "UPDATE mini_waste_bin_entries SET cycle_id=$1, assigned_at=now(), assigned_by_ref=$2
                 WHERE id=ANY($3)",
            )
            .bind(cycle_id)
            .bind(&actor.ref_)
            .bind(pending)
            .execute(&mut *tx)
            .await?;
        }
        let result = read::bin_snapshot(&mut tx, &code, Some(cycle_id)).await?;
        tx.commit().await?;
        Ok(operator_snapshot(result))
    }
}
