use async_trait::async_trait;
use sqlx::{FromRow, PgPool};

use crate::core::admin::item_customer_policy::FINISHED_GOODS_CUSTOMER_REQUIRED;
use crate::core::admin::models::AdminDirectoryEntry;
use crate::core::admin::ports::AdminPortError;
use crate::core::auth::ports::{AuthPortError, CustomerLookup, CustomerRecord};
use crate::core::werka::models::SupplierItem;
use crate::db::postgres_admin_catalog::customer_policy::{
    item_requires_customer, lock_item_customer_policy,
};

#[derive(Debug, Clone)]
pub struct PostgresCustomerStore {
    pool: PgPool,
}

#[derive(Debug, FromRow)]
struct CustomerRow {
    customer_ref: String,
    name: String,
    phone: String,
}

#[derive(Debug, FromRow)]
struct CustomerItemRow {
    code: String,
    name: String,
    uom: String,
    item_group: String,
}

impl PostgresCustomerStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        let needle = format!("%{}%", query.trim().to_lowercase());
        sqlx::query_as::<_, CustomerRow>(
            "SELECT ref AS customer_ref, name, phone
             FROM mini_customers
             WHERE $1 = '%%'
                OR lower(ref) LIKE $1
                OR lower(name) LIKE $1
                OR lower(phone) LIKE $1
             ORDER BY lower(name), ref
             LIMIT $2 OFFSET $3",
        )
        .bind(needle)
        .bind(limit.min(500) as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(CustomerRow::into_entry).collect())
        .map_err(|_| AdminPortError::LookupFailed)
    }

    pub async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        sqlx::query_as::<_, CustomerRow>(
            "SELECT ref AS customer_ref, name, phone
             FROM mini_customers
             WHERE ref = $1",
        )
        .bind(ref_.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?
        .map(CustomerRow::into_entry)
        .ok_or(AdminPortError::NotFound)
    }

    pub async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "customer name is required".to_string(),
            ));
        }

        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        let ref_: String =
            sqlx::query_scalar("SELECT format('CUST-%s', nextval('mini_customer_ref_seq'))")
                .fetch_one(&mut *transaction)
                .await
                .map_err(|_| AdminPortError::LookupFailed)?;
        sqlx::query(
            "INSERT INTO mini_customers (ref, name, phone)
             VALUES ($1, $2, $3)",
        )
        .bind(&ref_)
        .bind(name)
        .bind(phone.trim())
        .execute(&mut *transaction)
        .await
        .map_err(map_customer_write_error)?;
        transaction
            .commit()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        Ok(AdminDirectoryEntry {
            ref_,
            name: name.to_string(),
            phone: phone.trim().to_string(),
        })
    }

    pub async fn update_customer_phone(
        &self,
        ref_: &str,
        phone: &str,
    ) -> Result<(), AdminPortError> {
        let affected = sqlx::query(
            "UPDATE mini_customers
             SET phone = $2, updated_at = now()
             WHERE ref = $1",
        )
        .bind(ref_.trim())
        .bind(phone.trim())
        .execute(&self.pool)
        .await
        .map_err(map_customer_write_error)?
        .rows_affected();
        if affected == 0 {
            return Err(AdminPortError::NotFound);
        }
        Ok(())
    }

    pub async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let needle = format!("%{}%", query.trim().to_lowercase());
        sqlx::query_as::<_, CustomerItemRow>(
            "SELECT items.code, items.name, items.uom, items.item_group
             FROM mini_customer_items assignments
             JOIN mini_items items ON items.code = assignments.item_code
             WHERE assignments.customer_ref = $1
               AND (
                    $2 = '%%'
                    OR lower(items.code) LIKE $2
                    OR lower(items.name) LIKE $2
                    OR lower(items.item_group) LIKE $2
               )
             ORDER BY lower(items.code)
             LIMIT $3",
        )
        .bind(customer_ref.trim())
        .bind(needle)
        .bind(limit.min(500) as i64)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(CustomerItemRow::into_item).collect())
        .map_err(|_| AdminPortError::LookupFailed)
    }

    pub async fn assign_customer_item(
        &self,
        customer_ref: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        sqlx::query(
            "INSERT INTO mini_customer_items (customer_ref, item_code)
             VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(customer_ref.trim())
        .bind(item_code.trim())
        .execute(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        Ok(())
    }

    pub async fn unassign_customer_item(
        &self,
        customer_ref: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.unassign_customer_item_guarded(customer_ref, item_code)
            .await
    }

    pub async fn unassign_customer_item_guarded(
        &self,
        customer_ref: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        lock_item_customer_policy(&mut transaction).await?;
        let stored_code = sqlx::query_scalar::<_, String>(
            "SELECT code
             FROM mini_items
             WHERE lower(code) = lower($1)
             FOR UPDATE",
        )
        .bind(item_code.trim())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?
        .ok_or(AdminPortError::NotFound)?;
        let assignment_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                 SELECT 1
                 FROM mini_customer_items
                 WHERE customer_ref = $1 AND item_code = $2
             )",
        )
        .bind(customer_ref.trim())
        .bind(&stored_code)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        if !assignment_exists {
            transaction
                .commit()
                .await
                .map_err(|_| AdminPortError::LookupFailed)?;
            return Ok(());
        }
        if item_requires_customer(&mut transaction, &stored_code).await? {
            let customer_count = sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM mini_customer_items WHERE item_code = $1",
            )
            .bind(&stored_code)
            .fetch_one(&mut *transaction)
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
            if customer_count <= 1 {
                return Err(AdminPortError::InvalidInput(
                    FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
                ));
            }
        }
        sqlx::query(
            "DELETE FROM mini_customer_items
             WHERE customer_ref = $1 AND item_code = $2",
        )
        .bind(customer_ref.trim())
        .bind(&stored_code)
        .execute(&mut *transaction)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        transaction
            .commit()
            .await
            .map_err(|_| AdminPortError::LookupFailed)
    }
}

fn map_customer_write_error(error: sqlx::Error) -> AdminPortError {
    if error
        .as_database_error()
        .and_then(|error| error.constraint())
        == Some("idx_mini_customers_phone_key_unique")
    {
        AdminPortError::InvalidInput("phone already exists".to_string())
    } else {
        AdminPortError::LookupFailed
    }
}

#[async_trait]
impl CustomerLookup for PostgresCustomerStore {
    async fn search_customers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CustomerRecord>, AuthPortError> {
        self.customers_page(query, limit, 0)
            .await
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| CustomerRecord {
                        id: entry.ref_,
                        name: entry.name,
                        phone: entry.phone,
                    })
                    .collect()
            })
            .map_err(|_| AuthPortError::LookupFailed)
    }
}

impl CustomerRow {
    fn into_entry(self) -> AdminDirectoryEntry {
        AdminDirectoryEntry {
            ref_: self.customer_ref,
            name: self.name,
            phone: self.phone,
        }
    }
}

impl CustomerItemRow {
    fn into_item(self) -> SupplierItem {
        SupplierItem {
            code: self.code,
            name: self.name,
            uom: self.uom,
            warehouse: String::new(),
            item_group: self.item_group,
        }
    }
}
