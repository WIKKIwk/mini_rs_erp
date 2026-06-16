use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::push::models::PushTokenRecord;
use crate::core::push::ports::{PushStoreError, PushTokenStorePort};

#[derive(Clone)]
pub struct PostgresPushTokenStore {
    pool: PgPool,
}

impl PostgresPushTokenStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PushTokenStorePort for PostgresPushTokenStore {
    async fn move_token_to_key(
        &self,
        target_key: &str,
        token: &str,
        platform: &str,
    ) -> Result<(), PushStoreError> {
        let target_key = target_key.trim();
        let token = token.trim();
        let platform = platform.trim();
        sqlx::query(
            "INSERT INTO mini_push_tokens (token, owner_key, platform, updated_at)
             VALUES ($1, $2, $3, now())
             ON CONFLICT (token) DO UPDATE SET
                owner_key = excluded.owner_key,
                platform = excluded.platform,
                updated_at = excluded.updated_at",
        )
        .bind(token)
        .bind(target_key)
        .bind(platform)
        .execute(&self.pool)
        .await
        .map_err(|_| PushStoreError::StoreFailed)?;
        Ok(())
    }

    async fn delete(&self, key: &str, token: &str) -> Result<(), PushStoreError> {
        sqlx::query(
            "DELETE FROM mini_push_tokens
             WHERE owner_key = $1 AND token = $2",
        )
        .bind(key.trim())
        .bind(token.trim())
        .execute(&self.pool)
        .await
        .map_err(|_| PushStoreError::StoreFailed)?;
        Ok(())
    }

    async fn list(&self, key: &str) -> Result<Vec<PushTokenRecord>, PushStoreError> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT token, platform
             FROM mini_push_tokens
             WHERE owner_key = $1
             ORDER BY updated_at DESC, token",
        )
        .bind(key.trim())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| PushStoreError::StoreFailed)?;
        Ok(rows
            .into_iter()
            .map(|(token, platform)| PushTokenRecord {
                token,
                platform,
                updated_at: time::OffsetDateTime::now_utc(),
            })
            .collect())
    }
}
