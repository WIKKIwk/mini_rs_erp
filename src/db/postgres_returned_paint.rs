use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::auth::models::PrincipalRole;
use crate::core::returned_paint::{
    ReturnedPaintError, ReturnedPaintItem, ReturnedPaintRequest, ReturnedPaintStorePort,
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
        let items_json = serde_json::to_value(&request.items)
            .map_err(|_| ReturnedPaintError::StoreFailed)?;
        let row = sqlx::query_as::<_, ReturnedPaintRow>(
            "INSERT INTO mini_returned_paint_requests (
                id, target_role, order_id, order_code, order_name, apparatus,
                sender_role, sender_ref, sender_display_name, items_json, created_at
             ) VALUES ($1, 'boyoqchi', $2, $3, $4, $5, $6, $7, $8, $9, to_timestamp($10))
             RETURNING id, order_id, order_code, order_name, apparatus,
                sender_role, sender_ref, sender_display_name, items_json,
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
        .bind(request.created_at_unix as f64)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| ReturnedPaintError::StoreFailed)?;
        row.into_model()
    }

    async fn list(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ReturnedPaintRequest>, ReturnedPaintError> {
        let rows = sqlx::query_as::<_, ReturnedPaintRow>(
            "SELECT id, order_id, order_code, order_name, apparatus,
                sender_role, sender_ref, sender_display_name, items_json,
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
    created_at_unix: i64,
}

impl ReturnedPaintRow {
    fn into_model(self) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        Ok(ReturnedPaintRequest {
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
            created_at_unix: self.created_at_unix,
        })
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
