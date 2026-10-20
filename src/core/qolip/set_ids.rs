use std::sync::atomic::{AtomicU64, Ordering};

use super::QolipError;

// In-memory stores have no persistent database. PostgreSQL overrides the port
// with its own durable sequence, shared by every server process.
pub(super) fn next_memory_set_id() -> Result<String, QolipError> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let number = NEXT
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            (value <= 99_999_999).then_some(value + 1)
        })
        .map_err(|_| QolipError::StoreFailed)?;
    Ok(format!("9501{number:08}"))
}
