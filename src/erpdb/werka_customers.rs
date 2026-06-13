use sqlx::{MySqlPool, query_as};

use crate::core::werka::models::CustomerDirectoryEntry;
use crate::erpdb::werka_suppliers::{clamp_limit, like_pattern};

pub(crate) async fn read_werka_customers(
    pool: &MySqlPool,
    query: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<CustomerDirectoryEntry>, sqlx::Error> {
    let limit = clamp_limit(limit, 50, 500);
    let like = like_pattern(query);
    let rows = query_as::<_, CustomerDirectoryRow>(WERKA_CUSTOMERS_SQL)
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
        .map(|row| CustomerDirectoryEntry {
            ref_: row.ref_,
            name: row.name,
            phone: row.phone,
        })
        .collect())
}

#[derive(Debug, sqlx::FromRow)]
struct CustomerDirectoryRow {
    #[sqlx(rename = "ref")]
    ref_: String,
    name: String,
    phone: String,
}

const WERKA_CUSTOMERS_SQL: &str = r#"
    SELECT DISTINCT
        c.name AS ref,
        COALESCE(NULLIF(c.customer_name, ''), c.name) AS name,
        COALESCE(c.mobile_no, '') AS phone
    FROM tabCustomer c
    INNER JOIN `tabItem Customer Detail` icd ON icd.customer_name = c.name
    INNER JOIN tabItem i ON i.name = icd.parent
    WHERE c.disabled = 0
      AND i.disabled = 0
      AND (? = '' OR c.name LIKE ? ESCAPE '\\' OR c.customer_name LIKE ? ESCAPE '\\' OR COALESCE(c.mobile_no, '') LIKE ? ESCAPE '\\')
    ORDER BY c.modified DESC
    LIMIT ? OFFSET ?
"#;
