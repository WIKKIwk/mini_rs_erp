use super::*;
use super::issues::movement_error;
use crate::core::inventory_movements::{
    InventoryActor, InventoryAssetKind, InventoryAssetSelector, InventoryMovementError,
    InventoryTransferCreate, InventoryTransferStatus,
};
use crate::db::postgres_inventory_movements::{
    PostgresInventoryMovementStore, ensure_raw_material_destination_scope_tx,
};
use crate::db::postgres_preparation::production_journal::BASE as JOURNAL_CTES;
use std::collections::{BTreeMap, BTreeSet};

const STOCK_WAREHOUSE: &str = "LEFT JOIN mini_warehouses w
    ON lower(w.name) = lower(s.warehouse) AND NOT w.is_group";
const ROLL_BLOCKER: &str = concat!(
    "CASE WHEN s.id IS NULL THEN 'missing'
          WHEN s.status = 'on_paddon' OR EXISTS (SELECT 1 FROM mini_raw_material_paddon_units u
              WHERE u.stock_id = s.id AND u.issue_id IS NULL) THEN 'packed'
          WHEN s.status = 'consumed' THEN 'consumed'
          WHEN s.status <> 'available' OR s.qty IS NULL OR s.qty <= 0 OR s.uom <> 'kg'
              THEN 'unavailable'
          WHEN NOT (",
    physically_in_warehouse!(),
    ")
            OR NOT EXISTS (SELECT 1 FROM mini_warehouse_assignments a
                WHERE lower(a.warehouse_name) = lower(w.name) AND a.assignment_kind = 'warehouse'
                  AND a.principal_role = 'tayyorlov_masteri' AND a.principal_ref = $1)
              THEN 'outside_warehouse'
     END",
);

const ROLL_STOCK: &str = "FROM mini_gscale_receipts r
    JOIN mini_raw_material_stock s ON s.source_receipt_id = r.name
        AND lower(s.barcode) = lower(r.barcode)";
const SELECTED_RECEIPTS: &str = "WHERE r.name = ANY($2::text[]) AND r.status = 'submitted'
      AND r.payload_json->>'actor_role' = 'tayyorlov_masteri'
      AND r.payload_json->>'actor_ref' = $1";
const PADDON_LOCATION: &str = "LEFT JOIN mini_raw_material_paddon_units u
    ON u.stock_id = s.id AND u.issue_id IS NULL
    LEFT JOIN mini_raw_material_paddons p ON p.id = u.paddon_id
        AND p.owner_role = 'tayyorlov_masteri' AND p.owner_ref = $1
        AND EXISTS (
            SELECT 1 FROM mini_warehouses pw
            JOIN mini_warehouse_assignments a ON lower(a.warehouse_name) = lower(pw.name)
            WHERE pw.id = p.warehouse_id AND NOT pw.is_group
                AND a.assignment_kind = 'warehouse'
                AND a.principal_role = 'tayyorlov_masteri' AND a.principal_ref = $1
        )";
const PREVIEW_FIELDS: &str = "s.id AS stock_id, s.item_code, w.id AS warehouse_id, w.name AS warehouse,
    p.code AS paddon_code, p.name AS paddon_name";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaddonRollOptions {
    batch_id: Option<String>,
    batch_ids: Option<Vec<String>>,
    #[serde(default)]
    receipt_names: Vec<String>,
    #[serde(default)]
    offset: u32,
    #[serde(skip)]
    pub transfer_between_warehouses: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaddonRollAttachment {
    code: String,
    request_id: String,
    receipt_names: Vec<String>,
    batch_id: Option<String>,
    batch_ids: Option<Vec<String>>,
    #[serde(default)]
    transfer_between_warehouses: bool,
}

fn normalize_batch_ids(
    batch_id: Option<String>,
    batch_ids: Option<Vec<String>>,
) -> Result<Vec<String>, PreparationError> {
    let mut ids = match (batch_id, batch_ids) {
        (None, None) => return Ok(Vec::new()),
        (Some(id), None) => vec![id],
        (None, Some(ids)) => ids,
        (Some(_), Some(_)) => return Err(PreparationError::Invalid("Batch tanlovi noto‘g‘ri")),
    };
    if !(1..=MAX_PREPARATION_BATCH_SELECTION).contains(&ids.len())
        || ids.iter().any(|id| {
            id.len() > 1024
                || id.contains('\0')
                || id
                    .strip_prefix("batch:")
                    .is_none_or(|id| id.trim().is_empty())
        })
    {
        return Err(PreparationError::Invalid("1 dan 100 tagacha batch tanlang"));
    }
    ids.sort_unstable();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(PreparationError::Invalid("Batch faqat bir marta tanlanadi"));
    }
    Ok(ids)
}

fn validate_roll_selection(
    names: &mut [String],
    from_batches: bool,
) -> Result<(), PreparationError> {
    if !from_batches && !(1..=100).contains(&names.len()) {
        return Err(PreparationError::Invalid("1–100 ta rulon tanlang"));
    }
    if names.is_empty()
        || names
            .iter()
            .any(|name| name.trim().is_empty() || name.len() > 1024 || name.contains('\0'))
    {
        return Err(PreparationError::Invalid("Rulon tanlovi noto‘g‘ri"));
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
        preview_partial: bool,
    ) -> Result<Value, PreparationError> {
        require_preparation_master(actor)?;
        let batch_ids = normalize_batch_ids(input.batch_id, input.batch_ids)?;
        let from_batches = !batch_ids.is_empty();
        if from_batches {
            if !input.receipt_names.is_empty() {
                return Err(PreparationError::Invalid("Batch yoki rulonlarni tanlang"));
            }
        } else {
            validate_roll_selection(&mut input.receipt_names, false)?;
        }
        let mut tx = self.pool.begin().await?;
        let preview = from_batches || preview_partial || input.transfer_between_warehouses;
        let (warehouse_ids, selection) = if preview {
            let rolls = if from_batches {
                batch_rolls(&mut tx, actor, &batch_ids).await?
            } else {
                selected_rolls(&mut tx, actor, &input.receipt_names).await?
            };
            let (mut warehouses, mut selection) = preview_roll_selection(&rolls, &batch_ids);
            if input.transfer_between_warehouses && !warehouses.is_empty() {
                let targets = transfer_warehouses(&mut tx, actor, &rolls).await?;
                let (ids, names): (Vec<_>, Vec<_>) = targets.into_iter().unzip();
                warehouses.extend(ids);
                selection["transfer_warehouses"] = json!(names);
            }
            (warehouses, Some(selection))
        } else {
            let (warehouse_id, _) = eligible_rolls(&mut tx, actor, &input.receipt_names).await?;
            (vec![warehouse_id], None)
        };
        tx.commit().await?;
        let mut page = self
            .read_raw_material_paddons(
                actor,
                PaddonQuery {
                    offset: input.offset,
                    ..PaddonQuery::default()
                },
                50,
                Some(&warehouse_ids),
            )
            .await?;
        if let Some(selection) = selection {
            let key = if from_batches {
                "batch_selection"
            } else {
                "roll_selection"
            };
            page[key] = selection;
        }
        Ok(page)
    }

    pub async fn attach_prepared_rolls(
        &self,
        actor: &Principal,
        mut input: PaddonRollAttachment,
    ) -> Result<Value, PreparationError> {
        require_preparation_master(actor)?;
        let batch_ids = normalize_batch_ids(input.batch_id, input.batch_ids)?;
        validate_roll_selection(&mut input.receipt_names, !batch_ids.is_empty())?;
        let code = paddon_code(&input.code)?;
        if !(8..=128).contains(&input.request_id.len())
            || !input
                .request_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        {
            return Err(PreparationError::Invalid("Biriktirish kaliti noto‘g‘ri"));
        }
        let attachment_key = format!("paddon-attach:{}:{}", actor.ref_, input.request_id);
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(&attachment_key)
            .execute(&mut *tx)
            .await?;
        lock_paddon(&mut tx, &code).await?;
        let (paddon_id, warehouse_id, warehouse) = scoped_paddon(&mut tx, actor, &code).await?;
        let mut result = json!({
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
            let origins: Vec<String> = sqlx::query_scalar(
                "SELECT source_warehouse FROM mini_inventory_transfers
                 WHERE requested_by_role = 'tayyorlov_masteri' AND requested_by_ref = $1
                   AND starts_with(idempotency_key, $2)
                 ORDER BY source_warehouse_id",
            )
            .bind(&actor.ref_)
            .bind(format!("{attachment_key}:"))
            .fetch_all(&mut *tx)
            .await?;
            if !origins.is_empty() {
                result["transferred_from"] = json!(origins);
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
        let from_batches = !batch_ids.is_empty();
        let stock_ids = if from_batches || input.transfer_between_warehouses {
            let rolls = if from_batches {
                batch_rolls(&mut tx, actor, &batch_ids).await?
            } else {
                selected_rolls(&mut tx, actor, &input.receipt_names).await?
            };
            let mut eligible: Vec<_> = rolls
                .into_iter()
                .filter(|roll| {
                    roll.blocker.is_none()
                        && roll.stock_id.is_some()
                        && (input.transfer_between_warehouses
                            || roll.warehouse_id.as_deref() == Some(&warehouse_id))
                })
                .collect();
            eligible.sort_unstable_by(|first, second| first.receipt_name.cmp(&second.receipt_name));
            if !eligible
                .iter()
                .map(|roll| &roll.receipt_name)
                .eq(&input.receipt_names)
            {
                return Err(PreparationError::Conflict(
                    "Rulonlar holati o‘zgargan. Tanlovni qayta tekshiring",
                ));
            }
            if input.transfer_between_warehouses {
                let origins = transfer_prepared_rolls(
                    &mut tx,
                    actor,
                    &eligible,
                    &warehouse_id,
                    &warehouse,
                    &code,
                    &attachment_key,
                )
                .await?;
                if !origins.is_empty() {
                    result["transferred_from"] = json!(origins);
                }
            }
            eligible.into_iter().filter_map(|roll| roll.stock_id).collect()
        } else {
            let (origin, stock_ids) = eligible_rolls(&mut tx, actor, &input.receipt_names).await?;
            if origin != warehouse_id {
                return Err(PreparationError::Conflict(
                    "Paddon va rulonlar bitta omborda bo‘lishi kerak",
                ));
            }
            stock_ids
        };

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
        {STOCK_WAREHOUSE} {SELECTED_RECEIPTS} AND ({ROLL_BLOCKER}) IS NULL",
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

#[derive(sqlx::FromRow)]
struct PaddonRoll {
    batch_id: Option<String>,
    receipt_name: String,
    barcode: Option<String>,
    stock_id: Option<String>,
    item_code: Option<String>,
    warehouse_id: Option<String>,
    warehouse: Option<String>,
    blocker: Option<String>,
    paddon_code: Option<String>,
    paddon_name: Option<String>,
}

async fn transfer_warehouses(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Principal,
    rolls: &[PaddonRoll],
) -> Result<Vec<(String, String)>, PreparationError> {
    let warehouses: Vec<(String, String)> = sqlx::query_as(
        "SELECT w.id, w.name FROM mini_warehouses w
         WHERE NOT w.is_group
           AND EXISTS (SELECT 1 FROM mini_warehouse_assignments a
                       WHERE lower(a.warehouse_name) = lower(w.name) AND a.assignment_kind = 'warehouse'
                         AND a.principal_role = 'tayyorlov_masteri' AND a.principal_ref = $1)
           AND EXISTS (SELECT 1 FROM mini_inventory_locations l
                       WHERE l.warehouse_id = w.id AND l.kind = 'warehouse' AND l.active)
         ORDER BY w.id",
    )
    .bind(&actor.ref_)
    .fetch_all(&mut **tx)
    .await?;
    let eligible: Vec<_> = rolls.iter().filter(|roll| roll.blocker.is_none()).collect();
    let ready: BTreeSet<_> = warehouses.iter().map(|(id, _)| id.as_str()).collect();
    if eligible.iter().any(|roll| {
        !roll
            .warehouse_id
            .as_deref()
            .is_some_and(|id| ready.contains(id))
    }) {
        return Ok(Vec::new());
    }
    let mut targets = Vec::new();
    'destination: for (id, name) in warehouses {
        let items: BTreeSet<_> = eligible
            .iter()
            .filter(|roll| roll.warehouse_id.as_deref() != Some(id.as_str()))
            .filter_map(|roll| roll.item_code.as_deref())
            .collect();
        if items.is_empty() {
            continue;
        }
        for item in items {
            match ensure_raw_material_destination_scope_tx(tx, item, &id, &name).await {
                Ok(()) => {}
                Err(InventoryMovementError::MaterialWarehouseScopeMissing) => continue 'destination,
                Err(error) => return Err(movement_error(error)),
            }
        }
        targets.push((id, name));
    }
    Ok(targets)
}

async fn transfer_prepared_rolls(
    tx: &mut Transaction<'_, Postgres>,
    principal: &Principal,
    rolls: &[PaddonRoll],
    destination_id: &str,
    destination: &str,
    code: &str,
    attachment_key: &str,
) -> Result<Vec<String>, PreparationError> {
    let mut origins = BTreeMap::<_, Vec<_>>::new();
    for roll in rolls {
        let (Some(id), Some(name), Some(stock_id)) =
            (&roll.warehouse_id, &roll.warehouse, &roll.stock_id)
        else {
            return Err(PreparationError::Conflict("Rulon ombori aniqlanmadi"));
        };
        if id != destination_id {
            origins
                .entry((id, name))
                .or_default()
                .push(InventoryAssetSelector {
                    asset_kind: InventoryAssetKind::RawMaterial,
                    asset_ref: stock_id.clone(),
                });
        }
    }
    if origins.is_empty() {
        return Ok(Vec::new());
    }
    let mut sources = Vec::with_capacity(origins.len());
    for (_, name) in origins.keys() {
        sources.push(assigned_warehouse(tx, &principal.ref_, name).await?);
    }
    let actor = InventoryActor::new(
        principal.clone(),
        false,
        sources.iter().cloned().chain([destination.into()]),
    );
    for (index, ((source_warehouse_id, _), assets)) in origins.into_iter().enumerate() {
        let status = PostgresInventoryMovementStore::create_paddon_transfer_assets_tx(
            tx,
            &actor,
            &new_id(),
            &InventoryTransferCreate {
                source_warehouse_id: source_warehouse_id.clone(),
                destination_warehouse_id: destination_id.into(),
                assets,
                note: format!("Rulonlarni {code} paddoniga biriktirish"),
                idempotency_key: format!("{attachment_key}:{index}"),
            },
        )
        .await
        .map_err(movement_error)?;
        if status != InventoryTransferStatus::Received {
            return Err(PreparationError::Forbidden);
        }
    }
    Ok(sources)
}

fn preview_roll_selection(rolls: &[PaddonRoll], batch_ids: &[String]) -> (Vec<String>, Value) {
    let mut warehouse_ids = BTreeSet::new();
    let mut receipts = BTreeMap::<_, Vec<_>>::new();
    let mut skipped = BTreeMap::new();
    let mut packed = Vec::new();
    for roll in rolls {
        if let Some(reason) = &roll.blocker {
            *skipped.entry(reason).or_insert(0) += 1;
            if reason == "packed" {
                packed.push(json!({
                    "barcode": roll.barcode,
                    "paddon_name": roll.paddon_name,
                    "paddon_code": roll.paddon_code
                }));
            }
        } else if let (Some(id), Some(name)) = (&roll.warehouse_id, &roll.warehouse) {
            warehouse_ids.insert(id.clone());
            receipts.entry(name).or_default().push(&roll.receipt_name);
        }
    }
    let mut selection = json!({
        "batch_ids": batch_ids,
        "total_rolls": rolls.len(),
        "receipt_names_by_warehouse": receipts,
        "skipped": skipped,
        "packed_rolls": packed
    });
    if let [batch_id] = batch_ids {
        selection["batch_id"] = json!(batch_id);
    }
    (warehouse_ids.into_iter().collect(), selection)
}

async fn selected_rolls(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Principal,
    names: &[String],
) -> Result<Vec<PaddonRoll>, PreparationError> {
    let rolls = sqlx::query_as(&format!(
        "SELECT NULL::text AS batch_id, r.name AS receipt_name, r.barcode, {PREVIEW_FIELDS},
                ({ROLL_BLOCKER}) AS blocker
         FROM mini_gscale_receipts r
         LEFT JOIN mini_raw_material_stock s ON s.source_receipt_id = r.name
             AND lower(s.barcode) = lower(r.barcode)
         {STOCK_WAREHOUSE} {PADDON_LOCATION} {SELECTED_RECEIPTS}
         ORDER BY r.name",
    ))
    .bind(&actor.ref_)
    .bind(names)
    .fetch_all(&mut **tx)
    .await?;
    if rolls.len() != names.len() {
        return Err(PreparationError::Forbidden);
    }
    Ok(rolls)
}

async fn batch_rolls(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Principal,
    batch_ids: &[String],
) -> Result<Vec<PaddonRoll>, PreparationError> {
    let mut rolls: Vec<PaddonRoll> = sqlx::query_as(&format!(
        "{JOURNAL_CTES}
         SELECT b.batch_id, COALESCE(r.receipt_name, '') AS receipt_name, r.epc AS barcode,
                {PREVIEW_FIELDS}, ({ROLL_BLOCKER}) AS blocker
         FROM batches b
         LEFT JOIN rolls r ON r.recorded_batch_id = b.batch_id
         LEFT JOIN mini_raw_material_stock s ON s.source_receipt_id = r.receipt_name
             AND lower(s.barcode) = lower(r.epc)
         {STOCK_WAREHOUSE} {PADDON_LOCATION} WHERE b.batch_id = ANY($2)
         ORDER BY r.receipt_name",
    ))
    .bind(&actor.ref_)
    .bind(batch_ids)
    .fetch_all(&mut **tx)
    .await?;
    let found: BTreeSet<_> = rolls
        .iter()
        .filter_map(|roll| roll.batch_id.as_deref())
        .collect();
    if found.len() != batch_ids.len() {
        return Err(PreparationError::Forbidden);
    }
    rolls.retain(|roll| !roll.receipt_name.is_empty());
    Ok(rolls)
}
