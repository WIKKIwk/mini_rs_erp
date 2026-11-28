use super::attachments::{
    STOCK_WAREHOUSE, save_paddon_attachment, validate_attachment_key, validate_roll_selection,
};
use super::issues::movement_error;
use super::*;
use crate::db::postgres_inventory_movements::ensure_raw_material_destination_scope_tx;
use crate::db::postgres_preparation::{READY_ROLL_RECEIPT_SQL, RULON_GROUPS_SQL, SERIYO_GROUPS_SQL};
use std::collections::BTreeSet;

const INVENTORY_ROLL_STOCK: &str = "FROM mini_raw_material_stock s
    JOIN mini_gscale_receipts receipt ON receipt.name = s.source_receipt_id
        AND lower(receipt.barcode) = lower(s.barcode)";

const INVENTORY_SCOPE: &str = "
    JOIN mini_items item ON item.code = s.item_code
    WHERE s.id = ANY($1)
      AND lower(btrim(item.item_group)) = ANY($3)
      AND ($4 = '' OR w.id = $4)
      AND EXISTS (
          SELECT 1 FROM mini_warehouse_assignments a
          WHERE lower(a.warehouse_name) = lower(w.name) AND a.assignment_kind = 'warehouse'
            AND a.principal_role = 'material_taminotchi' AND a.principal_ref = $2
      )";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryPaddonRollOptions {
    asset_refs: Vec<String>,
    #[serde(default)]
    q: String,
    #[serde(default)]
    offset: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryPaddonRollAttachment {
    asset_refs: Vec<String>,
    code: String,
    request_id: String,
}

#[derive(sqlx::FromRow)]
struct InventoryRoll {
    stock_id: String,
    receipt_name: String,
    item_code: String,
    warehouse_id: String,
    warehouse: String,
    available: bool,
}

impl PostgresPreparationStore {
    pub async fn inventory_paddon_options(
        &self,
        actor: &Principal,
        item_groups: &[String],
        mut input: InventoryPaddonRollOptions,
    ) -> Result<Value, PreparationError> {
        require_inventory_packing_scope(actor, item_groups)?;
        validate_roll_selection(&mut input.asset_refs, false)?;
        let mut tx = self.pool.begin().await?;
        let rolls = inventory_rolls(&mut tx, actor, item_groups, &input.asset_refs, None).await?;
        let warehouse_ids = [rolls[0].warehouse_id.clone()];
        tx.commit().await?;
        self.read_raw_material_paddons(
            actor,
            PaddonQuery {
                q: input.q,
                offset: input.offset,
                ..PaddonQuery::default()
            },
            50,
            Some(&warehouse_ids),
        )
        .await
    }

    pub async fn attach_inventory_rolls(
        &self,
        actor: &Principal,
        item_groups: &[String],
        mut input: InventoryPaddonRollAttachment,
    ) -> Result<Value, PreparationError> {
        require_inventory_packing_scope(actor, item_groups)?;
        validate_roll_selection(&mut input.asset_refs, false)?;
        validate_attachment_key(&input.request_id)?;
        let code = paddon_code(&input.code)?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!(
                "paddon-attach:material_taminotchi:{}:{}",
                actor.ref_, input.request_id
            ))
            .execute(&mut *tx)
            .await?;
        lock_paddon(&mut tx, &code).await?;
        let (paddon_id, warehouse_id, warehouse) = scoped_paddon(&mut tx, actor, &code).await?;
        let result =
            json!({"paddon_code": code, "warehouse": warehouse, "asset_refs": input.asset_refs});
        let previous: Option<(String, Vec<String>)> = sqlx::query_as(
            "SELECT paddon_id, stock_ids FROM mini_raw_material_paddon_attachments
             WHERE owner_role = 'material_taminotchi' AND owner_ref = $1 AND request_id = $2",
        )
        .bind(&actor.ref_)
        .bind(&input.request_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((saved_paddon, saved_assets)) = previous {
            if saved_paddon != paddon_id || saved_assets != input.asset_refs {
                return Err(PreparationError::Conflict(
                    "Biriktirish kaliti avval ishlatilgan",
                ));
            }
            return Ok(result);
        }
        let rolls = inventory_rolls(
            &mut tx,
            actor,
            item_groups,
            &input.asset_refs,
            Some(&warehouse_id),
        )
        .await?;
        let receipt_names: Vec<_> = rolls.into_iter().map(|roll| roll.receipt_name).collect();
        save_paddon_attachment(
            &mut tx,
            actor,
            &input.request_id,
            &paddon_id,
            &receipt_names,
            &input.asset_refs,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }
}

fn require_inventory_packing_scope(
    actor: &Principal,
    item_groups: &[String],
) -> Result<(), PreparationError> {
    if actor.role != PrincipalRole::MaterialTaminotchi
        || actor.ref_.trim().is_empty()
        || item_groups.is_empty()
    {
        return Err(PreparationError::Forbidden);
    }
    Ok(())
}

async fn inventory_rolls(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Principal,
    item_groups: &[String],
    asset_refs: &[String],
    target_warehouse_id: Option<&str>,
) -> Result<Vec<InventoryRoll>, PreparationError> {
    let groups = format!("WITH RECURSIVE {SERIYO_GROUPS_SQL}, {RULON_GROUPS_SQL}");
    let scope = format!(
        "{INVENTORY_ROLL_STOCK} {STOCK_WAREHOUSE} {INVENTORY_SCOPE} AND {READY_ROLL_RECEIPT_SQL}"
    );
    let warehouse_id = target_warehouse_id.unwrap_or_default();
    let out_of_scope = PreparationError::Conflict(
        "Shu ombordagi va sizga biriktirilgan guruhdagi QR kodli tayyor rulonlarni tanlang",
    );
    if target_warehouse_id.is_some() {
        let locked: Vec<String> = sqlx::query_scalar(&format!(
            "{groups} SELECT s.id {scope} ORDER BY s.id FOR UPDATE OF s FOR SHARE OF item"
        ))
        .bind(asset_refs)
        .bind(&actor.ref_)
        .bind(item_groups)
        .bind(warehouse_id)
        .fetch_all(&mut **tx)
        .await?;
        if locked != asset_refs {
            return Err(out_of_scope);
        }
    }
    let free = free_in_warehouse!();
    let rolls = sqlx::query_as::<_, InventoryRoll>(&format!(
        "{groups} SELECT s.id AS stock_id, receipt.name AS receipt_name, s.item_code, w.id AS warehouse_id,
                w.name AS warehouse, (s.status = 'available' AND s.qty > 0 AND s.uom = 'kg'
                    AND {free} AND NOT EXISTS (
                        SELECT 1 FROM mini_raw_material_paddon_units u
                        WHERE u.stock_id = s.id AND u.issue_id IS NULL
                    )) AS available
         {scope} ORDER BY s.id"
    ))
    .bind(asset_refs)
    .bind(&actor.ref_)
    .bind(item_groups)
    .bind(warehouse_id)
    .fetch_all(&mut **tx)
    .await?;
    if !rolls.iter().map(|roll| &roll.stock_id).eq(asset_refs) {
        return Err(out_of_scope);
    }
    let first = &rolls[0];
    if rolls
        .iter()
        .any(|roll| roll.warehouse_id != first.warehouse_id)
    {
        return Err(PreparationError::Conflict(
            "Bitta ombordagi rulonlarni tanlang",
        ));
    }
    if rolls.iter().any(|roll| !roll.available) {
        return Err(PreparationError::Conflict(
            "Band, buyurtmaga ulangan yoki omborda bo‘lmagan rulonni biriktirib bo‘lmaydi",
        ));
    }
    let item_codes: BTreeSet<_> = rolls.iter().map(|roll| roll.item_code.as_str()).collect();
    for item in item_codes {
        ensure_raw_material_destination_scope_tx(tx, item, &first.warehouse_id, &first.warehouse)
            .await
            .map_err(movement_error)?;
    }
    Ok(rolls)
}
