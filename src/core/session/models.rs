use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::core::auth::models::Principal;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub principal: Principal,
    #[serde(
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub created_at: Option<OffsetDateTime>,
    #[serde(
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub updated_at: Option<OffsetDateTime>,
    #[serde(
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub expires_at: Option<OffsetDateTime>,
}

impl SessionRecord {
    pub fn new(
        principal: Principal,
        now: OffsetDateTime,
        created_at: Option<OffsetDateTime>,
        ttl_seconds: Option<u64>,
    ) -> Self {
        Self {
            principal,
            created_at: Some(created_at.unwrap_or(now)),
            updated_at: Some(now),
            expires_at: ttl_seconds.map(|seconds| now + time::Duration::seconds(seconds as i64)),
        }
    }

    pub fn is_expired(&self, now: OffsetDateTime) -> bool {
        self.expires_at.is_some_and(|expires_at| now > expires_at)
    }
}
