mod json;
mod lmdb;
mod lmdb_codec;
mod lmdb_expiry;

use async_trait::async_trait;

use crate::core::auth::models::PrincipalRole;
use crate::core::session::models::SessionRecord;
use crate::error::AppError;

pub use json::JsonSessionStore;
pub use lmdb::LmdbSessionStore;

#[cfg(test)]
use heed::{BytesDecode, BytesEncode};
pub(crate) use lmdb::session_key;
#[cfg(test)]
use lmdb_codec::SessionRecordCodec;

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn get(&self, token: &str) -> Result<Option<SessionRecord>, AppError>;
    /// Resolve a server-private digest without trusting a process-local session cache.
    async fn get_by_digest(&self, digest: &[u8; 32]) -> Result<Option<SessionRecord>, AppError>;
    async fn ticket_session(
        &self,
        locator: &[u8; 16],
    ) -> Result<Option<([u8; 32], SessionRecord)>, AppError>;
    async fn put(&self, token: &str, record: &SessionRecord) -> Result<(), AppError>;
    async fn delete(&self, token: &str) -> Result<(), AppError>;
    async fn delete_if_expired(
        &self,
        digest: &[u8; 32],
        now: time::OffsetDateTime,
        ttl_seconds: Option<u64>,
    ) -> Result<(), AppError>;
    async fn delete_for_principal(
        &self,
        role: &PrincipalRole,
        principal_ref: &str,
    ) -> Result<usize, AppError>;
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
