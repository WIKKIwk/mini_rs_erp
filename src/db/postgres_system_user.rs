use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::auth::models::PrincipalRole;
use crate::core::system_users::{
    SystemUser, SystemUserError, SystemUserStorePort,
};

#[derive(Clone)]
pub struct PostgresSystemUserStore {
    pool: PgPool,
}

impl PostgresSystemUserStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SystemUserStorePort for PostgresSystemUserStore {
    async fn users(
        &self,
        role: &PrincipalRole,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SystemUser>, SystemUserError> {
        let role = role_as_str(role)?;
        let needle = query.trim().to_lowercase();
        let pattern = format!("%{needle}%");
        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT id, role, name, phone
             FROM mini_system_users
             WHERE role = $1
               AND ($2 = '' OR lower(name) LIKE $3 OR lower(phone) LIKE $3)
             ORDER BY lower(name), id
             LIMIT $4",
        )
        .bind(role)
        .bind(needle)
        .bind(pattern)
        .bind(limit.max(1) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| SystemUserError::StoreFailed)?;
        rows.into_iter().map(row_to_user).collect()
    }

    async fn users_by_ids(&self, ids: &[String]) -> Result<Vec<SystemUser>, SystemUserError> {
        let ids = ids
            .iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT id, role, name, phone
             FROM mini_system_users
             WHERE id = ANY($1)
             ORDER BY array_position($1, id)",
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| SystemUserError::StoreFailed)?;
        rows.into_iter().map(row_to_user).collect()
    }

    async fn upsert_user(&self, user: SystemUser) -> Result<SystemUser, SystemUserError> {
        let role = role_as_str(&user.role)?;
        let payload = serde_json::to_value(&user).map_err(|_| SystemUserError::StoreFailed)?;
        let row = sqlx::query_as::<_, (String, String, String, String)>(
            "INSERT INTO mini_system_users (id, role, name, phone, payload_json)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (id) DO UPDATE SET
               role = excluded.role,
               name = excluded.name,
               phone = excluded.phone,
               payload_json = excluded.payload_json,
               updated_at = now()
             RETURNING id, role, name, phone",
        )
        .bind(user.id)
        .bind(role)
        .bind(user.name)
        .bind(user.phone)
        .bind(payload)
        .fetch_one(&self.pool)
        .await
        .map_err(map_write_error)?;
        row_to_user(row)
    }
}

fn row_to_user(
    (id, role, name, phone): (String, String, String, String),
) -> Result<SystemUser, SystemUserError> {
    Ok(SystemUser {
        id,
        role: role_from_str(&role)?,
        name,
        phone,
    })
}

fn role_as_str(role: &PrincipalRole) -> Result<&'static str, SystemUserError> {
    match role {
        PrincipalRole::Qolipchi => Ok("qolipchi"),
        _ => Err(SystemUserError::InvalidRole),
    }
}

fn role_from_str(role: &str) -> Result<PrincipalRole, SystemUserError> {
    match role.trim().to_lowercase().as_str() {
        "qolipchi" => Ok(PrincipalRole::Qolipchi),
        _ => Err(SystemUserError::InvalidRole),
    }
}

fn map_write_error(error: sqlx::Error) -> SystemUserError {
    if error
        .as_database_error()
        .and_then(|error| error.constraint())
        == Some("idx_mini_system_users_role_phone_key_unique")
    {
        SystemUserError::DuplicatePhone
    } else {
        SystemUserError::StoreFailed
    }
}
