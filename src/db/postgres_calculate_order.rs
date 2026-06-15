use std::collections::BTreeSet;

use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderImage, CalculateOrderStorePort, CalculateOrderTemplate,
    validate_template,
};

#[derive(Clone)]
pub struct PostgresCalculateOrderStore {
    pool: PgPool,
}

impl PostgresCalculateOrderStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CalculateOrderStorePort for PostgresCalculateOrderStore {
    async fn list(
        &self,
        owner_key: &str,
    ) -> Result<Vec<CalculateOrderTemplate>, CalculateOrderError> {
        let rows = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json
             FROM mini_quick_order_templates
             WHERE owner_key = $1
             ORDER BY saved_at DESC",
        )
        .bind(owner_key.trim())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| CalculateOrderError::StoreFailed)?;

        json_rows_to_templates(rows)
    }

    async fn list_all(&self) -> Result<Vec<CalculateOrderTemplate>, CalculateOrderError> {
        let rows = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json
             FROM mini_quick_order_templates
             ORDER BY saved_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| CalculateOrderError::StoreFailed)?;

        json_rows_to_templates(rows)
    }

    async fn upsert(
        &self,
        owner_key: &str,
        template: CalculateOrderTemplate,
    ) -> Result<CalculateOrderTemplate, CalculateOrderError> {
        validate_template(&template)?;
        let mut incoming = template;
        if incoming.code.trim().is_empty() {
            incoming.code = format!("Z-{}", new_id());
        }
        let existing = existing_id_by_code(&self.pool, owner_key, &incoming.code).await?;
        let saved = stamp_template(incoming, existing);
        let payload = serde_json::to_value(&saved).map_err(|_| CalculateOrderError::StoreFailed)?;

        sqlx::query(
            "INSERT INTO mini_quick_order_templates
                (id, owner_key, code, name, item_code, product_name, customer_ref,
                 customer_name, payload_json, quick_key, saved_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, now())
             ON CONFLICT (id) DO UPDATE SET
                code = excluded.code,
                name = excluded.name,
                item_code = excluded.item_code,
                product_name = excluded.product_name,
                customer_ref = excluded.customer_ref,
                customer_name = excluded.customer_name,
                payload_json = excluded.payload_json,
                quick_key = excluded.quick_key,
                saved_at = excluded.saved_at",
        )
        .bind(&saved.id)
        .bind(owner_key.trim())
        .bind(&saved.code)
        .bind(&saved.name)
        .bind(&saved.item_code)
        .bind(&saved.product)
        .bind(&saved.customer_ref)
        .bind(&saved.customer)
        .bind(payload)
        .bind(quick_template_key(&saved))
        .execute(&self.pool)
        .await
        .map_err(|_| CalculateOrderError::StoreFailed)?;

        Ok(saved)
    }

    async fn delete(&self, owner_key: &str, id: &str) -> Result<(), CalculateOrderError> {
        sqlx::query(
            "DELETE FROM mini_quick_order_templates
             WHERE owner_key = $1 AND id = $2",
        )
        .bind(owner_key.trim())
        .bind(id.trim())
        .execute(&self.pool)
        .await
        .map_err(|_| CalculateOrderError::StoreFailed)?;

        Ok(())
    }

    async fn save_image(
        &self,
        owner_key: &str,
        image: CalculateOrderImage,
    ) -> Result<CalculateOrderImage, CalculateOrderError> {
        let saved = stamp_image(image)?;
        sqlx::query(
            "INSERT INTO mini_quick_order_images
                (owner_key, image_id, image_name, image_mime, image_size_bytes, body, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())
             ON CONFLICT (owner_key, image_id) DO UPDATE SET
                image_name = excluded.image_name,
                image_mime = excluded.image_mime,
                image_size_bytes = excluded.image_size_bytes,
                body = excluded.body",
        )
        .bind(owner_key.trim())
        .bind(&saved.image_id)
        .bind(&saved.image_name)
        .bind(&saved.image_mime)
        .bind(saved.image_size_bytes as i64)
        .bind(&saved.body)
        .execute(&self.pool)
        .await
        .map_err(|_| CalculateOrderError::StoreFailed)?;

        Ok(saved)
    }

    async fn get_image(
        &self,
        owner_key: &str,
        image_id: &str,
    ) -> Result<Option<CalculateOrderImage>, CalculateOrderError> {
        let row = sqlx::query_as::<_, (String, String, String, i64, Vec<u8>)>(
            "SELECT image_id, image_name, image_mime, image_size_bytes, body
             FROM mini_quick_order_images
             WHERE owner_key = $1 AND image_id = $2",
        )
        .bind(owner_key.trim())
        .bind(image_id.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| CalculateOrderError::StoreFailed)?;

        Ok(row.map(
            |(image_id, image_name, image_mime, image_size_bytes, body)| CalculateOrderImage {
                image_id,
                image_name,
                image_mime,
                image_size_bytes: image_size_bytes.max(0) as u64,
                body,
            },
        ))
    }
}

async fn existing_id_by_code(
    pool: &PgPool,
    owner_key: &str,
    code: &str,
) -> Result<Option<String>, CalculateOrderError> {
    sqlx::query_scalar(
        "SELECT id
         FROM mini_quick_order_templates
         WHERE owner_key = $1 AND lower(code) = lower($2)
         ORDER BY saved_at DESC
         LIMIT 1",
    )
    .bind(owner_key.trim())
    .bind(code.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| CalculateOrderError::StoreFailed)
}

fn json_rows_to_templates(
    rows: Vec<serde_json::Value>,
) -> Result<Vec<CalculateOrderTemplate>, CalculateOrderError> {
    let templates = rows
        .into_iter()
        .map(|payload| {
            serde_json::from_value::<CalculateOrderTemplate>(payload)
                .map_err(|_| CalculateOrderError::StoreFailed)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(dedupe_templates(templates))
}

fn stamp_template(
    mut template: CalculateOrderTemplate,
    existing_id: Option<String>,
) -> CalculateOrderTemplate {
    template.id = existing_id
        .filter(|id| !id.trim().is_empty())
        .or_else(|| (!template.id.trim().is_empty()).then(|| template.id.trim().to_string()))
        .unwrap_or_else(new_id);
    template.code = template.code.trim().to_string();
    template.name = template.name.trim().to_string();
    template.order_number = template.order_number.trim().to_string();
    template.customer_ref = template.customer_ref.trim().to_string();
    template.customer = template.customer.trim().to_string();
    template.item_code = template.item_code.trim().to_string();
    template.product = template.product.trim().to_string();
    template.status = template.status.trim().to_string();
    template.material_display = template.material_display.trim().to_string();
    template.color = template.color.trim().to_string();
    template.image_id = template.image_id.trim().to_string();
    template.image_name = template.image_name.trim().to_string();
    template.image_mime = template.image_mime.trim().to_string();
    template.image_url = template.image_url.trim().to_string();
    template.first_layer_material = template.first_layer_material.trim().to_string();
    template.first_layer_micron = template.first_layer_micron.trim().to_string();
    template.second_layer_material = template.second_layer_material.trim().to_string();
    template.second_layer_micron = template.second_layer_micron.trim().to_string();
    template.third_layer_material = template.third_layer_material.trim().to_string();
    template.third_layer_micron = template.third_layer_micron.trim().to_string();
    template.note = template.note.trim().to_string();
    template.source_map_id = template.source_map_id.trim().to_string();
    template.saved_at = unix_micros().to_string();
    template
}

fn stamp_image(mut image: CalculateOrderImage) -> Result<CalculateOrderImage, CalculateOrderError> {
    image.image_id = image.image_id.trim().to_string();
    image.image_name = image.image_name.trim().to_string();
    image.image_mime = image.image_mime.trim().to_string();
    image.image_size_bytes = image.body.len() as u64;
    if image.image_id.is_empty() {
        return Err(CalculateOrderError::InvalidInput("id kerak".to_string()));
    }
    if image.image_name.is_empty() {
        image.image_name = "rang.jpg".to_string();
    }
    if image.image_mime.is_empty() {
        image.image_mime = "image/jpeg".to_string();
    }
    Ok(image)
}

fn dedupe_templates(templates: Vec<CalculateOrderTemplate>) -> Vec<CalculateOrderTemplate> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::with_capacity(templates.len());
    for template in templates {
        let key = quick_template_key(&template);
        if key == "id:" || seen.insert(key) {
            result.push(template);
        }
    }
    result
}

fn quick_template_key(template: &CalculateOrderTemplate) -> String {
    let product_key = [
        template.item_code.as_str(),
        template.product.as_str(),
        template.name.as_str(),
    ]
    .into_iter()
    .map(normalize_key)
    .find(|value| !value.is_empty())
    .unwrap_or_default();
    if product_key.is_empty() {
        return legacy_template_key(template);
    }
    [
        "quick".to_string(),
        normalize_key(&template.customer_ref),
        normalize_key(&template.customer),
        product_key,
        normalize_key(&template.status),
        normalize_key(&template.material_display),
        normalize_key(&template.color),
        number_key(template.width_mm),
        number_key(template.waste_percent),
        option_number_key(template.roll_count),
        normalize_key(&template.first_layer_material),
        normalize_key(&template.first_layer_micron),
        normalize_key(&template.second_layer_material),
        normalize_key(&template.second_layer_micron),
        normalize_key(&template.third_layer_material),
        normalize_key(&template.third_layer_micron),
        normalize_key(&template.note),
    ]
    .join("|")
}

fn legacy_template_key(template: &CalculateOrderTemplate) -> String {
    let code = normalize_key(&template.code);
    if code.is_empty() {
        format!("id:{}", template.id.trim())
    } else {
        format!("code:{code}")
    }
}

fn normalize_key(value: &str) -> String {
    value.trim().to_lowercase()
}

fn number_key(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.3}")
    } else {
        String::new()
    }
}

fn option_number_key(value: Option<f64>) -> String {
    value.map(number_key).unwrap_or_default()
}

fn new_id() -> String {
    unix_micros().to_string()
}

fn unix_micros() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default()
}
