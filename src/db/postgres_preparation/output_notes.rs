use super::*;
use crate::core::auth::models::PrincipalRole;
use std::collections::BTreeMap;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryRollNotesUpdate {
    rolls: Vec<InventoryRollNoteTarget>,
    text: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InventoryRollNoteTarget {
    asset_ref: String,
    expected_revision: i64,
}

pub(super) fn output_note_json(alias: &str) -> String {
    format!(
        "CASE WHEN {alias}.revision IS NOT NULL THEN jsonb_build_object(
        'text',{alias}.note,'revision',{alias}.revision,
        'updated_by_name',{alias}.updated_by_name,'updated_at',{alias}.updated_at,
        'audience',CASE WHEN {alias}.audience_roles IS NULL
            THEN jsonb_build_object('mode','all')
            ELSE jsonb_build_object('mode','roles','roles',{alias}.audience_roles) END) END"
    )
}

pub type ReceiptOutputNotes = (Option<Value>, Option<Value>);

impl PostgresPreparationStore {
    pub async fn update_output_notes(
        &self,
        actor: &Principal,
        input: &PreparationOutputNoteUpdate,
    ) -> Result<Option<BTreeMap<String, Value>>, PreparationError> {
        let PreparationNoteChanges { target_kind: kind, targets, text, audience } = input.changes()?;
        let owner = actor.ref_.trim();
        if actor.role != PrincipalRole::TayyorlovMasteri || owner.is_empty() {
            return Err(PreparationError::Forbidden);
        }
        let ids: Vec<_> = targets.keys().copied().collect();
        let mut tx = self.pool.begin().await?;
        // Serialize this master's note edits, including rows that do not exist yet.
        lock_output_note_owner(&mut tx, owner).await?;

        let visible: i64 = if kind == "batch" {
            let batch_ids: Vec<_> = ids.iter().filter_map(|id| id.strip_prefix("batch:")).collect();
            sqlx::query_scalar(
                "SELECT count(*) FROM (
                     SELECT batch_id FROM mini_rps_batches
                     WHERE owner_key='tayyorlov_masteri:'||$1 AND owner_role='tayyorlov_masteri'
                       AND owner_ref=$1 AND batch_id=ANY($2)
                     UNION SELECT batch_id FROM mini_rps_batch_history
                     WHERE owner_key='tayyorlov_masteri:'||$1 AND owner_role='tayyorlov_masteri'
                       AND owner_ref=$1 AND batch_id=ANY($2)
                 ) matches",
            )
            .bind(owner)
            .bind(&batch_ids)
            .fetch_one(&mut *tx)
            .await?
        } else {
            sqlx::query_scalar::<_, String>(
                "SELECT name FROM mini_gscale_receipts
                 WHERE name=ANY($2::text[]) AND status='submitted'
                   AND payload_json->>'actor_role'='tayyorlov_masteri' AND payload_json->>'actor_ref'=$1
                 FOR SHARE",
            ).bind(owner).bind(&ids).fetch_all(&mut *tx).await?.len() as i64
        };
        if visible != ids.len() as i64 {
            return Ok(None);
        }

        let current = sqlx::query_as::<_, (String, i64)>(
            "SELECT target_id,revision FROM mini_preparation_output_notes
             WHERE owner_ref=$1 AND target_kind=$2 AND target_id=ANY($3::text[])",
        )
        .bind(owner)
        .bind(kind)
        .bind(&ids)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .collect::<BTreeMap<_, _>>();
        if targets
            .iter()
            .any(|(id, expected)| current.get(*id).copied().unwrap_or(0) != *expected)
        {
            return Err(PreparationError::Conflict(
                "Eslatma o‘zgartirilgan. Qayta tanlang",
            ));
        }
        let revisions: Vec<_> = targets.values().map(|rev| rev + 1).collect();
        let audience_roles = match audience {
            Some(PreparationNoteAudience::Roles { roles }) => {
                Some(roles.iter().map(|role| role.code()).collect::<Vec<_>>())
            }
            _ => None,
        };
        let notes = sqlx::query_as::<_, (String, Value)>(&format!(
            "INSERT INTO mini_preparation_output_notes AS saved
                (owner_ref,target_kind,target_id,note,revision,updated_by_name,audience_roles)
             SELECT $1,$2,target_id,coalesce($3::text,''),revision,$4,$7::text[]
             FROM unnest($5::text[],$6::bigint[]) AS targets(target_id,revision)
             ON CONFLICT (owner_ref,target_kind,target_id) DO UPDATE SET
                note=coalesce($3,saved.note),revision=excluded.revision,
                updated_by_name=excluded.updated_by_name,updated_at=now(),
                audience_roles=CASE WHEN $8 THEN excluded.audience_roles ELSE saved.audience_roles END
             RETURNING saved.target_id,{}", output_note_json("saved")
        )).bind(owner).bind(kind).bind(text).bind(actor.display_name.trim()).bind(&ids).bind(&revisions)
            .bind(audience_roles).bind(audience.is_some())
            .fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(notes.into_iter().collect()))
    }

    pub async fn update_inventory_roll_notes(
        &self,
        actor: &Principal,
        item_groups: &[String],
        input: InventoryRollNotesUpdate,
    ) -> Result<BTreeMap<String, Value>, PreparationError> {
        use super::{READY_ROLL_RECEIPT_SQL, RULON_GROUPS_SQL, SERIYO_GROUPS_SQL};
        use crate::core::preparation::PreparationRollNoteTarget;

        if actor.role != PrincipalRole::MaterialTaminotchi
            || actor.ref_.is_empty()
            || item_groups.is_empty()
        {
            return Err(PreparationError::Forbidden);
        }
        let update = PreparationOutputNoteUpdate::Rolls {
            rolls: input.rolls.into_iter().map(|roll| PreparationRollNoteTarget {
                receipt_name: roll.asset_ref,
                expected_revision: roll.expected_revision,
            }).collect(),
            text: Some(input.text),
            audience: None,
        };
        let changes = update.changes()?;
        let ids: Vec<_> = changes.targets.keys().copied().collect();
        let in_warehouse = physically_in_warehouse!();
        let mut tx = self.pool.begin().await?;
        let scoped: Vec<(String, String, String)> = sqlx::query_as(&format!(
            "WITH RECURSIVE {SERIYO_GROUPS_SQL}, {RULON_GROUPS_SQL}
             SELECT s.id, receipt.name, receipt.payload_json->>'actor_ref'
             FROM mini_raw_material_stock s
             JOIN mini_gscale_receipts receipt ON receipt.name = s.source_receipt_id
                 AND lower(receipt.barcode) = lower(s.barcode)
             JOIN mini_items item ON item.code = s.item_code
             JOIN mini_warehouses w ON lower(w.name) = lower(s.warehouse) AND NOT w.is_group
             WHERE s.id = ANY($1) AND {READY_ROLL_RECEIPT_SQL}
               AND s.status IN ('available', 'on_paddon') AND s.qty > 0 AND s.uom = 'kg'
               AND {in_warehouse} AND lower(btrim(item.item_group)) = ANY($3)
               AND EXISTS (
                    SELECT 1 FROM mini_warehouse_assignments a
                    WHERE lower(a.warehouse_name) = lower(w.name)
                      AND a.assignment_kind = 'warehouse'
                      AND a.principal_role = 'material_taminotchi' AND a.principal_ref = $2
               )
             ORDER BY s.id FOR UPDATE OF s FOR SHARE OF item, receipt"
        ))
        .bind(&ids)
        .bind(&actor.ref_)
        .bind(item_groups)
        .fetch_all(&mut *tx)
        .await?;
        if scoped.len() != ids.len() {
            return Err(PreparationError::Forbidden);
        }

        // Use the same owner locks as the journal so edits from both roles
        // cannot pass the revision check concurrently.
        let owners: std::collections::BTreeSet<_> =
            scoped.iter().map(|(_, _, owner)| owner.as_str()).collect();
        for owner in owners {
            lock_output_note_owner(&mut tx, owner).await?;
        }
        let receipt_names: Vec<_> = scoped.iter().map(|(_, name, _)| name.as_str()).collect();
        let owner_refs: Vec<_> = scoped.iter().map(|(_, _, owner)| owner.as_str()).collect();
        let revisions: Vec<_> = scoped.iter()
            .map(|(asset, _, _)| changes.targets[asset.as_str()]).collect();
        let mut notes: BTreeMap<String, Value> = sqlx::query_as(&format!(
            "INSERT INTO mini_preparation_output_notes AS saved
                (owner_ref,target_kind,target_id,note,revision,updated_by_name)
             SELECT target.owner_ref,'roll',target.receipt_name,$4,target.revision + 1,$5
             FROM unnest($1::text[],$2::text[],$3::bigint[])
                 AS target(receipt_name,owner_ref,revision)
             LEFT JOIN mini_preparation_output_notes current_note
               ON current_note.owner_ref=target.owner_ref AND current_note.target_kind='roll'
               AND current_note.target_id=target.receipt_name
             WHERE COALESCE(current_note.revision,0)=target.revision
               AND (current_note.audience_roles IS NULL
                    OR 'material_taminotchi'=ANY(current_note.audience_roles)
                    OR current_note.owner_ref=$6)
             ON CONFLICT (owner_ref,target_kind,target_id) DO UPDATE SET
                 note=excluded.note,revision=excluded.revision,
                 updated_by_name=excluded.updated_by_name,updated_at=now()
             RETURNING saved.target_id,{}", output_note_json("saved")
        ))
        .bind(receipt_names)
        .bind(owner_refs)
        .bind(revisions)
        .bind(changes.text)
        .bind(actor.display_name.trim())
        .bind(&actor.ref_)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .collect();
        if notes.len() != scoped.len() {
            return Err(PreparationError::Conflict("Eslatma o‘zgartirilgan. Qayta tanlang"));
        }
        let saved = scoped.into_iter().map(|(asset, receipt, _)| {
            notes.remove(&receipt).map(|note| (asset, note))
                .ok_or(PreparationError::StoreFailed)
        }).collect::<Result<BTreeMap<_, _>, _>>()?;
        tx.commit().await?;
        Ok(saved)
    }

    pub async fn output_notes_for_barcode(
        &self,
        barcode: &str,
        viewer: PreparationNoteViewerRole,
        viewer_ref: &str,
    ) -> Result<ReceiptOutputNotes, PreparationError> {
        let mut notes = self
            .output_notes_for_inventory(&[barcode.to_owned()], viewer, viewer_ref)
            .await?;
        Ok(notes
            .remove(&barcode.trim().to_uppercase())
            .unwrap_or_default())
    }

    /// Only call with barcodes already filtered by the inventory reader or an exact scan lookup.
    pub async fn output_notes_for_inventory(
        &self,
        barcodes: &[String],
        viewer: PreparationNoteViewerRole,
        viewer_ref: &str,
    ) -> Result<BTreeMap<String, ReceiptOutputNotes>, PreparationError> {
        self.output_notes_for_inventory_groups(barcodes, viewer, viewer_ref, None).await
    }

    pub async fn output_notes_for_inventory_groups(
        &self,
        barcodes: &[String],
        viewer: PreparationNoteViewerRole,
        viewer_ref: &str,
        item_groups: Option<&[String]>,
    ) -> Result<BTreeMap<String, ReceiptOutputNotes>, PreparationError> {
        if barcodes.is_empty() {
            return Ok(BTreeMap::new());
        }
        let query = format!(
            "{} SELECT r.barcode,{},{}
            FROM receipts r
            LEFT JOIN membership m ON m.owner_ref=r.owner_ref AND m.epc=r.epc
            LEFT JOIN mini_preparation_output_notes batch_note ON batch_note.owner_ref=r.owner_ref
                AND r.from_preparation AND batch_note.target_kind='batch' AND batch_note.target_id='batch:'||m.batch_id
                AND (batch_note.audience_roles IS NULL OR $2=ANY(batch_note.audience_roles) OR $3=batch_note.owner_ref)
            LEFT JOIN mini_preparation_output_notes roll_note ON roll_note.owner_ref=r.owner_ref
                AND roll_note.target_kind='roll' AND roll_note.target_id=r.receipt_name
                AND (roll_note.audience_roles IS NULL OR $2=ANY(roll_note.audience_roles) OR $3=roll_note.owner_ref)",
            include_str!("output_notes_inventory.sql"),
            output_note_json("batch_note"),
            output_note_json("roll_note")
        );
        let barcodes = barcodes
            .iter()
            .map(|code| code.trim().to_lowercase())
            .collect::<Vec<_>>();
        let rows = sqlx::query_as::<_, (String, Option<Value>, Option<Value>)>(&query)
            .bind(barcodes)
            .bind(viewer.code())
            .bind(match viewer {
                PreparationNoteViewerRole::TayyorlovMasteri
                | PreparationNoteViewerRole::MaterialTaminotchi => viewer_ref.trim(),
                _ => "",
            })
            .bind(item_groups)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .filter(|(_, batch, roll)| batch.is_some() || roll.is_some())
            .map(|(barcode, batch, roll)| (barcode, (batch, roll)))
            .collect())
    }
}

async fn lock_output_note_owner(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner: &str,
) -> Result<(), PreparationError> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('preparation-output-note:' || $1, 0))",
    )
    .bind(owner)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
