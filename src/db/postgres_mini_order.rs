use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::mini_orders::{MiniOrderError, MiniOrderSink};
use crate::core::production_map::ProductionMapDefinition;

#[derive(Clone)]
pub struct PostgresMiniOrderSink {
    pool: PgPool,
}

impl PostgresMiniOrderSink {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MiniOrderSink for PostgresMiniOrderSink {
    async fn save_order(
        &self,
        map: &ProductionMapDefinition,
        template: &CalculateOrderTemplate,
    ) -> Result<(), MiniOrderError> {
        let order_id = order_id(map);
        let order_code = first_non_empty([&template.code, &map.code, &map.order_number, &map.id]);
        let product_name = first_non_empty([&template.product, &template.name, &map.title]);
        let kg = if template.kg.is_finite() && template.kg > 0.0 {
            template.kg
        } else {
            0.0
        };
        let width_mm = positive_f64(template.width_mm).or(map.width_mm);
        let roll_count = template
            .roll_count
            .or(map.roll_count)
            .filter(|value| *value > 0.0);

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| MiniOrderError::StoreFailed)?;
        sqlx::query(
            "INSERT INTO mini_orders
                (id, code, order_number, customer_ref, customer_name, product_code,
                 product_name, status, kg, width_mm, roll_count, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, now())
             ON CONFLICT (id) DO UPDATE SET
                code = excluded.code,
                order_number = excluded.order_number,
                customer_ref = excluded.customer_ref,
                customer_name = excluded.customer_name,
                product_code = excluded.product_code,
                product_name = excluded.product_name,
                status = excluded.status,
                kg = excluded.kg,
                width_mm = excluded.width_mm,
                roll_count = excluded.roll_count,
                updated_at = excluded.updated_at",
        )
        .bind(&order_id)
        .bind(order_code)
        .bind(first_non_empty([&template.order_number, &map.order_number]))
        .bind(template.customer_ref.trim())
        .bind(template.customer.trim())
        .bind(first_non_empty([&template.item_code, &map.product_code]))
        .bind(product_name)
        .bind(template.status.trim())
        .bind(kg)
        .bind(width_mm)
        .bind(roll_count)
        .execute(&mut *tx)
        .await
        .map_err(|_| MiniOrderError::StoreFailed)?;

        sqlx::query("DELETE FROM mini_order_products WHERE order_id = $1")
            .bind(&order_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| MiniOrderError::StoreFailed)?;

        sqlx::query(
            "INSERT INTO mini_order_products
                (id, order_id, item_code, product_name, material_display, color,
                 first_layer_material, first_layer_micron, second_layer_material,
                 second_layer_micron, third_layer_material, third_layer_micron, note)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(format!("{order_id}:product"))
        .bind(&order_id)
        .bind(template.item_code.trim())
        .bind(product_name)
        .bind(template.material_display.trim())
        .bind(template.color.trim())
        .bind(template.first_layer_material.trim())
        .bind(template.first_layer_micron.trim())
        .bind(template.second_layer_material.trim())
        .bind(template.second_layer_micron.trim())
        .bind(template.third_layer_material.trim())
        .bind(template.third_layer_micron.trim())
        .bind(template.note.trim())
        .execute(&mut *tx)
        .await
        .map_err(|_| MiniOrderError::StoreFailed)?;

        tx.commit().await.map_err(|_| MiniOrderError::StoreFailed)
    }
}

fn order_id(map: &ProductionMapDefinition) -> String {
    let id = map.id.trim();
    if id.is_empty() {
        "order:unknown".to_string()
    } else {
        id.to_string()
    }
}

fn positive_f64(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

fn first_non_empty<const N: usize>(values: [&str; N]) -> &str {
    values
        .into_iter()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or("")
}
