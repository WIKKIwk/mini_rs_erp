use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Transaction};

use crate::core::auth::models::PrincipalRole;
use crate::core::returned_paint::{
    completion_report_message, normalize_returned_paint_stored_decimal,
    returned_paint_image_url, ReturnedPaintCalculation, ReturnedPaintError,
    ReturnedPaintImage, ReturnedPaintItem, ReturnedPaintRequest, ReturnedPaintStatus,
    ReturnedPaintStorePort, ReturnedPaintStoredImage,
};

#[derive(Clone)]
pub struct PostgresReturnedPaintStore {
    pool: PgPool,
}

impl PostgresReturnedPaintStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ReturnedPaintStorePort for PostgresReturnedPaintStore {
    async fn create(
        &self,
        request: ReturnedPaintRequest,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ReturnedPaintError::StoreFailed)?;
        let result = insert_returned_paint_request_tx(&mut tx, &request).await?;
        tx.commit()
            .await
            .map_err(|_| ReturnedPaintError::StoreFailed)?;
        Ok(result)
    }

    async fn list(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ReturnedPaintRequest>, ReturnedPaintError> {
        let rows = sqlx::query_as::<_, ReturnedPaintRow>(
            "SELECT request.id, request.order_id, request.order_code,
                request.order_name, request.apparatus, request.sender_role,
                request.sender_ref, request.sender_display_name,
                request.items_json, request.status,
                request.rasxot_mix_total::TEXT AS rasxot_mix_total,
                request.astatka_mix_total::TEXT AS astatka_mix_total,
                request.rasxot_alcohol::TEXT AS rasxot_alcohol,
                request.astatka_alcohol::TEXT AS astatka_alcohol,
                request.final_used_alcohol::TEXT AS final_used_alcohol,
                request.rasxot_pure_paint::TEXT AS rasxot_pure_paint,
                request.astatka_pure_paint::TEXT AS astatka_pure_paint,
                request.final_used_paint::TEXT AS final_used_paint,
                image.image_id, image.image_name, image.image_mime,
                image.image_size_bytes,
                EXTRACT(EPOCH FROM request.created_at)::BIGINT AS created_at_unix
             FROM mini_returned_paint_requests AS request
             LEFT JOIN mini_returned_paint_images AS image
                ON image.image_id = request.image_id
             WHERE request.target_role = 'boyoqchi'
             ORDER BY request.created_at DESC, request.id DESC
             LIMIT $1 OFFSET $2",
        )
        .bind(limit.max(1) as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ReturnedPaintError::StoreFailed)?;
        rows.into_iter().map(ReturnedPaintRow::into_model).collect()
    }

    async fn complete(
        &self,
        request_id: &str,
        items: Vec<ReturnedPaintItem>,
        calculation: ReturnedPaintCalculation,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        let items_json = serde_json::to_value(items)
            .map_err(|_| ReturnedPaintError::StoreFailed)?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ReturnedPaintError::StoreFailed)?;
        sqlx::query(
            "UPDATE mini_returned_paint_requests
             SET items_json = $2,
                status = 'completed',
                rasxot_mix_total = $3::NUMERIC,
                astatka_mix_total = $4::NUMERIC,
                rasxot_alcohol = $5::NUMERIC,
                astatka_alcohol = $6::NUMERIC,
                final_used_alcohol = $7::NUMERIC,
                rasxot_pure_paint = $8::NUMERIC,
                astatka_pure_paint = $9::NUMERIC,
                final_used_paint = $10::NUMERIC
             WHERE id = $1 AND status = 'waiting_for_boyoqchi_input'",
        )
        .bind(request_id)
        .bind(items_json)
        .bind(&calculation.rasxot_mix_total)
        .bind(&calculation.astatka_mix_total)
        .bind(&calculation.rasxot_alcohol)
        .bind(&calculation.astatka_alcohol)
        .bind(&calculation.final_used_alcohol)
        .bind(&calculation.rasxot_pure_paint)
        .bind(&calculation.astatka_pure_paint)
        .bind(&calculation.final_used_paint)
        .execute(&mut *tx)
        .await
        .map_err(|_| ReturnedPaintError::StoreFailed)?;
        let request = fetch_returned_paint_request_tx(&mut tx, request_id)
            .await?
            .ok_or(ReturnedPaintError::RequestNotFound)?;
        tx.commit()
            .await
            .map_err(|_| ReturnedPaintError::StoreFailed)?;
        Ok(request)
    }

    async fn save_image(
        &self,
        image: ReturnedPaintStoredImage,
    ) -> Result<ReturnedPaintStoredImage, ReturnedPaintError> {
        sqlx::query(
            "INSERT INTO mini_returned_paint_images (
                image_id, order_id, apparatus, owner_ref, image_name,
                image_mime, image_size_bytes, body
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&image.image.image_id)
        .bind(&image.order_id)
        .bind(&image.apparatus)
        .bind(&image.owner_ref)
        .bind(&image.image.image_name)
        .bind(&image.image.image_mime)
        .bind(image.image.image_size_bytes as i64)
        .bind(&image.body)
        .execute(&self.pool)
        .await
        .map_err(|_| ReturnedPaintError::StoreFailed)?;
        Ok(image)
    }

    async fn image(
        &self,
        image_id: &str,
    ) -> Result<Option<ReturnedPaintStoredImage>, ReturnedPaintError> {
        let row = sqlx::query_as::<_, ReturnedPaintImageRow>(
            "SELECT image_id, order_id, apparatus, owner_ref, image_name,
                image_mime, image_size_bytes, body
             FROM mini_returned_paint_images
             WHERE image_id = $1",
        )
        .bind(image_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| ReturnedPaintError::StoreFailed)?;
        row.map(ReturnedPaintImageRow::into_model).transpose()
    }

    async fn delete_image(
        &self,
        image_id: &str,
        owner_ref: &str,
    ) -> Result<bool, ReturnedPaintError> {
        let result = sqlx::query(
            "DELETE FROM mini_returned_paint_images AS image
             WHERE image.image_id = $1
               AND image.owner_ref = $2
               AND NOT EXISTS (
                    SELECT 1
                    FROM mini_returned_paint_requests AS request
                    WHERE request.image_id = image.image_id
               )",
        )
        .bind(image_id)
        .bind(owner_ref)
        .execute(&self.pool)
        .await
        .map_err(|_| ReturnedPaintError::StoreFailed)?;
        Ok(result.rows_affected() == 1)
    }
}

#[derive(sqlx::FromRow)]
struct ReturnedPaintRow {
    id: String,
    order_id: String,
    order_code: String,
    order_name: String,
    apparatus: String,
    sender_role: String,
    sender_ref: String,
    sender_display_name: String,
    items_json: serde_json::Value,
    status: String,
    rasxot_mix_total: Option<String>,
    astatka_mix_total: Option<String>,
    rasxot_alcohol: Option<String>,
    astatka_alcohol: Option<String>,
    final_used_alcohol: Option<String>,
    rasxot_pure_paint: Option<String>,
    astatka_pure_paint: Option<String>,
    final_used_paint: Option<String>,
    image_id: Option<String>,
    image_name: Option<String>,
    image_mime: Option<String>,
    image_size_bytes: Option<i64>,
    created_at_unix: i64,
}

impl ReturnedPaintRow {
    fn into_model(self) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        let calculation = calculation_from_columns([
            self.rasxot_mix_total,
            self.astatka_mix_total,
            self.rasxot_alcohol,
            self.astatka_alcohol,
            self.final_used_alcohol,
            self.rasxot_pure_paint,
            self.astatka_pure_paint,
            self.final_used_paint,
        ])?;
        let image = image_from_columns(
            self.image_id,
            self.image_name,
            self.image_mime,
            self.image_size_bytes,
        )?;
        let mut request = ReturnedPaintRequest {
            id: self.id,
            order_id: self.order_id,
            order_code: self.order_code,
            order_name: self.order_name,
            apparatus: self.apparatus,
            sender_role: parse_role(&self.sender_role)?,
            sender_ref: self.sender_ref,
            sender_display_name: self.sender_display_name,
            items: serde_json::from_value::<Vec<ReturnedPaintItem>>(self.items_json)
                .map_err(|_| ReturnedPaintError::StoreFailed)?,
            status: parse_status(&self.status)?,
            image,
            calculation,
            message: String::new(),
            created_at_unix: self.created_at_unix,
        };
        request.message = completion_report_message(&request);
        Ok(request)
    }
}

#[derive(sqlx::FromRow)]
struct ReturnedPaintImageRow {
    image_id: String,
    order_id: String,
    apparatus: String,
    owner_ref: String,
    image_name: String,
    image_mime: String,
    image_size_bytes: i64,
    body: Vec<u8>,
}

impl ReturnedPaintImageRow {
    fn into_model(self) -> Result<ReturnedPaintStoredImage, ReturnedPaintError> {
        let image_size_bytes = u64::try_from(self.image_size_bytes)
            .map_err(|_| ReturnedPaintError::StoreFailed)?;
        if image_size_bytes != self.body.len() as u64 {
            return Err(ReturnedPaintError::StoreFailed);
        }
        Ok(ReturnedPaintStoredImage {
            image: ReturnedPaintImage {
                image_url: returned_paint_image_url(&self.image_id),
                image_id: self.image_id,
                image_name: self.image_name,
                image_mime: self.image_mime,
                image_size_bytes,
            },
            order_id: self.order_id,
            apparatus: self.apparatus,
            owner_ref: self.owner_ref,
            body: self.body,
        })
    }
}

pub(crate) async fn insert_returned_paint_request_tx(
    tx: &mut Transaction<'_, Postgres>,
    request: &ReturnedPaintRequest,
) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
    let items_json = serde_json::to_value(&request.items)
        .map_err(|_| ReturnedPaintError::StoreFailed)?;
    let calculation = request.calculation.as_ref();
    let image_id = request.image.as_ref().map(|image| image.image_id.as_str());
    sqlx::query(
        "INSERT INTO mini_returned_paint_requests (
            id, target_role, order_id, order_code, order_name, apparatus,
            sender_role, sender_ref, sender_display_name, items_json, status,
            image_id,
            rasxot_mix_total, astatka_mix_total, rasxot_alcohol, astatka_alcohol,
            final_used_alcohol, rasxot_pure_paint, astatka_pure_paint,
            final_used_paint, created_at
         ) VALUES (
            $1, 'boyoqchi', $2, $3, $4, $5, $6, $7, $8, $9,
            $10, $11,
            $12::NUMERIC, $13::NUMERIC, $14::NUMERIC, $15::NUMERIC,
            $16::NUMERIC, $17::NUMERIC, $18::NUMERIC, $19::NUMERIC,
            to_timestamp($20)
         )
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(&request.id)
    .bind(&request.order_id)
    .bind(&request.order_code)
    .bind(&request.order_name)
    .bind(&request.apparatus)
    .bind(role_key(&request.sender_role))
    .bind(&request.sender_ref)
    .bind(&request.sender_display_name)
    .bind(items_json)
    .bind(status_key(request.status))
    .bind(image_id)
    .bind(calculation.map(|value| value.rasxot_mix_total.as_str()))
    .bind(calculation.map(|value| value.astatka_mix_total.as_str()))
    .bind(calculation.map(|value| value.rasxot_alcohol.as_str()))
    .bind(calculation.map(|value| value.astatka_alcohol.as_str()))
    .bind(calculation.map(|value| value.final_used_alcohol.as_str()))
    .bind(calculation.map(|value| value.rasxot_pure_paint.as_str()))
    .bind(calculation.map(|value| value.astatka_pure_paint.as_str()))
    .bind(calculation.map(|value| value.final_used_paint.as_str()))
    .bind(request.created_at_unix as f64)
    .execute(&mut **tx)
    .await
    .map_err(|_| ReturnedPaintError::StoreFailed)?;
    fetch_returned_paint_request_tx(tx, &request.id)
        .await?
        .ok_or(ReturnedPaintError::StoreFailed)
}

async fn fetch_returned_paint_request_tx(
    tx: &mut Transaction<'_, Postgres>,
    request_id: &str,
) -> Result<Option<ReturnedPaintRequest>, ReturnedPaintError> {
    let row = sqlx::query_as::<_, ReturnedPaintRow>(
        "SELECT request.id, request.order_id, request.order_code,
            request.order_name, request.apparatus, request.sender_role,
            request.sender_ref, request.sender_display_name,
            request.items_json, request.status,
            request.rasxot_mix_total::TEXT AS rasxot_mix_total,
            request.astatka_mix_total::TEXT AS astatka_mix_total,
            request.rasxot_alcohol::TEXT AS rasxot_alcohol,
            request.astatka_alcohol::TEXT AS astatka_alcohol,
            request.final_used_alcohol::TEXT AS final_used_alcohol,
            request.rasxot_pure_paint::TEXT AS rasxot_pure_paint,
            request.astatka_pure_paint::TEXT AS astatka_pure_paint,
            request.final_used_paint::TEXT AS final_used_paint,
            image.image_id, image.image_name, image.image_mime,
            image.image_size_bytes,
            EXTRACT(EPOCH FROM request.created_at)::BIGINT AS created_at_unix
         FROM mini_returned_paint_requests AS request
         LEFT JOIN mini_returned_paint_images AS image
            ON image.image_id = request.image_id
         WHERE request.id = $1",
    )
    .bind(request_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ReturnedPaintError::StoreFailed)?;
    row.map(ReturnedPaintRow::into_model).transpose()
}

fn calculation_from_columns(
    values: [Option<String>; 8],
) -> Result<Option<ReturnedPaintCalculation>, ReturnedPaintError> {
    let [
        rasxot_mix_total,
        astatka_mix_total,
        rasxot_alcohol,
        astatka_alcohol,
        final_used_alcohol,
        rasxot_pure_paint,
        astatka_pure_paint,
        final_used_paint,
    ] = values;
    match (
        rasxot_mix_total,
        astatka_mix_total,
        rasxot_alcohol,
        astatka_alcohol,
        final_used_alcohol,
        rasxot_pure_paint,
        astatka_pure_paint,
        final_used_paint,
    ) {
        (None, None, None, None, None, None, None, None) => Ok(None),
        (
            Some(rasxot_mix_total),
            Some(astatka_mix_total),
            Some(rasxot_alcohol),
            Some(astatka_alcohol),
            Some(final_used_alcohol),
            Some(rasxot_pure_paint),
            Some(astatka_pure_paint),
            Some(final_used_paint),
        ) => Ok(Some(ReturnedPaintCalculation {
            rasxot_mix_total: parse_decimal(&rasxot_mix_total)?,
            astatka_mix_total: parse_decimal(&astatka_mix_total)?,
            rasxot_alcohol: parse_decimal(&rasxot_alcohol)?,
            astatka_alcohol: parse_decimal(&astatka_alcohol)?,
            final_used_alcohol: parse_decimal(&final_used_alcohol)?,
            rasxot_pure_paint: parse_decimal(&rasxot_pure_paint)?,
            astatka_pure_paint: parse_decimal(&astatka_pure_paint)?,
            final_used_paint: parse_decimal(&final_used_paint)?,
        })),
        _ => Err(ReturnedPaintError::StoreFailed),
    }
}

fn image_from_columns(
    image_id: Option<String>,
    image_name: Option<String>,
    image_mime: Option<String>,
    image_size_bytes: Option<i64>,
) -> Result<Option<ReturnedPaintImage>, ReturnedPaintError> {
    match (image_id, image_name, image_mime, image_size_bytes) {
        (None, None, None, None) => Ok(None),
        (Some(image_id), Some(image_name), Some(image_mime), Some(image_size_bytes)) => {
            let image_size_bytes = u64::try_from(image_size_bytes)
                .map_err(|_| ReturnedPaintError::StoreFailed)?;
            Ok(Some(ReturnedPaintImage {
                image_url: returned_paint_image_url(&image_id),
                image_id,
                image_name,
                image_mime,
                image_size_bytes,
            }))
        }
        _ => Err(ReturnedPaintError::StoreFailed),
    }
}

fn parse_decimal(value: &str) -> Result<String, ReturnedPaintError> {
    normalize_returned_paint_stored_decimal(value)
        .map_err(|_| ReturnedPaintError::StoreFailed)
}

fn status_key(status: ReturnedPaintStatus) -> &'static str {
    match status {
        ReturnedPaintStatus::WaitingForBoyoqchiInput => "waiting_for_boyoqchi_input",
        ReturnedPaintStatus::Completed => "completed",
    }
}

fn parse_status(value: &str) -> Result<ReturnedPaintStatus, ReturnedPaintError> {
    match value.trim() {
        "waiting_for_boyoqchi_input" => Ok(ReturnedPaintStatus::WaitingForBoyoqchiInput),
        "completed" => Ok(ReturnedPaintStatus::Completed),
        _ => Err(ReturnedPaintError::StoreFailed),
    }
}

fn role_key(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::Boyoqchi => "boyoqchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    }
}

fn parse_role(value: &str) -> Result<PrincipalRole, ReturnedPaintError> {
    match value.trim() {
        "supplier" => Ok(PrincipalRole::Supplier),
        "werka" => Ok(PrincipalRole::Werka),
        "customer" => Ok(PrincipalRole::Customer),
        "aparatchi" => Ok(PrincipalRole::Aparatchi),
        "qolipchi" => Ok(PrincipalRole::Qolipchi),
        "boyoqchi" => Ok(PrincipalRole::Boyoqchi),
        "material_taminotchi" => Ok(PrincipalRole::MaterialTaminotchi),
        "admin" => Ok(PrincipalRole::Admin),
        _ => Err(ReturnedPaintError::StoreFailed),
    }
}
