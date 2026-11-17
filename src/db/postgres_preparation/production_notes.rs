use super::*;
use std::collections::BTreeMap;

pub(super) fn note_json_sql(order_id: &str) -> String {
    format!(
        "jsonb_build_object('order_id',{order_id},'text',COALESCE(note.note,''),
        'revision',COALESCE(note.revision,0),'updated_by_ref',COALESCE(note.updated_by_ref,''),
        'updated_by_name',COALESCE(note.updated_by_name,''),'updated_at',note.updated_at)"
    )
}

impl PostgresPreparationStore {
    pub async fn update_production_order_note(
        &self,
        actor: &Principal,
        input: &PreparationProductionNoteUpdate,
    ) -> Result<Option<Value>, PreparationError> {
        input.validate()?;
        let order_id = input.order_id.trim();
        let mut tx = self.pool.begin().await?;
        // Also serializes creation, when there is no row to lock yet.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('preparation-production-note:' || $1, 0))")
            .bind(order_id).execute(&mut *tx).await?;
        let visible: bool = sqlx::query_scalar(&format!(
            "{} SELECT EXISTS(SELECT 1 FROM journal_orders WHERE order_id=$2)",
            include_str!("production_journal.sql")
        ))
        .bind(actor.ref_.trim())
        .bind(order_id)
        .fetch_one(&mut *tx)
        .await?;
        if !visible {
            return Ok(None);
        }
        let revision: i64 = sqlx::query_scalar(
            "SELECT revision FROM mini_preparation_production_order_notes WHERE order_id=$1 FOR UPDATE"
        ).bind(order_id).fetch_optional(&mut *tx).await?.unwrap_or(0);
        if revision != input.expected_revision {
            return Err(PreparationError::Conflict(
                "Eslatma o‘zgartirilgan. Oynani yopib, qayta oching",
            ));
        }
        let next_revision = revision.checked_add(1).ok_or(PreparationError::Conflict(
            "Eslatma versiyasi chegaraga yetgan",
        ))?;
        let result = sqlx::query_scalar(&format!(
            "INSERT INTO mini_preparation_production_order_notes AS note
                (order_id,note,revision,updated_by_ref,updated_by_name)
             VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT (order_id) DO UPDATE SET note=excluded.note,revision=excluded.revision,
                updated_by_ref=excluded.updated_by_ref,updated_by_name=excluded.updated_by_name,updated_at=now()
             RETURNING {}", note_json_sql("note.order_id")
        ))
        .bind(order_id).bind(input.text.trim()).bind(next_revision)
        .bind(actor.ref_.trim()).bind(actor.display_name.trim())
        .fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(result))
    }

    /// Call only with barcodes already authorized by the inventory reader.
    /// Use original receipt provenance rather than a roll's current order assignment.
    pub async fn production_notes_for_receipts(
        &self,
        barcodes: &[String],
    ) -> Result<BTreeMap<String, Value>, PreparationError> {
        if barcodes.is_empty() {
            return Ok(BTreeMap::new());
        }
        let order_id = "COALESCE(NULLIF(origin.order_id,''),NULLIF(receipt.payload_json#>>'{order_assignment,order_id}',''))";
        let rows: Vec<(String, Value)> = sqlx::query_as(&format!(
            "SELECT upper(btrim(stock.barcode)),{} FROM mini_raw_material_stock stock
             JOIN mini_gscale_receipts receipt ON receipt.name=stock.source_receipt_id
             LEFT JOIN LATERAL (
                SELECT event.order_id FROM mini_raw_material_events event
                WHERE event.barcode_key=lower(stock.barcode) AND event.event_type='receipt_posted'
                    AND event.source_type='gscale_receipt'
                ORDER BY event.occurred_at,event.event_id LIMIT 1
             ) origin ON true
             LEFT JOIN mini_preparation_production_order_notes note ON note.order_id={order_id}
             WHERE lower(stock.barcode)=ANY($1::text[]) AND receipt.status='submitted'
                AND receipt.payload_json->>'actor_role'='tayyorlov_masteri' AND {order_id} IS NOT NULL",
            note_json_sql(order_id)
        ))
        .bind(barcodes.iter().map(|code| code.trim().to_lowercase()).collect::<Vec<_>>())
        .fetch_all(&self.pool).await?;
        Ok(rows.into_iter().collect())
    }
}
