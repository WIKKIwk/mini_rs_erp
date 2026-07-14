use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Transaction};

use crate::core::auth::models::PrincipalRole;
use crate::core::returned_paint::{
    completion_report_message, normalize_returned_paint_stored_decimal,
    ReturnedPaintCalculation, ReturnedPaintError, ReturnedPaintItem, ReturnedPaintRequest,
    ReturnedPaintStorePort,
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
            "SELECT id, order_id, order_code, order_name, apparatus,
                sender_role, sender_ref, sender_display_name, items_json,
                rasxot_mix_total::TEXT AS rasxot_mix_total,
                astatka_mix_total::TEXT AS astatka_mix_total,
                rasxot_alcohol::TEXT AS rasxot_alcohol,
                astatka_alcohol::TEXT AS astatka_alcohol,
                final_used_alcohol::TEXT AS final_used_alcohol,
                rasxot_pure_paint::TEXT AS rasxot_pure_paint,
                astatka_pure_paint::TEXT AS astatka_pure_paint,
                final_used_paint::TEXT AS final_used_paint,
                EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix
             FROM mini_returned_paint_requests
             WHERE target_role = 'boyoqchi'
             ORDER BY created_at DESC, id DESC
             LIMIT $1 OFFSET $2",
        )
        .bind(limit.max(1) as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ReturnedPaintError::StoreFailed)?;
        rows.into_iter().map(ReturnedPaintRow::into_model).collect()
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
    rasxot_mix_total: Option<String>,
    astatka_mix_total: Option<String>,
    rasxot_alcohol: Option<String>,
    astatka_alcohol: Option<String>,
    final_used_alcohol: Option<String>,
    rasxot_pure_paint: Option<String>,
    astatka_pure_paint: Option<String>,
    final_used_paint: Option<String>,
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
            calculation,
            message: String::new(),
            created_at_unix: self.created_at_unix,
        };
        request.message = completion_report_message(&request);
        Ok(request)
    }
}

pub(crate) async fn insert_returned_paint_request_tx(
    tx: &mut Transaction<'_, Postgres>,
    request: &ReturnedPaintRequest,
) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
    let items_json = serde_json::to_value(&request.items)
        .map_err(|_| ReturnedPaintError::StoreFailed)?;
    let calculation = request
        .calculation
        .as_ref()
        .ok_or(ReturnedPaintError::StoreFailed)?;
    let row = sqlx::query_as::<_, ReturnedPaintRow>(
        "INSERT INTO mini_returned_paint_requests (
            id, target_role, order_id, order_code, order_name, apparatus,
            sender_role, sender_ref, sender_display_name, items_json,
            rasxot_mix_total, astatka_mix_total, rasxot_alcohol, astatka_alcohol,
            final_used_alcohol, rasxot_pure_paint, astatka_pure_paint,
            final_used_paint, created_at
         ) VALUES (
            $1, 'boyoqchi', $2, $3, $4, $5, $6, $7, $8, $9,
            $10::NUMERIC, $11::NUMERIC, $12::NUMERIC, $13::NUMERIC,
            $14::NUMERIC, $15::NUMERIC, $16::NUMERIC, $17::NUMERIC,
            to_timestamp($18)
         )
         ON CONFLICT (id) DO NOTHING
         RETURNING id, order_id, order_code, order_name, apparatus,
            sender_role, sender_ref, sender_display_name, items_json,
            rasxot_mix_total::TEXT AS rasxot_mix_total,
            astatka_mix_total::TEXT AS astatka_mix_total,
            rasxot_alcohol::TEXT AS rasxot_alcohol,
            astatka_alcohol::TEXT AS astatka_alcohol,
            final_used_alcohol::TEXT AS final_used_alcohol,
            rasxot_pure_paint::TEXT AS rasxot_pure_paint,
            astatka_pure_paint::TEXT AS astatka_pure_paint,
            final_used_paint::TEXT AS final_used_paint,
            EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix",
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
    .bind(&calculation.rasxot_mix_total)
    .bind(&calculation.astatka_mix_total)
    .bind(&calculation.rasxot_alcohol)
    .bind(&calculation.astatka_alcohol)
    .bind(&calculation.final_used_alcohol)
    .bind(&calculation.rasxot_pure_paint)
    .bind(&calculation.astatka_pure_paint)
    .bind(&calculation.final_used_paint)
    .bind(request.created_at_unix as f64)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ReturnedPaintError::StoreFailed)?;
    let row = match row {
        Some(row) => row,
        None => sqlx::query_as::<_, ReturnedPaintRow>(
            "SELECT id, order_id, order_code, order_name, apparatus,
                sender_role, sender_ref, sender_display_name, items_json,
                rasxot_mix_total::TEXT AS rasxot_mix_total,
                astatka_mix_total::TEXT AS astatka_mix_total,
                rasxot_alcohol::TEXT AS rasxot_alcohol,
                astatka_alcohol::TEXT AS astatka_alcohol,
                final_used_alcohol::TEXT AS final_used_alcohol,
                rasxot_pure_paint::TEXT AS rasxot_pure_paint,
                astatka_pure_paint::TEXT AS astatka_pure_paint,
                final_used_paint::TEXT AS final_used_paint,
                EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix
             FROM mini_returned_paint_requests
             WHERE id = $1",
        )
        .bind(&request.id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ReturnedPaintError::StoreFailed)?
        .ok_or(ReturnedPaintError::StoreFailed)?,
    };
    row.into_model()
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

fn parse_decimal(value: &str) -> Result<String, ReturnedPaintError> {
    normalize_returned_paint_stored_decimal(value)
        .map_err(|_| ReturnedPaintError::StoreFailed)
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
