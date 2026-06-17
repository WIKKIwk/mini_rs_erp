use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::admin::models::AdminWarehouse;
use crate::core::warehouses::{WarehouseError, WarehouseStorePort};

#[derive(Clone)]
pub struct PostgresWarehouseStore {
    pool: PgPool,
}

impl PostgresWarehouseStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WarehouseStorePort for PostgresWarehouseStore {
    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, WarehouseError> {
        let query = query.trim().to_lowercase();
        let pattern = format!("%{query}%");
        let parent = parent.trim().to_lowercase();
        let rows = sqlx::query_as::<_, WarehouseRow>(
            "SELECT name, company, is_group, parent_warehouse
             FROM mini_warehouses
             WHERE ($1 = '' OR lower(name) LIKE $2)
               AND ($3 = '' OR lower(parent_warehouse) = $3)
             ORDER BY lower(name) ASC
             LIMIT $4",
        )
        .bind(query)
        .bind(pattern)
        .bind(parent)
        .bind(limit.max(1) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| WarehouseError::StoreFailed)?;

        Ok(rows
            .into_iter()
            .map(|row| AdminWarehouse {
                warehouse: row.name,
                company: row.company,
                is_group: row.is_group,
                parent_warehouse: row.parent_warehouse,
            })
            .collect())
    }

    async fn put_warehouse(
        &self,
        warehouse: AdminWarehouse,
    ) -> Result<AdminWarehouse, WarehouseError> {
        let name = warehouse.warehouse.trim();
        if name.is_empty() {
            return Err(WarehouseError::MissingWarehouse);
        }
        sqlx::query_as::<_, WarehouseRow>(
            "INSERT INTO mini_warehouses (
                 id, name, company, is_group, parent_warehouse, payload_json
             )
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT ((lower(name))) DO UPDATE SET
               name = excluded.name,
               company = excluded.company,
               is_group = excluded.is_group,
               parent_warehouse = excluded.parent_warehouse,
               payload_json = excluded.payload_json,
               updated_at = now()
             RETURNING name, company, is_group, parent_warehouse",
        )
        .bind(warehouse_id(name))
        .bind(name)
        .bind(warehouse.company.trim())
        .bind(warehouse.is_group)
        .bind(warehouse.parent_warehouse.trim())
        .bind(serde_json::json!({
            "warehouse": name,
            "company": warehouse.company.trim(),
            "is_group": warehouse.is_group,
            "parent_warehouse": warehouse.parent_warehouse.trim(),
        }))
        .fetch_one(&self.pool)
        .await
        .map(|row| AdminWarehouse {
            warehouse: row.name,
            company: row.company,
            is_group: row.is_group,
            parent_warehouse: row.parent_warehouse,
        })
        .map_err(|_| WarehouseError::StoreFailed)
    }
}

#[derive(sqlx::FromRow)]
struct WarehouseRow {
    name: String,
    company: String,
    is_group: bool,
    parent_warehouse: String,
}

fn warehouse_id(name: &str) -> String {
    format!("warehouse:{}", name.trim().to_lowercase())
}
