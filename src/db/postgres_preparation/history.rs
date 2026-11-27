use super::{
    history_documents::operation_history_documents, transfer_receipt_scope::pending_lots_sql, *,
};
use crate::db::postgres_inventory_movements::load_preparation_history_transfers;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum HistoryFilter {
    #[default]
    All,
    Transfer,
    Receipt,
    Consumption,
}

impl HistoryFilter {
    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Transfer => "transfer",
            Self::Receipt => "receipt",
            Self::Consumption => "consumption",
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct PreparationHistoryQuery {
    warehouse_id: String,
    kind: HistoryFilter,
    before_time: Option<String>,
    before_key: Option<String>,
    limit: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct HistoryRow {
    key: String,
    id: String,
    kind: String,
    time: String,
    preparation_received_at: Option<String>,
    preparation_receipts: Option<Value>,
}

impl PostgresPreparationStore {
    pub(crate) async fn history(
        &self,
        owner: &str,
        query: &PreparationHistoryQuery,
    ) -> Result<Value, PreparationError> {
        match (&query.before_time, &query.before_key) {
            (None, None) => {}
            (Some(time), Some(key))
                if chrono::DateTime::parse_from_rfc3339(time).is_ok() && !key.trim().is_empty() => {
            }
            _ => return Err(PreparationError::Invalid("Tarix sahifasi noto‘g‘ri")),
        }
        let limit = query.limit.unwrap_or(40).clamp(1, 100);
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await?;
        let pending =
            pending_lots_sql("SELECT id FROM transfer_candidates WHERE status = 'received'");
        let sql = HISTORY_SQL.replace("{pending_lots}", &pending);
        let mut rows = sqlx::query_as::<_, HistoryRow>(&sql)
            .bind(query.warehouse_id.trim())
            .bind(owner)
            .bind(query.kind.as_str())
            .bind(query.before_time.as_deref())
            .bind(query.before_key.as_deref())
            .bind(limit + 1)
            .fetch_all(&mut *tx)
            .await?;
        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);

        let (transfer_ids, operation_ids): (Vec<_>, Vec<_>) =
            rows.iter().partition(|row| row.kind == "transfer");
        let transfer_ids: Vec<_> = transfer_ids.into_iter().map(|row| row.id.clone()).collect();
        let operation_ids: Vec<_> = operation_ids
            .into_iter()
            .map(|row| row.id.clone())
            .collect();
        let mut transfers: BTreeMap<_, _> =
            load_preparation_history_transfers(&mut tx, &transfer_ids)
                .await
                .map_err(|_| PreparationError::StoreFailed)?
                .into_iter()
                .map(|transfer| (transfer.id.clone(), transfer))
                .collect();
        let mut documents = BTreeMap::new();
        if !operation_ids.is_empty() {
            for document in
                operation_history_documents(&mut tx, owner, Some(&operation_ids), limit).await?
            {
                let id = document["id"]
                    .as_str()
                    .ok_or(PreparationError::StoreFailed)?;
                documents.insert(id.to_owned(), document);
            }
        }
        tx.commit().await?;

        let mut entries = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut entry = json!({"key": row.key, "kind": row.kind, "occurred_at": row.time});
            if row.kind == "transfer" {
                let transfer = transfers
                    .remove(&row.id)
                    .ok_or(PreparationError::StoreFailed)?;
                entry["transfer"] = json!(transfer);
                entry["preparation_received_at"] = json!(row.preparation_received_at);
                if let Some(receipts) = &row.preparation_receipts {
                    entry["preparation_receipts"] = receipts.clone();
                }
            } else {
                let operation = documents
                    .remove(&row.id)
                    .ok_or(PreparationError::StoreFailed)?;
                entry["operation"] = operation;
            }
            entries.push(entry);
        }
        let next_cursor = rows
            .last()
            .filter(|_| has_more)
            .map(|row| json!({"time": row.time, "key": row.key}));
        Ok(json!({"entries": entries, "next_cursor": next_cursor}))
    }
}

const HISTORY_SQL: &str = r#"
WITH assigned AS (
    SELECT DISTINCT w.id, lower(w.name) AS name
    FROM mini_warehouse_assignments a
    JOIN mini_warehouses w
      ON lower(w.name) = lower(COALESCE(NULLIF(btrim(a.warehouse_name), ''), btrim(a.warehouse)))
    WHERE a.assignment_kind = 'warehouse' AND a.principal_role = 'tayyorlov_masteri'
      AND a.principal_ref = $2 AND NOT w.is_group
),
selected AS (
    SELECT id, name FROM assigned WHERE id = $1
),
transfer_candidates AS (
    SELECT t.id, t.status, t.destination_warehouse_id,
           COALESCE(t.received_at, t.rejected_at, t.cancelled_at, t.created_at) AS terminal_at
    FROM mini_inventory_transfers t
    WHERE $3 <> 'consumption'
      AND t.status IN ('received', 'rejected', 'cancelled')
      AND (t.source_warehouse_id IN (SELECT id FROM assigned)
           OR t.destination_warehouse_id IN (SELECT id FROM assigned))
      AND ($1 = '' OR EXISTS (
          SELECT 1 FROM selected WHERE id IN (t.source_warehouse_id, t.destination_warehouse_id)
      ))
      AND EXISTS (SELECT 1 FROM mini_inventory_transfer_lines l WHERE l.transfer_id = t.id)
      AND NOT EXISTS (
          SELECT 1 FROM mini_inventory_transfer_lines l
          WHERE l.transfer_id = t.id AND l.asset_kind <> 'raw_material'
      )
),
pending AS ({pending_lots}),
transfers AS (
    SELECT t.* FROM transfer_candidates t
    WHERE NOT EXISTS (SELECT 1 FROM pending WHERE transfer_id = t.id)
),
operations AS (
    SELECT o.id, o.kind, o.created_at, o.request_json,
           link.warehouse_id, o.response_json->>'warehouse' AS warehouse,
           o.response_json->>'name' AS material, o.response_json->>'kg' AS kg
    FROM mini_preparation_operations o
    LEFT JOIN mini_preparation_warehouse_history_names link ON link.operation_id = o.id
    WHERE o.owner_ref = $2 AND o.kind IN ('receipt', 'consumption', 'receipt_reversal')
      AND o.request_json->>'kind' IS DISTINCT FROM 'paddon_portion'
),
receipt_links AS (
    SELECT o.id, o.created_at, o.material, o.kg,
           COALESCE(NULLIF(o.request_json->>'transfer_id', ''), legacy.id) AS transfer_id
    FROM operations o
    LEFT JOIN LATERAL (
        -- Older automatic receipts and transfers were written in the same transaction.
        SELECT t.id
        FROM mini_inventory_transfer_lines l
        JOIN mini_inventory_transfers t ON t.id = l.transfer_id
        WHERE NULLIF(o.request_json->>'transfer_id', '') IS NULL
          AND l.asset_kind = 'raw_material'
          AND l.asset_ref = o.request_json->>'stock_id'
          AND t.status = 'received' AND t.received_at = o.created_at
          AND (t.destination_warehouse_id = o.warehouse_id
               OR (o.warehouse_id IS NULL AND lower(t.destination_warehouse) = lower(o.warehouse)))
        ORDER BY t.id
        LIMIT 1
    ) legacy ON true
    WHERE $3 <> 'consumption' AND o.kind = 'receipt'
      AND o.request_json->>'kind' IN ('warehouse_transfer', 'paddon_transfer')
),
transfer_receipts AS (
    SELECT transfer_id, max(created_at) AS received_at,
           jsonb_agg(jsonb_build_object('material', material, 'kg', kg, 'received_at', created_at)
                     ORDER BY created_at DESC, id DESC) AS receipts
    FROM receipt_links
    WHERE transfer_id IS NOT NULL
    GROUP BY transfer_id
),
combined AS (
    SELECT 'transfer:' || t.id AS key, t.id, 'transfer' AS kind,
           COALESCE(r.received_at, t.terminal_at) AS occurred_at,
           r.received_at AS preparation_received_at, r.receipts AS preparation_receipts
    FROM transfers t
    LEFT JOIN transfer_receipts r ON r.transfer_id = t.id AND t.status = 'received'
    WHERE $3 IN ('all', 'transfer')
       OR ($3 = 'receipt' AND r.received_at IS NOT NULL
           AND ($1 = '' OR t.destination_warehouse_id = $1))
    UNION ALL
    SELECT 'operation:' || o.id, o.id, o.kind, o.created_at, NULL::timestamptz, NULL::jsonb
    FROM operations o
    LEFT JOIN receipt_links r ON r.id = o.id
    WHERE ($3 = 'all' OR o.kind = $3 OR ($3 = 'receipt' AND o.kind = 'receipt_reversal'))
      AND ($1 = '' OR EXISTS (
          SELECT 1 FROM selected s
          WHERE s.id = o.warehouse_id OR (o.warehouse_id IS NULL AND s.name = lower(o.warehouse))
      ))
      AND NOT EXISTS (SELECT 1 FROM transfers t WHERE t.id = r.transfer_id AND t.status = 'received')
)
SELECT key, id, kind, to_jsonb(occurred_at) #>> '{}' AS time,
       to_jsonb(preparation_received_at) #>> '{}' AS preparation_received_at,
       preparation_receipts
FROM combined
WHERE ($4::text IS NULL OR (occurred_at, key) < ($4::text::timestamptz, $5::text))
ORDER BY occurred_at DESC, key DESC
LIMIT $6
"#;

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
