use std::collections::BTreeMap;

use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;

use crate::core::calculate_orders::{
    hydrate_template_layers, validate_template, CalculateOrderError, CalculateOrderTemplate,
};
use crate::core::production_map::{compile_map, ProductionMapDefinition, ProductionMapSaved};

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
            "SELECT order_id, apparatus, barcode, payload_json
             FROM mini_training_raw_material_assignments
             ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        let order_id = order_id.trim();
        let apparatus = apparatus.trim();
        Ok(rows
            .into_iter()
            .filter(|row| {
                (order_id.is_empty() || row.order_id == order_id)
                    && (apparatus.is_empty() || row.apparatus.eq_ignore_ascii_case(apparatus))
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
        let barcode = payload_string(&payload, "barcode");
        if order_id.is_empty() || apparatus.is_empty() || barcode.is_empty() {
            return Err(TrainingWorkspaceError::InvalidInput(
                "order_id, apparatus va barcode kerak".to_string(),
            ));
        }

        let duplicate = sqlx::query_scalar::<_, String>(
            "SELECT id
             FROM mini_training_raw_material_assignments
             WHERE order_id = $1
               AND lower(apparatus) = lower($2)
               AND lower(barcode) = lower($3)
             LIMIT 1",
        )
        .bind(&order_id)
        .bind(&apparatus)
        .bind(&barcode)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        if duplicate.is_some() {
            return Err(TrainingWorkspaceError::DuplicateRawMaterialAssignment);
        }

        let id = format!("training-assignment-{}", unix_micros());
        sqlx::query(
            "INSERT INTO mini_training_raw_material_assignments
                (id, order_id, apparatus, barcode, payload_json, updated_at)
             VALUES ($1, $2, $3, $4, $5, now())",
        )
        .bind(id)
        .bind(order_id)
        .bind(apparatus)
        .bind(barcode)
        .bind(&payload)
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(payload)
    }

    pub async fn apparatus_modes(&self) -> Result<BTreeMap<String, bool>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<_, (String, bool)>(
            "SELECT apparatus, enabled
             FROM mini_training_apparatus_modes
             ORDER BY lower(apparatus)",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(rows.into_iter().collect())
    }

    pub async fn set_apparatus_mode(
        &self,
        apparatus: &str,
        enabled: bool,
    ) -> Result<(), TrainingWorkspaceError> {
        let apparatus = apparatus.trim();
        if apparatus.is_empty() {
            return Err(TrainingWorkspaceError::InvalidInput(
                "apparatus kerak".to_string(),
            ));
        }
        sqlx::query(
            "INSERT INTO mini_training_apparatus_modes (apparatus, enabled, updated_at)
             VALUES ($1, $2, now())
             ON CONFLICT (apparatus) DO UPDATE SET
                 enabled = excluded.enabled,
                 updated_at = now()",
        )
        .bind(apparatus)
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
            "SELECT apparatus, order_id, state
             FROM mini_training_queue_states
             ORDER BY updated_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;

        let mut states = BTreeMap::new();
        for (apparatus, order_id, state) in rows {
            let apparatus = apparatus.trim();
            let order_id = order_id.trim();
            if apparatus.is_empty() || order_id.is_empty() {
                continue;
            }
            states
                .entry(apparatus.to_string())
                .or_insert_with(BTreeMap::new)
                .insert(order_id.to_string(), state.trim().to_string());
        }
        Ok(states)
    }

    pub async fn put_queue_state(
        &self,
        apparatus: &str,
        order_id: &str,
        state: &str,
    ) -> Result<(), TrainingWorkspaceError> {
        let apparatus = apparatus.trim();
        let order_id = order_id.trim();
        let state = state.trim();
        if apparatus.is_empty() || order_id.is_empty() || state.is_empty() {
            return Err(TrainingWorkspaceError::InvalidInput(
                "apparatus, order_id va state kerak".to_string(),
            ));
        }
        sqlx::query(
            "INSERT INTO mini_training_queue_states (apparatus, order_id, state, updated_at)
             VALUES ($1, $2, $3, now())
             ON CONFLICT (apparatus, order_id) DO UPDATE SET
                 state = excluded.state,
                 updated_at = now()",
        )
        .bind(apparatus)
        .bind(order_id)
        .bind(state)
        .execute(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(())
    }

    pub async fn reset_queue_states(
        &self,
        apparatus: &str,
    ) -> Result<u64, TrainingWorkspaceError> {
        let apparatus = apparatus.trim();
        let result = (if apparatus.is_empty() {
            sqlx::query("DELETE FROM mini_training_queue_states")
                .execute(&self.pool)
                .await
        } else {
            sqlx::query(
                "DELETE FROM mini_training_queue_states
                 WHERE lower(apparatus) = lower($1)",
            )
            .bind(apparatus)
            .execute(&self.pool)
            .await
        })
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        Ok(result.rows_affected())
    }

    pub async fn raw_material_barcodes_for_order_apparatus(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<Vec<String>, TrainingWorkspaceError> {
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT barcode
             FROM mini_training_raw_material_assignments
             WHERE order_id = $1
               AND lower(apparatus) = lower($2)
             ORDER BY updated_at ASC",
        )
        .bind(order_id.trim())
        .bind(apparatus.trim())
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
    compile_map(map).map_err(|error| TrainingWorkspaceError::InvalidMap(error.to_string()))?;
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
    if map.code.trim().is_empty() && !map.order_number.trim().is_empty() {
        map.code = map.order_number.trim().to_string();
    }
    let program =
        compile_map(&map).map_err(|error| TrainingWorkspaceError::InvalidMap(error.to_string()))?;
    Ok(ProductionMapSaved { map, program })
}

fn payload_string(payload: &serde_json::Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
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
