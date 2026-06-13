use sqlx::{MySqlPool, query_as};

use crate::core::werka::models::SupplierDirectoryEntry;

pub(crate) async fn read_werka_suppliers(
    pool: &MySqlPool,
    query: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<SupplierDirectoryEntry>, sqlx::Error> {
    let limit = clamp_limit(limit, 50, 500);
    let like = like_pattern(query);
    let rows = query_as::<_, SupplierDirectoryRow>(WERKA_SUPPLIERS_SQL)
        .bind(query.trim())
        .bind(&like)
        .bind(&like)
        .bind(&like)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(pool)
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| SupplierDirectoryEntry {
            ref_: row.ref_,
            name: row.name,
            phone: row.phone,
        })
        .collect())
}

#[derive(Debug, sqlx::FromRow)]
struct SupplierDirectoryRow {
    #[sqlx(rename = "ref")]
    ref_: String,
    name: String,
    phone: String,
}

pub(crate) fn clamp_limit(value: usize, fallback: usize, max: usize) -> usize {
    let value = if value == 0 { fallback } else { value };
    if max > 0 && value > max { max } else { value }
}

pub(crate) fn like_pattern(query: &str) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return "%".to_string();
    }

    let escaped = trimmed
        .replace('\\', r"\\")
        .replace('%', r"\%")
        .replace('_', r"\_");
    format!("%{escaped}%")
}

const WERKA_SUPPLIERS_SQL: &str = r#"
    SELECT DISTINCT
        s.name AS ref,
        COALESCE(NULLIF(s.supplier_name, ''), s.name) AS name,
        COALESCE(s.mobile_no, '') AS phone
    FROM `tabItem Supplier` isup
    INNER JOIN tabSupplier s ON s.name = isup.supplier
    INNER JOIN tabItem i ON i.name = isup.parent
    WHERE s.disabled = 0
      AND i.disabled = 0
      AND (? = '' OR s.name LIKE ? ESCAPE '\\' OR s.supplier_name LIKE ? ESCAPE '\\' OR COALESCE(s.mobile_no, '') LIKE ? ESCAPE '\\')
    ORDER BY s.modified DESC
    LIMIT ? OFFSET ?
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supplier_limit_clamps_like_go_reader() {
        assert_eq!(clamp_limit(0, 50, 500), 50);
        assert_eq!(clamp_limit(700, 50, 500), 500);
        assert_eq!(clamp_limit(120, 50, 500), 120);
    }

    #[test]
    fn like_pattern_escapes_mysql_wildcards_like_go() {
        assert_eq!(like_pattern(""), "%");
        assert_eq!(like_pattern(r" A%_\\ "), r"%A\%\_\\\\%");
    }
}
