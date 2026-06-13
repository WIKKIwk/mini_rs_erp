use async_trait::async_trait;
use sqlx::query_scalar;

use crate::core::werka::ports::{CustomerIssueSourceLookup, WerkaPortError};
use crate::erpdb::reader::DirectDbReader;

#[async_trait]
impl CustomerIssueSourceLookup for DirectDbReader {
    async fn customer_issue_source_exists(&self, marker: &str) -> Result<bool, WerkaPortError> {
        let marker = marker.trim();
        if marker.is_empty() {
            return Ok(false);
        }

        let name: Option<String> = query_scalar(
            r#"
            SELECT name
            FROM `tabDelivery Note`
            WHERE COALESCE(accord_source_key, '') = ?
              AND COALESCE(docstatus, 0) < 2
            LIMIT 1
            "#,
        )
        .bind(marker)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| WerkaPortError::Database(error.to_string()))?;

        Ok(name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some())
    }
}
