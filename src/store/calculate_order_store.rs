use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};

use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderStorePort, CalculateOrderTemplate, validate_template,
};

#[derive(Clone)]
pub struct CalculateOrderStore {
    conn: Arc<Mutex<Connection>>,
}

impl CalculateOrderStore {
    pub fn new(path: PathBuf) -> Self {
        Self::open(path).unwrap_or_else(|error| {
            panic!("calculate order sqlite store unavailable: {error}");
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, CalculateOrderError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|_| CalculateOrderError::StoreFailed)?;
        }
        let conn = Connection::open(path).map_err(|_| CalculateOrderError::StoreFailed)?;
        configure_connection(&conn).map_err(|_| CalculateOrderError::StoreFailed)?;
        migrate(&conn).map_err(|_| CalculateOrderError::StoreFailed)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

#[async_trait]
impl CalculateOrderStorePort for CalculateOrderStore {
    async fn list(
        &self,
        owner_key: &str,
    ) -> Result<Vec<CalculateOrderTemplate>, CalculateOrderError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CalculateOrderError::StoreFailed)?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json
                 FROM calculate_order_templates
                 WHERE owner_key = ?1
                 ORDER BY saved_at DESC",
            )
            .map_err(|_| CalculateOrderError::StoreFailed)?;
        let rows = stmt
            .query_map(params![owner_key.trim()], |row| {
                let payload: String = row.get(0)?;
                let template = serde_json::from_str::<CalculateOrderTemplate>(&payload)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
                Ok(template)
            })
            .map_err(|_| CalculateOrderError::StoreFailed)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map(dedupe_templates)
            .map_err(|_| CalculateOrderError::StoreFailed)
    }

    async fn list_all(&self) -> Result<Vec<CalculateOrderTemplate>, CalculateOrderError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CalculateOrderError::StoreFailed)?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json
                 FROM calculate_order_templates
                 ORDER BY saved_at DESC",
            )
            .map_err(|_| CalculateOrderError::StoreFailed)?;
        let rows = stmt
            .query_map([], |row| {
                let payload: String = row.get(0)?;
                let template = serde_json::from_str::<CalculateOrderTemplate>(&payload)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
                Ok(template)
            })
            .map_err(|_| CalculateOrderError::StoreFailed)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map(dedupe_templates)
            .map_err(|_| CalculateOrderError::StoreFailed)
    }

    async fn upsert(
        &self,
        owner_key: &str,
        template: CalculateOrderTemplate,
    ) -> Result<CalculateOrderTemplate, CalculateOrderError> {
        validate_template(&template)?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| CalculateOrderError::StoreFailed)?;
        let mut incoming = template;
        if incoming.code.trim().is_empty() {
            incoming.code = format!("Z-{}", new_id());
        }
        let existing = existing_id_by_code(&conn, owner_key, &incoming.code)?;
        let saved = stamp_template(incoming, existing);
        let lower_code = normalize_key(&saved.code);
        let lower_name = normalize_key(&saved.name);
        let payload =
            serde_json::to_string(&saved).map_err(|_| CalculateOrderError::StoreFailed)?;
        conn.execute(
            "INSERT INTO calculate_order_templates
                (id, owner_key, code, lower_code, name, lower_name, saved_at, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(owner_key, lower_code) DO UPDATE SET
                id = excluded.id,
                code = excluded.code,
                name = excluded.name,
                lower_name = excluded.lower_name,
                saved_at = excluded.saved_at,
                payload_json = excluded.payload_json",
            params![
                saved.id,
                owner_key.trim(),
                saved.code,
                lower_code,
                saved.name,
                lower_name,
                saved.saved_at,
                payload
            ],
        )
        .map_err(|_| CalculateOrderError::StoreFailed)?;
        Ok(saved)
    }

    async fn delete(&self, owner_key: &str, id: &str) -> Result<(), CalculateOrderError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CalculateOrderError::StoreFailed)?;
        conn.execute(
            "DELETE FROM calculate_order_templates WHERE owner_key = ?1 AND id = ?2",
            params![owner_key.trim(), id.trim()],
        )
        .map_err(|_| CalculateOrderError::StoreFailed)?;
        Ok(())
    }
}

fn existing_id_by_code(
    conn: &Connection,
    owner_key: &str,
    code: &str,
) -> Result<Option<String>, CalculateOrderError> {
    conn.query_row(
        "SELECT id
         FROM calculate_order_templates
         WHERE owner_key = ?1 AND lower_code = ?2
         ORDER BY saved_at DESC
         LIMIT 1",
        params![owner_key.trim(), normalize_key(code)],
        |row| row.get(0),
    )
    .optional()
    .map_err(|_| CalculateOrderError::StoreFailed)
}

fn stamp_template(
    mut template: CalculateOrderTemplate,
    existing_id: Option<String>,
) -> CalculateOrderTemplate {
    template.id = existing_id
        .filter(|id| !id.trim().is_empty())
        .or_else(|| (!template.id.trim().is_empty()).then(|| template.id.trim().to_string()))
        .unwrap_or_else(new_id);
    template.code = template.code.trim().to_string();
    template.name = template.name.trim().to_string();
    template.order_number = template.order_number.trim().to_string();
    template.customer_ref = template.customer_ref.trim().to_string();
    template.customer = template.customer.trim().to_string();
    template.item_code = template.item_code.trim().to_string();
    template.product = template.product.trim().to_string();
    template.status = template.status.trim().to_string();
    template.material_display = template.material_display.trim().to_string();
    template.color = template.color.trim().to_string();
    template.image_id = template.image_id.trim().to_string();
    template.image_name = template.image_name.trim().to_string();
    template.image_mime = template.image_mime.trim().to_string();
    template.image_url = template.image_url.trim().to_string();
    template.first_layer_material = template.first_layer_material.trim().to_string();
    template.first_layer_micron = template.first_layer_micron.trim().to_string();
    template.second_layer_material = template.second_layer_material.trim().to_string();
    template.second_layer_micron = template.second_layer_micron.trim().to_string();
    template.third_layer_material = template.third_layer_material.trim().to_string();
    template.third_layer_micron = template.third_layer_micron.trim().to_string();
    template.note = template.note.trim().to_string();
    template.source_map_id = template.source_map_id.trim().to_string();
    template.saved_at = unix_micros().to_string();
    template
}

fn configure_connection(conn: &Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS calculate_order_templates (
            id TEXT PRIMARY KEY,
            owner_key TEXT NOT NULL,
            code TEXT NOT NULL DEFAULT '',
            lower_code TEXT NOT NULL DEFAULT '',
            name TEXT NOT NULL,
            lower_name TEXT NOT NULL,
            saved_at TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            UNIQUE(owner_key, lower_code)
        );
        CREATE INDEX IF NOT EXISTS idx_calculate_order_templates_owner_saved
            ON calculate_order_templates(owner_key, saved_at DESC);
        CREATE INDEX IF NOT EXISTS idx_calculate_order_templates_owner_name
            ON calculate_order_templates(owner_key, lower_name);",
    )?;
    ensure_code_columns(conn)?;
    rebuild_with_code_unique(conn)
}

fn ensure_code_columns(conn: &Connection) -> rusqlite::Result<()> {
    let has_code: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM pragma_table_info('calculate_order_templates')
         WHERE name = 'code'",
        [],
        |row| row.get(0),
    )?;
    if has_code > 0 {
        return Ok(());
    }

    conn.execute_batch(
        "ALTER TABLE calculate_order_templates ADD COLUMN code TEXT NOT NULL DEFAULT '';
         ALTER TABLE calculate_order_templates ADD COLUMN lower_code TEXT NOT NULL DEFAULT '';
         UPDATE calculate_order_templates
            SET code = 'Z-' || id,
                lower_code = lower('Z-' || id)
          WHERE trim(code) = '';
         CREATE UNIQUE INDEX IF NOT EXISTS idx_calculate_order_templates_owner_code
            ON calculate_order_templates(owner_key, lower_code);",
    )?;

    rebuild_without_name_unique(conn)
}

fn rebuild_with_code_unique(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS calculate_order_templates_next;
        CREATE TABLE calculate_order_templates_next (
            id TEXT PRIMARY KEY,
            owner_key TEXT NOT NULL,
            code TEXT NOT NULL,
            lower_code TEXT NOT NULL,
            name TEXT NOT NULL,
            lower_name TEXT NOT NULL,
            saved_at TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            UNIQUE(owner_key, lower_code)
        );
        INSERT OR IGNORE INTO calculate_order_templates_next
            (id, owner_key, code, lower_code, name, lower_name, saved_at, payload_json)
        SELECT
            id,
            owner_key,
            CASE
                WHEN trim(code) != '' THEN trim(code)
                ELSE 'Z-' || id
            END,
            lower(
                CASE
                    WHEN trim(code) != '' THEN trim(code)
                    ELSE 'Z-' || id
                END
            ),
            name,
            lower_name,
            saved_at,
            payload_json
        FROM calculate_order_templates
        ORDER BY saved_at DESC, id DESC;
        DROP TABLE calculate_order_templates;
        ALTER TABLE calculate_order_templates_next RENAME TO calculate_order_templates;
        CREATE INDEX IF NOT EXISTS idx_calculate_order_templates_owner_saved
            ON calculate_order_templates(owner_key, saved_at DESC);
        CREATE INDEX IF NOT EXISTS idx_calculate_order_templates_owner_name
            ON calculate_order_templates(owner_key, lower_name);",
    )
}

fn rebuild_without_name_unique(conn: &Connection) -> rusqlite::Result<()> {
    let uses_name_unique: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM sqlite_master
         WHERE type = 'table'
           AND name = 'calculate_order_templates'
           AND sql LIKE '%UNIQUE(owner_key, lower_name)%'",
        [],
        |row| row.get(0),
    )?;
    if uses_name_unique == 0 {
        return Ok(());
    }

    conn.execute_batch(
        "CREATE TABLE calculate_order_templates_next (
            id TEXT PRIMARY KEY,
            owner_key TEXT NOT NULL,
            code TEXT NOT NULL,
            lower_code TEXT NOT NULL,
            name TEXT NOT NULL,
            lower_name TEXT NOT NULL,
            saved_at TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            UNIQUE(owner_key, lower_code)
        );
        INSERT INTO calculate_order_templates_next
            (id, owner_key, code, lower_code, name, lower_name, saved_at, payload_json)
        SELECT
            id,
            owner_key,
            CASE
                WHEN trim(code) != '' THEN trim(code)
                ELSE 'Z-' || id
            END,
            lower(
                CASE
                    WHEN trim(code) != '' THEN trim(code)
                    ELSE 'Z-' || id
                END
            ),
            name,
            lower_name,
            saved_at,
            payload_json
        FROM calculate_order_templates;
        DROP TABLE calculate_order_templates;
        ALTER TABLE calculate_order_templates_next RENAME TO calculate_order_templates;
        CREATE INDEX IF NOT EXISTS idx_calculate_order_templates_owner_saved
            ON calculate_order_templates(owner_key, saved_at DESC);
        CREATE INDEX IF NOT EXISTS idx_calculate_order_templates_owner_name
            ON calculate_order_templates(owner_key, lower_name);",
    )
}

fn dedupe_templates(templates: Vec<CalculateOrderTemplate>) -> Vec<CalculateOrderTemplate> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::with_capacity(templates.len());
    for template in templates {
        let key = quick_template_key(&template);
        if key == "id:" || seen.insert(key) {
            result.push(template);
        }
    }
    result
}

fn quick_template_key(template: &CalculateOrderTemplate) -> String {
    let product_key = [
        template.item_code.as_str(),
        template.product.as_str(),
        template.name.as_str(),
    ]
    .into_iter()
    .map(normalize_key)
    .find(|value| !value.is_empty())
    .unwrap_or_default();
    if product_key.is_empty() {
        return legacy_template_key(template);
    }
    [
        "quick".to_string(),
        normalize_key(&template.customer_ref),
        normalize_key(&template.customer),
        product_key,
        normalize_key(&template.status),
        normalize_key(&template.material_display),
        normalize_key(&template.color),
        number_key(template.width_mm),
        number_key(template.waste_percent),
        option_number_key(template.roll_count),
        normalize_key(&template.first_layer_material),
        normalize_key(&template.first_layer_micron),
        normalize_key(&template.second_layer_material),
        normalize_key(&template.second_layer_micron),
        normalize_key(&template.third_layer_material),
        normalize_key(&template.third_layer_micron),
        normalize_key(&template.note),
    ]
    .join("|")
}

fn legacy_template_key(template: &CalculateOrderTemplate) -> String {
    let code = normalize_key(&template.code);
    if code.is_empty() {
        format!("id:{}", template.id.trim())
    } else {
        format!("code:{code}")
    }
}

fn normalize_key(value: &str) -> String {
    value.trim().to_lowercase()
}

fn number_key(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.3}")
    } else {
        String::new()
    }
}

fn option_number_key(value: Option<f64>) -> String {
    value.map(number_key).unwrap_or_default()
}

fn new_id() -> String {
    unix_micros().to_string()
}

fn unix_micros() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn calculate_order_sqlite_store_round_trips_and_upserts_templates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("orders.sqlite");
        let store = CalculateOrderStore::new(path.clone());

        let saved = store
            .upsert(
                "admin:admin",
                CalculateOrderTemplate {
                    id: String::new(),
                    code: "Z-CPP-600".to_string(),
                    name: "CPP 600".to_string(),
                    saved_at: String::new(),
                    order_number: "ORD-1".to_string(),
                    customer_ref: "CUST-001".to_string(),
                    customer: "Mijoz".to_string(),
                    item_code: "ITEM-001".to_string(),
                    product: "cpp / 20 mikron / 600".to_string(),
                    status: String::new(),
                    material_display: String::new(),
                    color: String::new(),
                    image_id: "img-1".to_string(),
                    image_name: "rang.jpg".to_string(),
                    image_mime: "image/jpeg".to_string(),
                    image_size_bytes: 3,
                    image_url: "/v1/mobile/calculate/orders/image/view?id=img-1".to_string(),
                    width_mm: 530.0,
                    waste_percent: 3.0,
                    roll_count: Some(7.0),
                    first_layer_material: "pet".to_string(),
                    first_layer_micron: "12".to_string(),
                    second_layer_material: "pe oq".to_string(),
                    second_layer_micron: "30".to_string(),
                    third_layer_material: String::new(),
                    third_layer_micron: String::new(),
                    note: String::new(),
                    kg: 0.0,
                    source_map_id: String::new(),
                },
            )
            .await
            .expect("save");
        let updated = store
            .upsert(
                "admin:admin",
                CalculateOrderTemplate {
                    width_mm: 630.0,
                    ..saved.clone()
                },
            )
            .await
            .expect("update");

        assert_eq!(updated.id, saved.id);
        assert_eq!(updated.width_mm, 630.0);

        drop(store);
        let conn = rusqlite::Connection::open(&path).expect("open sqlite");
        let row_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM calculate_order_templates",
                [],
                |row| row.get(0),
            )
            .expect("row count");
        assert_eq!(row_count, 1);
        drop(conn);

        let reloaded = CalculateOrderStore::new(path);
        let rows = reloaded.list("admin:admin").await.expect("list");

        assert_eq!(rows, vec![updated.clone()]);
        assert!(
            serde_json::to_value(&rows[0])
                .expect("json")
                .get("kg")
                .is_none()
        );

        reloaded
            .delete("admin:admin", &updated.id)
            .await
            .expect("delete");
        assert!(reloaded.list("admin:admin").await.expect("list").is_empty());
    }

    #[tokio::test]
    async fn calculate_order_sqlite_store_dedupes_same_quick_template_across_order_codes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = CalculateOrderStore::new(dir.path().join("orders.sqlite"));

        let base = CalculateOrderTemplate {
            id: String::new(),
            code: "1111".to_string(),
            name: "Qurt".to_string(),
            saved_at: String::new(),
            order_number: "1111".to_string(),
            customer_ref: String::new(),
            customer: String::new(),
            item_code: "QURT-001".to_string(),
            product: "Qurt".to_string(),
            status: String::new(),
            material_display: String::new(),
            color: String::new(),
            image_id: String::new(),
            image_name: String::new(),
            image_mime: String::new(),
            image_size_bytes: 0,
            image_url: String::new(),
            width_mm: 530.0,
            waste_percent: 5.0,
            roll_count: Some(7.0),
            first_layer_material: "pet".to_string(),
            first_layer_micron: "12".to_string(),
            second_layer_material: "pe oq".to_string(),
            second_layer_micron: "30".to_string(),
            third_layer_material: String::new(),
            third_layer_micron: String::new(),
            note: String::new(),
            kg: 500.0,
            source_map_id: "zakaz-1111".to_string(),
        };

        let first = store
            .upsert("admin:admin", base.clone())
            .await
            .expect("first save");
        let duplicate = CalculateOrderTemplate {
            code: "2222".to_string(),
            order_number: "2222".to_string(),
            kg: 900.0,
            source_map_id: "zakaz-2222".to_string(),
            ..base
        };
        let second = store
            .upsert("admin:admin", duplicate)
            .await
            .expect("second save");

        assert_ne!(first.code, second.code);
        assert_eq!(first.name, "Qurt");
        assert_eq!(second.name, "Qurt");
        let rows = store.list("admin:admin").await.expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].code, second.code);

        let updated = store
            .upsert(
                "admin:admin",
                CalculateOrderTemplate {
                    width_mm: 640.0,
                    ..second.clone()
                },
            )
            .await
            .expect("update second");
        assert_eq!(updated.id, second.id);
        assert_eq!(updated.width_mm, 640.0);
        assert_eq!(store.list("admin:admin").await.expect("list").len(), 2);
    }

    #[tokio::test]
    async fn calculate_order_sqlite_store_dedupes_legacy_same_code_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("orders.sqlite");
        let conn = Connection::open(&path).expect("open sqlite");
        conn.execute_batch(
            "CREATE TABLE calculate_order_templates (
                id TEXT PRIMARY KEY,
                owner_key TEXT NOT NULL,
                code TEXT NOT NULL,
                lower_code TEXT NOT NULL,
                name TEXT NOT NULL,
                lower_name TEXT NOT NULL,
                saved_at TEXT NOT NULL,
                payload_json TEXT NOT NULL
            );",
        )
        .expect("legacy schema");

        let old = CalculateOrderTemplate {
            id: "old-id".to_string(),
            code: "Z-DUP-1".to_string(),
            name: "Old duplicate".to_string(),
            saved_at: "100".to_string(),
            order_number: String::new(),
            customer_ref: String::new(),
            customer: String::new(),
            item_code: "ITEM-001".to_string(),
            product: "Qurt".to_string(),
            status: String::new(),
            material_display: String::new(),
            color: String::new(),
            image_id: String::new(),
            image_name: String::new(),
            image_mime: String::new(),
            image_size_bytes: 0,
            image_url: String::new(),
            width_mm: 530.0,
            waste_percent: 5.0,
            roll_count: Some(7.0),
            first_layer_material: "pet".to_string(),
            first_layer_micron: "12".to_string(),
            second_layer_material: "pe oq".to_string(),
            second_layer_micron: "30".to_string(),
            third_layer_material: String::new(),
            third_layer_micron: String::new(),
            note: String::new(),
            kg: 0.0,
            source_map_id: String::new(),
        };
        let newer = CalculateOrderTemplate {
            id: "new-id".to_string(),
            name: "New duplicate".to_string(),
            saved_at: "200".to_string(),
            width_mm: 640.0,
            ..old.clone()
        };
        for template in [&old, &newer] {
            conn.execute(
                "INSERT INTO calculate_order_templates
                    (id, owner_key, code, lower_code, name, lower_name, saved_at, payload_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    template.id,
                    "admin:admin",
                    template.code,
                    template.code.to_lowercase(),
                    template.name,
                    template.name.to_lowercase(),
                    template.saved_at,
                    serde_json::to_string(template).expect("json"),
                ],
            )
            .expect("insert duplicate");
        }
        drop(conn);

        let store = CalculateOrderStore::new(path.clone());
        let rows = store.list("admin:admin").await.expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "new-id");
        assert_eq!(rows[0].width_mm, 640.0);

        let conn = Connection::open(path).expect("reopen sqlite");
        let row_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM calculate_order_templates",
                [],
                |row| row.get(0),
            )
            .expect("row count");
        assert_eq!(row_count, 1);
    }
}
