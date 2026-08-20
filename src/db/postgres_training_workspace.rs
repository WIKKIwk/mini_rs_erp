use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;

use crate::core::apparatus_standard::ApparatusId;
use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderTemplate, hydrate_template_layers, validate_template,
};
use crate::core::production_map::{
    OrderProgressBatch, ProductionMapDefinition, ProductionMapNodeKind, ProductionMapProgram,
    ProductionMapSaved, compile_map,
};
use crate::core::production_map::{progress_batch_id, progress_qr_payload, queue_state};
use crate::core::returned_paint::{ReturnedPaintCalculation, ReturnedPaintItem};

#[path = "postgres_training_workspace_delete.rs"]
mod postgres_training_workspace_delete;

#[derive(Debug, Error)]
pub enum TrainingWorkspaceError {
    #[error("store failed")]
    StoreFailed,
    #[error("training map not found")]
    MapNotFound,
    #[error("training order number already exists")]
    DuplicateOrderNumber,
    #[error("training raw material assignment already exists")]
    DuplicateRawMaterialAssignment,
    #[error("invalid training input: {0}")]
    InvalidInput(String),
    #[error("invalid training map: {0}")]
    InvalidMap(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct TrainingProductionMapSaveWithOrder {
    pub saved: ProductionMapSaved,
    pub template: Option<CalculateOrderTemplate>,
}

#[derive(Debug, Clone)]
pub struct TrainingImage {
    pub image_id: String,
    pub image_name: String,
    pub image_mime: String,
    pub image_size_bytes: u64,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrainingQueueStateRecord {
    pub apparatus: String,
    pub order_id: String,
    pub state: String,
    pub updated_at_unix: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrainingQueueEvent {
    pub event_id: String,
    pub apparatus: String,
    pub order_id: String,
    pub action: String,
    pub from_state: String,
    pub to_state: String,
    pub actor_ref: String,
    pub actor_display_name: String,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainingInputBatchIdentity {
    pub order_id: String,
    pub apparatus: String,
    pub batch_id: String,
    pub session_id: String,
    pub qr_payload: String,
}

pub const TRAINING_VIRTUAL_INPUT_BOSMA: &str = "training-input:bosma";
pub const TRAINING_VIRTUAL_INPUT_LAMINATSIYA: &str = "training-input:laminatsiya";

#[derive(Clone)]
pub struct PostgresTrainingWorkspaceStore {
    pool: PgPool,
}

impl PostgresTrainingWorkspaceStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn maps(&self) -> Result<Vec<ProductionMapSaved>, TrainingWorkspaceError> {
        let rows = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT map_json
             FROM mini_training_production_maps
             ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        let mut maps = Vec::with_capacity(rows.len());
        for payload in rows {
            match saved_map_from_payload(payload) {
                Ok(saved) => maps.push(saved),
                Err(error) => {
                    tracing::warn!(%error, "skipping invalid training production map");
                }
            }
        }
        Ok(maps)
    }

    pub async fn map(
        &self,
        map_id: &str,
    ) -> Result<Option<ProductionMapSaved>, TrainingWorkspaceError> {
        let payload = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT map_json
             FROM mini_training_production_maps
             WHERE id = $1",
        )
        .bind(map_id.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        payload.map(saved_map_from_payload).transpose()
    }

    pub async fn template_for_order(
        &self,
        map_id: &str,
        order_number: &str,
    ) -> Result<Option<CalculateOrderTemplate>, TrainingWorkspaceError> {
        let payload = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json
             FROM mini_training_quick_order_templates
             WHERE payload_json->>'source_map_id' = $1
                OR ($2 <> '' AND payload_json->>'order_number' = $2)
             ORDER BY
                 CASE WHEN payload_json->>'source_map_id' = $1 THEN 0 ELSE 1 END,
                 saved_at DESC
             LIMIT 1",
        )
        .bind(map_id.trim())
        .bind(order_number.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        payload
            .map(|payload| {
                serde_json::from_value(payload).map_err(|_| TrainingWorkspaceError::StoreFailed)
            })
            .transpose()
    }

    pub async fn save_map(
        &self,
        map: ProductionMapDefinition,
    ) -> Result<ProductionMapSaved, TrainingWorkspaceError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let map = prepare_map_for_save(&mut tx, map).await?;
        save_map_tx(&mut tx, &map).await?;
        tx.commit()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        saved_map_from_definition(map)
    }

    pub async fn save_map_with_order(
        &self,
        map: ProductionMapDefinition,
        template: CalculateOrderTemplate,
        owner_key: &str,
    ) -> Result<TrainingProductionMapSaveWithOrder, TrainingWorkspaceError> {
        validate_template(&template).map_err(training_calculate_error)?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let mut map = prepare_map_for_save(&mut tx, map).await?;
        map.customer_name = template.customer.trim().to_string();
        save_map_tx(&mut tx, &map).await?;

        let template = prepare_template_for_save(template, &map);
        save_template_tx(&mut tx, &template, owner_key).await?;
        tx.commit()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        Ok(TrainingProductionMapSaveWithOrder {
            saved: saved_map_from_definition(map)?,
            template: Some(template),
        })
    }

    pub async fn raw_material_assignments(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<Vec<serde_json::Value>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<_, TrainingRawMaterialRow>(
            "SELECT order_id, canonical_apparatus_id AS apparatus, barcode, payload_json
             FROM mini_training_raw_material_assignments
             ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        let order_id = order_id.trim();
        let apparatus = if apparatus.trim().is_empty() {
            None
        } else {
            Some(canonical_training_apparatus(apparatus)?)
        };
        let rows = rows.into_iter().filter_map(|mut row| {
            row.apparatus = canonical_training_apparatus(&row.apparatus)
                .ok()?
                .to_string();
            Some(row)
        });
        Ok(rows
            .filter(|row| {
                (order_id.is_empty() || row.order_id == order_id)
                    && apparatus
                        .as_ref()
                        .is_none_or(|id| row.apparatus.eq_ignore_ascii_case(id.as_str()))
            })
            .map(|row| {
                let mut payload = match row.payload_json {
                    serde_json::Value::Object(object) => object,
                    _ => serde_json::Map::new(),
                };
                payload.insert("order_id".to_string(), serde_json::json!(row.order_id));
                payload.insert("apparatus".to_string(), serde_json::json!(row.apparatus));
                payload.insert("barcode".to_string(), serde_json::json!(row.barcode));
                serde_json::Value::Object(payload)
            })
            .collect())
    }

    pub async fn save_raw_material_assignment(
        &self,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, TrainingWorkspaceError> {
        let order_id = payload_string(&payload, "order_id");
        let apparatus = payload_string(&payload, "apparatus");
        let apparatus = canonical_training_apparatus(&apparatus)?;
        let barcode = payload_string(&payload, "barcode");
        if order_id.is_empty() || barcode.is_empty() {
            return Err(TrainingWorkspaceError::InvalidInput(
                "order_id, apparatus va barcode kerak".to_string(),
            ));
        }

        let duplicate = sqlx::query_scalar::<_, String>(
            "SELECT id
             FROM mini_training_raw_material_assignments
             WHERE order_id = $1
               AND canonical_apparatus_id = $2
               AND lower(barcode) = lower($3)
             LIMIT 1",
        )
        .bind(&order_id)
        .bind(apparatus.as_str())
        .bind(&barcode)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        if duplicate.is_some() {
            return Err(TrainingWorkspaceError::DuplicateRawMaterialAssignment);
        }

        let id = format!("training-assignment-{}", unix_micros());
        let mut normalized_payload = payload;
        if let serde_json::Value::Object(object) = &mut normalized_payload {
            object.insert(
                "apparatus".to_string(),
                serde_json::json!(apparatus.as_str()),
            );
        }
        sqlx::query(
            "INSERT INTO mini_training_raw_material_assignments
                (id, order_id, apparatus, canonical_apparatus_id, barcode, payload_json, updated_at)
             VALUES ($1, $2, COALESCE((SELECT name FROM mini_apparatus WHERE id = $3), $3), $3, $4, $5, now())",
        )
        .bind(id)
        .bind(order_id)
        .bind(apparatus.as_str())
        .bind(barcode)
        .bind(&normalized_payload)
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(normalized_payload)
    }

    pub async fn delete_raw_material_assignment(
        &self,
        order_id: &str,
        apparatus: &str,
        barcode: &str,
    ) -> Result<bool, TrainingWorkspaceError> {
        let order_id = order_id.trim();
        let apparatus = canonical_training_apparatus(apparatus)?;
        let barcode = barcode.trim();
        if order_id.is_empty() || !order_id.starts_with("training-") || barcode.is_empty() {
            return Err(TrainingWorkspaceError::InvalidInput(
                "order_id, apparatus va barcode kerak".to_string(),
            ));
        }

        let result = sqlx::query(
            "DELETE FROM mini_training_raw_material_assignments
             WHERE order_id = $1
               AND canonical_apparatus_id = $2
               AND lower(barcode) = lower($3)",
        )
        .bind(order_id)
        .bind(apparatus.as_str())
        .bind(barcode)
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn generate_training_input_batch(
        &self,
        order_id: &str,
        apparatus: &str,
        input_apparatus: &str,
    ) -> Result<TrainingInputBatchIdentity, TrainingWorkspaceError> {
        self.generate_training_input_batches(order_id, apparatus, input_apparatus, 1)
            .await?
            .into_iter()
            .next()
            .ok_or(TrainingWorkspaceError::StoreFailed)
    }

    pub async fn generate_training_input_batches(
        &self,
        order_id: &str,
        apparatus: &str,
        input_apparatus: &str,
        count: usize,
    ) -> Result<Vec<TrainingInputBatchIdentity>, TrainingWorkspaceError> {
        let order_id = order_id.trim();
        let apparatus = canonical_training_apparatus(apparatus)?;
        let input_apparatus = training_virtual_input_id(input_apparatus)?;
        if order_id.is_empty()
            || !order_id.starts_with("training-")
            || apparatus.as_str().is_empty()
            || input_apparatus.is_empty()
            || count == 0
        {
            return Err(TrainingWorkspaceError::InvalidInput(
                "training order, target va input apparati kerak".to_string(),
            ));
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let mut identities = Vec::with_capacity(count);
        for _ in 0..count {
            let batch_id = progress_batch_id(
                &input_apparatus,
                order_id,
                queue_state::ApparatusQueueAction::Complete,
                0,
            );
            let session_id = format!("training-input-session:{batch_id}");
            let qr_payload = progress_qr_payload(&batch_id);
            let row = sqlx::query_as::<_, (String, String, String, String, String)>(
                "INSERT INTO mini_training_input_batches
                    (order_id, apparatus, canonical_apparatus_id, batch_id, session_id, qr_payload, generated_at)
                 VALUES ($1, COALESCE((SELECT name FROM mini_apparatus WHERE id = $2), $2), $2, $3, $4, $5, now())
                 RETURNING order_id, canonical_apparatus_id AS apparatus, batch_id, session_id, qr_payload",
            )
            .bind(order_id)
            .bind(apparatus.as_str())
            .bind(batch_id)
            .bind(session_id)
            .bind(qr_payload)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
            identities.push(training_input_batch_identity_from_row(row)?);
        }
        tx.commit()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(identities)
    }

    pub async fn training_input_batch(
        &self,
        order_id: &str,
        apparatus: &str,
        input_apparatus: &str,
    ) -> Result<Option<TrainingInputBatchIdentity>, TrainingWorkspaceError> {
        let identities = self.training_input_batches(order_id, apparatus).await?;
        let apparatus = canonical_training_apparatus(apparatus)?;
        if identities.len() != 1 {
            return Ok(None);
        }
        let identity = identities
            .into_iter()
            .next()
            .ok_or(TrainingWorkspaceError::StoreFailed)?;
        if is_production_progress_qr(&identity.qr_payload) {
            return Ok(Some(identity));
        }
        sqlx::query(
            "DELETE FROM mini_training_input_batches
             WHERE order_id = $1 AND canonical_apparatus_id = $2",
        )
        .bind(order_id.trim())
        .bind(apparatus.as_str())
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        self.generate_training_input_batch(order_id, apparatus.as_str(), input_apparatus)
            .await
            .map(Some)
    }

    pub async fn training_input_batches(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<Vec<TrainingInputBatchIdentity>, TrainingWorkspaceError> {
        let apparatus = canonical_training_apparatus(apparatus)?;
        let rows = sqlx::query_as::<_, (String, String, String, String, String)>(
            "SELECT order_id, canonical_apparatus_id AS apparatus, batch_id, session_id, qr_payload
             FROM mini_training_input_batches
             WHERE order_id = $1 AND canonical_apparatus_id = $2
             ORDER BY generated_at ASC, batch_id ASC",
        )
        .bind(order_id.trim())
        .bind(apparatus.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        rows.into_iter()
            .map(training_input_batch_identity_from_row)
            .collect()
    }

    pub async fn training_input_batch_for_qr(
        &self,
        qr_payload: &str,
    ) -> Result<Option<TrainingInputBatchIdentity>, TrainingWorkspaceError> {
        let row = sqlx::query_as::<_, (String, String, String, String, String)>(
            "SELECT order_id, canonical_apparatus_id AS apparatus, batch_id, session_id, qr_payload
             FROM mini_training_input_batches
             WHERE lower(qr_payload) = lower($1)",
        )
        .bind(qr_payload.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        row.map(training_input_batch_identity_from_row).transpose()
    }

    pub async fn delete_training_input_batch(
        &self,
        order_id: &str,
        apparatus: &str,
        qr_payload: &str,
    ) -> Result<Vec<String>, TrainingWorkspaceError> {
        let order_id = order_id.trim();
        let apparatus = if apparatus.trim().is_empty() {
            None
        } else {
            Some(canonical_training_apparatus(apparatus)?)
        };
        let qr_payload = qr_payload.trim();
        if order_id.is_empty() || !order_id.starts_with("training-") {
            return Err(TrainingWorkspaceError::InvalidInput(
                "training order id kerak".to_string(),
            ));
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let deleted_batch_ids = sqlx::query_scalar::<_, String>(
            "DELETE FROM mini_training_input_batches
             WHERE order_id = $1
               AND ($2 = '' OR canonical_apparatus_id = $2)
               AND ($3 = '' OR lower(qr_payload) = lower($3))
             RETURNING batch_id",
        )
        .bind(order_id)
        .bind(
            apparatus
                .as_ref()
                .map(ApparatusId::as_str)
                .unwrap_or_default(),
        )
        .bind(qr_payload)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        for batch_id in &deleted_batch_ids {
            sqlx::query("DELETE FROM mini_training_progress_batches WHERE batch_id = $1")
                .bind(batch_id)
                .execute(&mut *tx)
                .await
                .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        }
        tx.commit()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(deleted_batch_ids)
    }

    pub async fn training_input_batch_generated(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<bool, TrainingWorkspaceError> {
        let apparatus = canonical_training_apparatus(apparatus)?;
        let generated = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                 SELECT 1
                 FROM mini_training_input_batches
                 WHERE order_id = $1 AND canonical_apparatus_id = $2
             )",
        )
        .bind(order_id.trim())
        .bind(apparatus.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(generated)
    }

    pub async fn training_input_batch_set_started(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<bool, TrainingWorkspaceError> {
        let apparatus = canonical_training_apparatus(apparatus)?;
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                 SELECT 1
                 FROM mini_training_queue_events
                 WHERE order_id = $1
                   AND canonical_apparatus_id = $2
                   AND action = 'start'
             )",
        )
        .bind(order_id.trim())
        .bind(apparatus.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)
    }

    pub async fn training_input_batch_orders(
        &self,
    ) -> Result<Vec<(String, String)>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT order_id, canonical_apparatus_id AS apparatus
             FROM mini_training_input_batches
             ORDER BY generated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(rows
            .into_iter()
            .filter_map(|(order_id, apparatus)| {
                canonical_training_apparatus(&apparatus)
                    .ok()
                    .map(|id| (order_id, id.to_string()))
            })
            .collect())
    }

    pub async fn put_training_progress_batches(
        &self,
        progress_batches: &[OrderProgressBatch],
    ) -> Result<(), TrainingWorkspaceError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        for batch in progress_batches {
            let apparatus = canonical_training_apparatus(&batch.apparatus)?;
            let payload = training_progress_payload(batch)?;
            sqlx::query(
                "INSERT INTO mini_training_progress_batches
                    (batch_id, order_id, apparatus, canonical_apparatus_id, qr_payload, payload_json, generated_at)
                 VALUES ($1, $2, COALESCE((SELECT name FROM mini_apparatus WHERE id = $3), $3), $3, $4, $5, now())
                 ON CONFLICT (batch_id) DO UPDATE SET
                     order_id = excluded.order_id,
                     apparatus = excluded.apparatus,
                     canonical_apparatus_id = excluded.canonical_apparatus_id,
                     qr_payload = excluded.qr_payload,
                     payload_json = excluded.payload_json,
                     generated_at = now()",
            )
            .bind(batch.batch_id.trim())
            .bind(batch.order_id.trim())
            .bind(apparatus.as_str())
            .bind(batch.qr_payload.trim())
            .bind(payload)
            .execute(&mut *tx)
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        }
        tx.commit()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(())
    }

    pub async fn training_progress_batch_for_key(
        &self,
        batch_id: &str,
        qr_payload: &str,
    ) -> Result<Option<OrderProgressBatch>, TrainingWorkspaceError> {
        let row = sqlx::query_as::<_, (String, serde_json::Value)>(
            "SELECT canonical_apparatus_id, payload_json
             FROM mini_training_progress_batches
             WHERE ($1 <> '' AND lower(batch_id) = lower($1))
                OR ($2 <> '' AND lower(qr_payload) = lower($2))
             LIMIT 1",
        )
        .bind(batch_id.trim())
        .bind(qr_payload.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        row.map(training_progress_batch_from_row).transpose()
    }

    pub async fn training_progress_batches_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<OrderProgressBatch>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
            "SELECT canonical_apparatus_id, payload_json
             FROM mini_training_progress_batches
             WHERE order_id = $1
             ORDER BY generated_at ASC, batch_id ASC",
        )
        .bind(order_id.trim())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        rows.into_iter()
            .map(training_progress_batch_from_row)
            .collect()
    }

    pub async fn apparatus_modes(&self) -> Result<BTreeMap<String, bool>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<_, (String, bool)>(
            "SELECT canonical_apparatus_id AS apparatus, enabled
             FROM mini_training_apparatus_modes
             ORDER BY canonical_apparatus_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(rows
            .into_iter()
            .filter_map(|(apparatus, enabled)| {
                canonical_training_apparatus(&apparatus)
                    .map(|id| (id.to_string(), enabled))
                    .map_err(|error| {
                        tracing::warn!(%error, apparatus, "skipping invalid training apparatus mode");
                        error
                    })
                    .ok()
            })
            .collect())
    }

    pub async fn set_apparatus_mode(
        &self,
        apparatus: &str,
        enabled: bool,
    ) -> Result<(), TrainingWorkspaceError> {
        let apparatus = canonical_training_apparatus(apparatus)?;
        sqlx::query(
            "INSERT INTO mini_training_apparatus_modes
                (apparatus, canonical_apparatus_id, enabled, updated_at)
             VALUES (COALESCE((SELECT name FROM mini_apparatus WHERE id = $1), $1), $1, $2, now())
             ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
                 apparatus = excluded.apparatus,
                 enabled = excluded.enabled,
                 updated_at = now()",
        )
        .bind(apparatus.as_str())
        .bind(enabled)
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(())
    }

    pub async fn queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT canonical_apparatus_id AS apparatus, order_id, state
             FROM mini_training_queue_states
             ORDER BY updated_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        let mut states = BTreeMap::new();
        for (apparatus, order_id, state) in rows {
            let Ok(apparatus) = canonical_training_apparatus(&apparatus) else {
                continue;
            };
            let order_id = order_id.trim();
            if apparatus.as_str().is_empty() || order_id.is_empty() {
                continue;
            }
            states
                .entry(apparatus.to_string())
                .or_insert_with(BTreeMap::new)
                .insert(order_id.to_string(), state.trim().to_string());
        }
        Ok(states)
    }

    pub async fn queue_state_records(
        &self,
    ) -> Result<Vec<TrainingQueueStateRecord>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<_, (String, String, String, i64)>(
            "SELECT canonical_apparatus_id AS apparatus, order_id, state,
                    EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at_unix
             FROM mini_training_queue_states
             ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        Ok(rows
            .into_iter()
            .filter_map(|(apparatus, order_id, state, updated_at_unix)| {
                let apparatus = canonical_training_apparatus(&apparatus).ok()?.to_string();
                let order_id = order_id.trim().to_string();
                let state = state.trim().to_string();
                (!apparatus.is_empty() && !order_id.is_empty() && !state.is_empty()).then_some(
                    TrainingQueueStateRecord {
                        apparatus,
                        order_id,
                        state,
                        updated_at_unix,
                    },
                )
            })
            .collect())
    }

    pub async fn put_queue_state(
        &self,
        apparatus: &str,
        order_id: &str,
        state: &str,
    ) -> Result<(), TrainingWorkspaceError> {
        let apparatus = canonical_training_apparatus(apparatus)?;
        let order_id = order_id.trim();
        let state = state.trim();
        if apparatus.as_str().is_empty() || order_id.is_empty() || state.is_empty() {
            return Err(TrainingWorkspaceError::InvalidInput(
                "apparatus, order_id va state kerak".to_string(),
            ));
        }
        sqlx::query(
            "INSERT INTO mini_training_queue_states
                (apparatus, canonical_apparatus_id, order_id, state, updated_at)
             VALUES (COALESCE((SELECT name FROM mini_apparatus WHERE id = $1), $1), $1, $2, $3, now())
             ON CONFLICT (canonical_apparatus_id, order_id) DO UPDATE SET
                 apparatus = excluded.apparatus,
                 state = excluded.state,
                 updated_at = now()",
        )
        .bind(apparatus.as_str())
        .bind(order_id)
        .bind(state)
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn put_queue_state_with_event(
        &self,
        apparatus: &str,
        order_id: &str,
        state: &str,
        event_id: &str,
        action: &str,
        from_state: &str,
        actor_ref: &str,
        actor_display_name: &str,
        progress_batches: &[OrderProgressBatch],
    ) -> Result<(), TrainingWorkspaceError> {
        let apparatus = canonical_training_apparatus(apparatus)?;
        let order_id = order_id.trim();
        let state = state.trim();
        let event_id = event_id.trim();
        let action = action.trim();
        let from_state = from_state.trim();
        if apparatus.as_str().is_empty()
            || order_id.is_empty()
            || state.is_empty()
            || event_id.is_empty()
            || action.is_empty()
            || from_state.is_empty()
        {
            return Err(TrainingWorkspaceError::InvalidInput(
                "training queue event uchun aparat, order, state va amal kerak".to_string(),
            ));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        sqlx::query(
            "INSERT INTO mini_training_queue_states
                (apparatus, canonical_apparatus_id, order_id, state, updated_at)
             VALUES (COALESCE((SELECT name FROM mini_apparatus WHERE id = $1), $1), $1, $2, $3, now())
             ON CONFLICT (canonical_apparatus_id, order_id) DO UPDATE SET
                 apparatus = excluded.apparatus,
                 state = excluded.state,
                 updated_at = now()",
        )
        .bind(apparatus.as_str())
        .bind(order_id)
        .bind(state)
        .execute(&mut *tx)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        sqlx::query(
            "INSERT INTO mini_training_queue_events
                (event_id, apparatus, canonical_apparatus_id, order_id, action, from_state, to_state,
                 actor_ref, actor_display_name, created_at)
             VALUES ($1, COALESCE((SELECT name FROM mini_apparatus WHERE id = $2), $2), $2,
                     $3, $4, $5, $6, $7, $8, now())",
        )
        .bind(event_id)
        .bind(apparatus.as_str())
        .bind(order_id)
        .bind(action)
        .bind(from_state)
        .bind(state)
        .bind(actor_ref.trim())
        .bind(actor_display_name.trim())
        .execute(&mut *tx)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        for batch in progress_batches {
            let apparatus = canonical_training_apparatus(&batch.apparatus)?;
            let payload = training_progress_payload(batch)?;
            sqlx::query(
                "INSERT INTO mini_training_progress_batches
                    (batch_id, order_id, apparatus, canonical_apparatus_id, qr_payload, payload_json, generated_at)
                 VALUES ($1, $2, COALESCE((SELECT name FROM mini_apparatus WHERE id = $3), $3), $3, $4, $5, now())
                 ON CONFLICT (batch_id) DO UPDATE SET
                     order_id = excluded.order_id,
                     apparatus = excluded.apparatus,
                     canonical_apparatus_id = excluded.canonical_apparatus_id,
                     qr_payload = excluded.qr_payload,
                     payload_json = excluded.payload_json,
                     generated_at = now()",
            )
            .bind(batch.batch_id.trim())
            .bind(batch.order_id.trim())
            .bind(apparatus.as_str())
            .bind(batch.qr_payload.trim())
            .bind(payload)
            .execute(&mut *tx)
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        }
        tx.commit()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(())
    }

    pub async fn latest_queue_events(
        &self,
    ) -> Result<Vec<TrainingQueueEvent>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                i64,
            ),
        >(
            "SELECT event_id, canonical_apparatus_id AS apparatus, order_id, action, from_state, to_state,
                    actor_ref, actor_display_name,
                    EXTRACT(EPOCH FROM created_at)::bigint AS created_at_unix
             FROM (
                SELECT DISTINCT ON (canonical_apparatus_id, order_id)
                    event_id, canonical_apparatus_id, order_id, action, from_state, to_state,
                    actor_ref, actor_display_name, created_at
                FROM mini_training_queue_events
                ORDER BY canonical_apparatus_id, order_id, created_at DESC, event_id DESC
             ) latest
             ORDER BY created_at DESC, event_id DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        rows.into_iter()
            .map(training_queue_event_from_row)
            .collect()
    }

    pub async fn completed_queue_events_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<TrainingQueueEvent>, TrainingWorkspaceError> {
        let actor_ref = actor_ref.trim();
        if actor_ref.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit.min(500)).unwrap_or(500);
        let rows = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                i64,
            ),
        >(
            "SELECT event_id, canonical_apparatus_id AS apparatus, order_id, action, from_state, to_state,
                    actor_ref, actor_display_name,
                    EXTRACT(EPOCH FROM created_at)::bigint AS created_at_unix
             FROM (
                SELECT DISTINCT ON (order_id, canonical_apparatus_id)
                    event_id, canonical_apparatus_id, order_id, action, from_state, to_state,
                    actor_ref, actor_display_name, created_at
                FROM mini_training_queue_events
                WHERE actor_ref = $1
                  AND action IN ('pause', 'detach_roll', 'roll_complete', 'complete')
                ORDER BY order_id, canonical_apparatus_id, created_at DESC, event_id DESC
             ) latest
             ORDER BY created_at DESC, event_id DESC
             LIMIT $2",
        )
        .bind(actor_ref)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        rows.into_iter()
            .map(training_queue_event_from_row)
            .collect()
    }

    pub async fn reset_queue_states(&self, apparatus: &str) -> Result<u64, TrainingWorkspaceError> {
        let apparatus = if apparatus.trim().is_empty() {
            None
        } else {
            Some(canonical_training_apparatus(apparatus)?)
        };
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let result = (if apparatus.is_none() {
            sqlx::query("DELETE FROM mini_training_queue_states")
                .execute(&mut *tx)
                .await
        } else {
            sqlx::query(
                "DELETE FROM mini_training_queue_states
                 WHERE canonical_apparatus_id = $1",
            )
            .bind(
                apparatus
                    .as_ref()
                    .map(ApparatusId::as_str)
                    .unwrap_or_default(),
            )
            .execute(&mut *tx)
            .await
        })
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        if apparatus.is_none() {
            sqlx::query("DELETE FROM mini_training_queue_events")
                .execute(&mut *tx)
                .await
                .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        } else {
            sqlx::query(
                "DELETE FROM mini_training_queue_events
                 WHERE canonical_apparatus_id = $1",
            )
            .bind(
                apparatus
                    .as_ref()
                    .map(ApparatusId::as_str)
                    .unwrap_or_default(),
            )
            .execute(&mut *tx)
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        }
        tx.commit()
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(result.rows_affected())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn save_returned_paint_report(
        &self,
        order_id: &str,
        apparatus: &str,
        action: &str,
        items: &[ReturnedPaintItem],
        image_id: &str,
        return_ink_kg: Option<f64>,
        calculation: Option<&ReturnedPaintCalculation>,
    ) -> Result<serde_json::Value, TrainingWorkspaceError> {
        let order_id = order_id.trim();
        let apparatus = canonical_training_apparatus(apparatus)?;
        let action = action.trim();
        let image_id = image_id.trim();
        if order_id.is_empty() || action.is_empty() {
            return Err(TrainingWorkspaceError::InvalidInput(
                "training qaytarilgan bo‘yoq hisoboti uchun order, aparat va amal kerak"
                    .to_string(),
            ));
        }
        let id = format!("training-returned-paint-{}", unix_micros());
        let created_at_unix = (unix_micros() / 1_000_000) as i64;
        let items_json =
            serde_json::to_value(items).map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let calculation_json = calculation
            .map(serde_json::to_value)
            .transpose()
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let return_ink_kg = return_ink_kg.filter(|value| value.is_finite() && *value >= 0.0);

        sqlx::query(
            "INSERT INTO mini_training_returned_paint_reports
                (id, order_id, apparatus, canonical_apparatus_id, action, items_json, image_id,
                 return_ink_kg, calculation_json, created_at)
             VALUES ($1, $2, $3, $3, $4, $5, $6, $7, $8, to_timestamp($9))",
        )
        .bind(&id)
        .bind(order_id)
        .bind(apparatus.as_str())
        .bind(action)
        .bind(&items_json)
        .bind(image_id)
        .bind(return_ink_kg)
        .bind(calculation_json.clone())
        .bind(created_at_unix as f64)
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        Ok(serde_json::json!({
            "id": id,
            "order_id": order_id,
            "apparatus": apparatus,
            "action": action,
            "items": items_json,
            "image_id": image_id,
            "return_ink_kg": return_ink_kg,
            "calculation": calculation_json,
            "created_at_unix": created_at_unix,
        }))
    }

    pub async fn raw_material_barcodes_for_order_apparatus(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<Vec<String>, TrainingWorkspaceError> {
        let apparatus = canonical_training_apparatus(apparatus)?;
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT barcode
             FROM mini_training_raw_material_assignments
             WHERE order_id = $1
               AND canonical_apparatus_id = $2
             ORDER BY updated_at ASC",
        )
        .bind(order_id.trim())
        .bind(apparatus.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(rows
            .into_iter()
            .map(|(barcode,)| barcode.trim().to_string())
            .filter(|barcode| !barcode.is_empty())
            .collect())
    }

    pub async fn save_image(
        &self,
        owner_key: &str,
        image: TrainingImage,
    ) -> Result<TrainingImage, TrainingWorkspaceError> {
        sqlx::query(
            "INSERT INTO mini_training_order_images
                (owner_key, image_id, image_name, image_mime, image_size_bytes, body, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())
             ON CONFLICT (owner_key, image_id) DO UPDATE SET
                 image_name = excluded.image_name,
                 image_mime = excluded.image_mime,
                 image_size_bytes = excluded.image_size_bytes,
                 body = excluded.body,
                 created_at = now()",
        )
        .bind(owner_key.trim())
        .bind(&image.image_id)
        .bind(&image.image_name)
        .bind(&image.image_mime)
        .bind(image.image_size_bytes as i64)
        .bind(&image.body)
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(image)
    }

    pub async fn image(
        &self,
        owner_key: &str,
        image_id: &str,
    ) -> Result<Option<TrainingImage>, TrainingWorkspaceError> {
        let row = sqlx::query_as::<_, TrainingImageRow>(
            "SELECT image_id, image_name, image_mime, image_size_bytes, body
             FROM mini_training_order_images
             WHERE owner_key = $1 AND image_id = $2",
        )
        .bind(owner_key.trim())
        .bind(image_id.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        Ok(row.map(|row| TrainingImage {
            image_id: row.image_id,
            image_name: row.image_name,
            image_mime: row.image_mime,
            image_size_bytes: row.image_size_bytes.max(0) as u64,
            body: row.body,
        }))
    }

    pub async fn delete_image(
        &self,
        owner_key: &str,
        image_id: &str,
    ) -> Result<bool, TrainingWorkspaceError> {
        let deleted = sqlx::query_scalar::<_, String>(
            "DELETE FROM mini_training_order_images
             WHERE owner_key = $1 AND image_id = $2
             RETURNING image_id",
        )
        .bind(owner_key.trim())
        .bind(image_id.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(deleted.is_some())
    }
}

#[derive(sqlx::FromRow)]
struct TrainingRawMaterialRow {
    order_id: String,
    apparatus: String,
    #[allow(dead_code)]
    barcode: String,
    payload_json: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct TrainingImageRow {
    image_id: String,
    image_name: String,
    image_mime: String,
    image_size_bytes: i64,
    body: Vec<u8>,
}

async fn prepare_map_for_save(
    tx: &mut Transaction<'_, Postgres>,
    mut map: ProductionMapDefinition,
) -> Result<ProductionMapDefinition, TrainingWorkspaceError> {
    if map.order_number.trim().is_empty() {
        let next = sqlx::query_scalar::<_, i64>("SELECT nextval('mini_training_order_number_seq')")
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let order_number = format!("T-{next:04}");
        map.order_number = order_number.clone();
        if map.code.trim().is_empty() {
            map.code = order_number.clone();
        }
        if map.id.trim().is_empty() || map.id.starts_with("zakaz-draft-") {
            map.id = format!("training-zakaz-{next:04}");
        }
    }
    if map.id.trim().is_empty() {
        map.id = format!("training-zakaz-{}", unix_micros());
    }
    if map.code.trim().is_empty() {
        map.code = map.order_number.trim().to_string();
    }
    normalize_training_map_apparatus_ids(&mut map)?;

    let duplicate = sqlx::query_scalar::<_, String>(
        "SELECT id
         FROM mini_training_production_maps
         WHERE order_number = $1 AND id <> $2
         LIMIT 1",
    )
    .bind(map.order_number.trim())
    .bind(map.id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    if duplicate.is_some() {
        return Err(TrainingWorkspaceError::DuplicateOrderNumber);
    }
    Ok(map)
}

async fn save_map_tx(
    tx: &mut Transaction<'_, Postgres>,
    map: &ProductionMapDefinition,
) -> Result<(), TrainingWorkspaceError> {
    let payload = serde_json::to_value(map).map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    let program =
        compile_map(map).map_err(|error| TrainingWorkspaceError::InvalidMap(error.to_string()))?;
    validate_training_apparatus_ids(tx, &program).await?;
    sqlx::query(
        "INSERT INTO mini_training_production_maps
            (id, order_number, map_json, updated_at)
         VALUES ($1, $2, $3, now())
         ON CONFLICT (id) DO UPDATE SET
             order_number = excluded.order_number,
             map_json = excluded.map_json,
             updated_at = now()",
    )
    .bind(map.id.trim())
    .bind(map.order_number.trim())
    .bind(payload)
    .execute(&mut **tx)
    .await
    .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    Ok(())
}

async fn validate_training_apparatus_ids(
    tx: &mut Transaction<'_, Postgres>,
    program: &ProductionMapProgram,
) -> Result<(), TrainingWorkspaceError> {
    let mut requested = BTreeSet::new();
    for operation in program
        .operations
        .iter()
        .filter(|operation| operation.op_code == "apparatus")
    {
        for key in ["apparatus_id", "alternative_assigned_apparatus_id"] {
            let Some(value) = operation.args.get(key) else {
                continue;
            };
            if value.is_empty() {
                continue;
            }
            let id = ApparatusId::new(value.clone()).map_err(|_| {
                TrainingWorkspaceError::InvalidMap(format!(
                    "{key} must be an exact canonical apparatus id"
                ))
            })?;
            if id.as_str() != value {
                return Err(TrainingWorkspaceError::InvalidMap(format!(
                    "{key} must be an exact canonical apparatus id"
                )));
            }
            requested.insert(id.to_string());
        }
    }
    if requested.is_empty() {
        return Ok(());
    }

    let requested_ids = requested.iter().cloned().collect::<Vec<_>>();
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT id
         FROM mini_apparatus
         WHERE id = ANY($1)
         FOR KEY SHARE",
    )
    .bind(&requested_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| TrainingWorkspaceError::StoreFailed)?
    .into_iter()
    .collect::<BTreeSet<_>>();

    if existing != requested {
        let missing = requested.difference(&existing).cloned().collect::<Vec<_>>();
        return Err(TrainingWorkspaceError::InvalidMap(format!(
            "apparatus id(s) missing from mini_apparatus: {}",
            missing.join(", ")
        )));
    }
    Ok(())
}

fn prepare_template_for_save(
    mut template: CalculateOrderTemplate,
    map: &ProductionMapDefinition,
) -> CalculateOrderTemplate {
    template = hydrate_template_layers(template);
    if template.id.trim().is_empty() || !template.id.starts_with("training-") {
        template.id = format!("training-template-{}", unix_micros());
    }
    if template.code.trim().is_empty() {
        template.code = format!("TR-{}", map.order_number.trim());
    }
    template.order_number = map.order_number.trim().to_string();
    if template.source_map_id.trim().is_empty() {
        template.source_map_id = map.id.trim().to_string();
    }
    template.saved_at = unix_micros().to_string();
    template
}

async fn save_template_tx(
    tx: &mut Transaction<'_, Postgres>,
    template: &CalculateOrderTemplate,
    owner_key: &str,
) -> Result<(), TrainingWorkspaceError> {
    let payload =
        serde_json::to_value(template).map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO mini_training_quick_order_templates
            (id, owner_key, code, payload_json, saved_at)
         VALUES ($1, $2, $3, $4, now())
         ON CONFLICT (id) DO UPDATE SET
             owner_key = excluded.owner_key,
             code = excluded.code,
             payload_json = excluded.payload_json,
             saved_at = now()",
    )
    .bind(template.id.trim())
    .bind(owner_key.trim())
    .bind(template.code.trim())
    .bind(payload)
    .execute(&mut **tx)
    .await
    .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    Ok(())
}

fn saved_map_from_payload(
    payload: serde_json::Value,
) -> Result<ProductionMapSaved, TrainingWorkspaceError> {
    let map = serde_json::from_value::<ProductionMapDefinition>(payload)
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    saved_map_from_definition(map)
}

fn saved_map_from_definition(
    mut map: ProductionMapDefinition,
) -> Result<ProductionMapSaved, TrainingWorkspaceError> {
    normalize_training_map_apparatus_ids(&mut map)?;
    if map.code.trim().is_empty() && !map.order_number.trim().is_empty() {
        map.code = map.order_number.trim().to_string();
    }
    let program =
        compile_map(&map).map_err(|error| TrainingWorkspaceError::InvalidMap(error.to_string()))?;
    Ok(ProductionMapSaved { map, program })
}

fn normalize_training_map_apparatus_ids(
    map: &mut ProductionMapDefinition,
) -> Result<(), TrainingWorkspaceError> {
    for node in &mut map.nodes {
        if node.kind != ProductionMapNodeKind::Apparatus {
            continue;
        }

        node.apparatus_id = normalize_training_map_apparatus_id(
            &node.apparatus_id,
            "apparatus_id",
        )?;
        node.alternative_assigned_apparatus_id = normalize_training_map_apparatus_id(
            &node.alternative_assigned_apparatus_id,
            "alternative_assigned_apparatus_id",
        )?;
    }
    Ok(())
}

fn normalize_training_map_apparatus_id(
    value: &str,
    field: &str,
) -> Result<String, TrainingWorkspaceError> {
    if value.trim().is_empty() {
        return Ok(String::new());
    }
    canonical_training_apparatus(value)
        .map(|id| id.to_string())
        .map_err(|_| {
            TrainingWorkspaceError::InvalidMap(format!(
                "{field} must be an exact canonical apparatus id"
            ))
        })
}

fn payload_string(payload: &serde_json::Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn canonical_training_apparatus(value: &str) -> Result<ApparatusId, TrainingWorkspaceError> {
    ApparatusId::new(value.trim().to_string()).map_err(|_| {
        TrainingWorkspaceError::InvalidInput("canonical apparatus id kerak".to_string())
    })
}

fn training_virtual_input_id(value: &str) -> Result<String, TrainingWorkspaceError> {
    match value.trim().to_ascii_lowercase().as_str() {
        TRAINING_VIRTUAL_INPUT_BOSMA => Ok(TRAINING_VIRTUAL_INPUT_BOSMA.to_string()),
        TRAINING_VIRTUAL_INPUT_LAMINATSIYA => Ok(TRAINING_VIRTUAL_INPUT_LAMINATSIYA.to_string()),
        _ => Err(TrainingWorkspaceError::InvalidInput(
            "training virtual input kerak".to_string(),
        )),
    }
}

fn training_queue_event_from_row(
    (
        event_id,
        apparatus,
        order_id,
        action,
        from_state,
        to_state,
        actor_ref,
        actor_display_name,
        created_at_unix,
    ): (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
    ),
) -> Result<TrainingQueueEvent, TrainingWorkspaceError> {
    Ok(TrainingQueueEvent {
        event_id,
        apparatus: canonical_training_apparatus(&apparatus)?.to_string(),
        order_id,
        action,
        from_state,
        to_state,
        actor_ref,
        actor_display_name,
        created_at_unix,
    })
}

fn training_input_batch_identity_from_row(
    (order_id, apparatus, batch_id, session_id, qr_payload): (
        String,
        String,
        String,
        String,
        String,
    ),
) -> Result<TrainingInputBatchIdentity, TrainingWorkspaceError> {
    Ok(TrainingInputBatchIdentity {
        order_id,
        apparatus: canonical_training_apparatus(&apparatus)?.to_string(),
        batch_id,
        session_id,
        qr_payload,
    })
}

fn training_progress_payload(
    batch: &OrderProgressBatch,
) -> Result<serde_json::Value, TrainingWorkspaceError> {
    let apparatus = canonical_training_apparatus(&batch.apparatus)?;
    let mut payload =
        serde_json::to_value(batch).map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    let object = payload
        .as_object_mut()
        .ok_or(TrainingWorkspaceError::StoreFailed)?;
    object.insert(
        "apparatus".to_string(),
        serde_json::Value::String(apparatus.to_string()),
    );
    for (field, value) in [
        ("current_apparatus_key", batch.current_apparatus_key.as_str()),
        ("current_apparatus", batch.current_apparatus.as_str()),
        ("next_apparatus", batch.next_apparatus.as_str()),
        ("used_by_apparatus", batch.used_by_apparatus.as_str()),
        (
            "processed_by_apparatus",
            batch.processed_by_apparatus.as_str(),
        ),
    ] {
        let value = if value.trim().is_empty() {
            String::new()
        } else {
            canonical_training_apparatus(value)?.to_string()
        };
        object.insert(field.to_string(), serde_json::Value::String(value));
    }
    Ok(payload)
}

fn training_progress_batch_from_row(
    (canonical_apparatus_id, mut payload): (String, serde_json::Value),
) -> Result<OrderProgressBatch, TrainingWorkspaceError> {
    let canonical_apparatus_id = canonical_training_apparatus(&canonical_apparatus_id)?;
    let object = payload
        .as_object_mut()
        .ok_or(TrainingWorkspaceError::StoreFailed)?;
    object.insert(
        "apparatus".to_string(),
        serde_json::Value::String(canonical_apparatus_id.to_string()),
    );
    training_progress_batch_from_payload(payload)
}

fn training_progress_batch_from_payload(
    payload: serde_json::Value,
) -> Result<OrderProgressBatch, TrainingWorkspaceError> {
    let batch = serde_json::from_value::<OrderProgressBatch>(payload).map_err(|error| {
        tracing::warn!(%error, "invalid persisted training progress batch");
        TrainingWorkspaceError::StoreFailed
    })?;
    for apparatus in [
        batch.apparatus.as_str(),
        batch.current_apparatus_key.as_str(),
        batch.current_apparatus.as_str(),
        batch.next_apparatus.as_str(),
        batch.used_by_apparatus.as_str(),
        batch.processed_by_apparatus.as_str(),
    ] {
        if !apparatus.trim().is_empty() && canonical_training_apparatus(apparatus).is_err() {
            return Err(TrainingWorkspaceError::StoreFailed);
        }
    }
    Ok(batch)
}

fn is_production_progress_qr(value: &str) -> bool {
    let value = value.trim().as_bytes();
    value.len() == 24
        && value[..4].eq_ignore_ascii_case(b"4001")
        && value[4..].iter().all(u8::is_ascii_hexdigit)
}

fn training_calculate_error(error: CalculateOrderError) -> TrainingWorkspaceError {
    match error {
        CalculateOrderError::InvalidInput(detail) => TrainingWorkspaceError::InvalidInput(detail),
        CalculateOrderError::StoreFailed => TrainingWorkspaceError::StoreFailed,
    }
}

fn unix_micros() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        TRAINING_VIRTUAL_INPUT_BOSMA, canonical_training_apparatus, is_production_progress_qr,
        training_virtual_input_id,
    };

    #[test]
    fn recognizes_only_production_progress_qr_payloads() {
        assert!(is_production_progress_qr("400118904D9F447100000F96"));
        assert!(is_production_progress_qr("400118904d9f447100000f96"));
        assert!(!is_production_progress_qr(
            "TRAINING-INPUT:training-zakaz-0005"
        ));
        assert!(!is_production_progress_qr("4001🚫000000000000000"));
    }

    #[test]
    fn training_store_accepts_canonical_ids_but_not_renamed_titles() {
        let id = canonical_training_apparatus("apparatus:training:lam-001")
            .expect("canonical training apparatus");
        assert_eq!(id.as_str(), "apparatus:training:lam-001");
        assert!(canonical_training_apparatus("Renamed laminatsiya").is_err());
    }

    #[test]
    fn virtual_training_input_is_not_canonical_or_production_fallback() {
        assert!(canonical_training_apparatus(TRAINING_VIRTUAL_INPUT_BOSMA).is_err());
        assert_eq!(
            training_virtual_input_id(TRAINING_VIRTUAL_INPUT_BOSMA).expect("virtual input"),
            TRAINING_VIRTUAL_INPUT_BOSMA
        );
        assert!(!is_production_progress_qr("TRAINING-INPUT:training-1001"));
    }
}
