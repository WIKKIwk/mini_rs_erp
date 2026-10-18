use sqlx::PgPool;

use crate::core::qolip::{QolipError, QolipProduct};

pub(super) async fn load(
    pool: &PgPool,
    item_code: &str,
) -> Result<Option<QolipProduct>, QolipError> {
    let item_code = item_code.trim();
    if item_code.is_empty() {
        return Ok(None);
    }
    let row = sqlx::query_as::<_, (String, String, String)>(
        r#"
        SELECT code, name, item_group
        FROM mini_items
        WHERE lower(btrim(code)) = lower($1)
        ORDER BY (code = $1) DESC, code
        LIMIT 1
        "#,
    )
    .bind(item_code)
    .fetch_optional(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    Ok(row.map(|(code, name, item_group)| QolipProduct {
        code,
        name,
        item_group,
        ..Default::default()
    }))
}
