use crate::core::calculate_orders::CalculateOrderImage;
use crate::core::pending_orders::*;
use crate::core::production_map::ProductionMapSaved;
use async_trait::async_trait;
use sqlx::{PgPool, Row};

pub struct PostgresPendingOrderStore(pub PgPool);

fn store_error<E: std::fmt::Debug>(error: E) -> PendingOrderError {
    tracing::error!(?error, "pending order store operation failed");
    PendingOrderError::Store
}

fn decode(row: &sqlx::postgres::PgRow) -> Result<PendingOrder, PendingOrderError> {
    let mut order: PendingOrder =
        serde_json::from_value(row.try_get("payload_json").map_err(store_error)?)
            .map_err(store_error)?;
    order.completion = row
        .try_get::<Option<serde_json::Value>, _>("completion_json")
        .map_err(store_error)?
        .map(serde_json::from_value)
        .transpose()
        .map_err(store_error)?;
    Ok(order)
}

#[async_trait]
impl PendingOrderStore for PostgresPendingOrderStore {
    async fn create(
        &self,
        order: PendingOrder,
        image: CalculateOrderImage,
    ) -> Result<PendingOrder, PendingOrderError> {
        order.validate()?;
        if image.body.is_empty() || image.image_id != order.template.image_id {
            return Err(PendingOrderError::Invalid("rasm kerak".into()));
        }
        sqlx::query("INSERT INTO mini_pending_orders (id, order_number, telegram_user_id, payload_json, image_body)
            VALUES ($1,$2,$3,$4,$5) ON CONFLICT (id) DO NOTHING")
            .bind(&order.id).bind(&order.template.order_number).bind(&order.telegram_user_id)
            .bind(serde_json::to_value(&order).map_err(store_error)?).bind(image.body)
            .execute(&self.0).await.map_err(store_error)?;
        let existing = self.get(&order.id).await?;
        if existing.telegram_user_id != order.telegram_user_id {
            return Err(PendingOrderError::Conflict);
        }
        Ok(existing)
    }

    async fn list(&self) -> Result<Vec<PendingOrder>, PendingOrderError> {
        sqlx::query("SELECT payload_json, completion_json FROM mini_pending_orders WHERE completion_json IS NULL ORDER BY created_at DESC")
            .fetch_all(&self.0).await.map_err(store_error)?.iter().map(decode).collect()
    }

    async fn get(&self, id: &str) -> Result<PendingOrder, PendingOrderError> {
        let row = sqlx::query(
            "SELECT payload_json, completion_json FROM mini_pending_orders WHERE id=$1",
        )
        .bind(id)
        .fetch_optional(&self.0)
        .await
        .map_err(store_error)?
        .ok_or(PendingOrderError::NotFound)?;
        decode(&row)
    }

    async fn image(&self, id: &str) -> Result<CalculateOrderImage, PendingOrderError> {
        let row = sqlx::query(
            "SELECT payload_json, completion_json, image_body FROM mini_pending_orders WHERE id=$1",
        )
        .bind(id)
        .fetch_optional(&self.0)
        .await
        .map_err(store_error)?
        .ok_or(PendingOrderError::NotFound)?;
        let t = decode(&row)?.template;
        Ok(CalculateOrderImage {
            image_id: t.image_id,
            image_name: t.image_name,
            image_mime: t.image_mime,
            image_size_bytes: t.image_size_bytes,
            body: row.try_get("image_body").map_err(store_error)?,
        })
    }

    async fn complete(
        &self,
        id: &str,
        owner_key: &str,
        completion: PendingOrderCompletion,
        template_map: ProductionMapSaved,
    ) -> Result<(PendingOrderCompletion, bool), PendingOrderError> {
        let mut tx = self.0.begin().await.map_err(store_error)?;
        let row = sqlx::query("SELECT payload_json, completion_json, image_body FROM mini_pending_orders WHERE id=$1 FOR UPDATE")
            .bind(id).fetch_optional(&mut *tx).await.map_err(store_error)?.ok_or(PendingOrderError::NotFound)?;
        let order = decode(&row)?;
        if let Some(existing) = order.completion {
            return Ok((existing, false));
        }
        if completion.saved.map.id != id
            || completion.saved.map.order_number != order.template.order_number
        {
            return Err(PendingOrderError::Conflict);
        }
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_production_maps WHERE id=$1)")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .map_err(store_error)?;
        if exists {
            return Err(PendingOrderError::Conflict);
        }
        for map in [&completion.saved.map, &template_map.map] {
            super::reject_duplicate_order_number_tx(&mut tx, map)
                .await
                .map_err(store_error)?;
            super::put_map_inner_tx(&mut tx, map)
                .await
                .map_err(store_error)?;
        }
        let t = &completion.template;
        sqlx::query("INSERT INTO mini_quick_order_templates
            (id,owner_key,code,name,item_code,product_name,customer_ref,customer_name,payload_json,quick_key)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(&t.id).bind(owner_key).bind(&t.code).bind(&t.name).bind(&t.item_code)
            .bind(&t.product).bind(&t.customer_ref).bind(&t.customer)
            .bind(serde_json::to_value(t).map_err(store_error)?)
            .bind(crate::db::postgres_calculate_order::quick_template_key(t))
            .execute(&mut *tx).await.map_err(store_error)?;
        sqlx::query("INSERT INTO mini_quick_order_images
            (owner_key,image_id,image_name,image_mime,image_size_bytes,body) VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(owner_key).bind(&t.image_id).bind(&t.image_name).bind(&t.image_mime)
            .bind(t.image_size_bytes as i64).bind(row.try_get::<Vec<u8>, _>("image_body").map_err(store_error)?)
            .execute(&mut *tx).await.map_err(store_error)?;
        let mut order_snapshot = t.clone();
        order_snapshot.code = completion.saved.map.code.clone();
        order_snapshot.source_map_id = completion.saved.map.id.clone();
        crate::db::postgres_mini_order::save_order_tx(&mut tx, &completion.saved.map, &order_snapshot)
            .await
            .map_err(store_error)?;
        sqlx::query(
            "UPDATE mini_pending_orders SET completion_json=$2,completed_at=now() WHERE id=$1",
        )
        .bind(id)
        .bind(serde_json::to_value(&completion).map_err(store_error)?)
        .execute(&mut *tx)
        .await
        .map_err(store_error)?;
        tx.commit().await.map_err(store_error)?;
        Ok((completion, true))
    }
}
