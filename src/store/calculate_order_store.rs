use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};

use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderImage, CalculateOrderStorePort, CalculateOrderTemplate,
    validate_template,
};

mod migration;
mod template_helpers;
#[cfg(test)]
mod tests;

use self::migration::{configure_connection, migrate};
use self::template_helpers::{
    dedupe_templates, existing_id_by_code, new_id, normalize_key, stamp_image, stamp_template,
    unix_micros,
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

    async fn save_image(
        &self,
        owner_key: &str,
        image: CalculateOrderImage,
    ) -> Result<CalculateOrderImage, CalculateOrderError> {
        let saved = stamp_image(image)?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| CalculateOrderError::StoreFailed)?;
        conn.execute(
            "INSERT INTO calculate_order_images
                (owner_key, image_id, image_name, image_mime, image_size_bytes, body, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(owner_key, image_id) DO UPDATE SET
                image_name = excluded.image_name,
                image_mime = excluded.image_mime,
                image_size_bytes = excluded.image_size_bytes,
                body = excluded.body",
            params![
                owner_key.trim(),
                &saved.image_id,
                &saved.image_name,
                &saved.image_mime,
                saved.image_size_bytes as i64,
                &saved.body,
                unix_micros().to_string()
            ],
        )
        .map_err(|_| CalculateOrderError::StoreFailed)?;
        Ok(saved)
    }

    async fn get_image(
        &self,
        owner_key: &str,
        image_id: &str,
    ) -> Result<Option<CalculateOrderImage>, CalculateOrderError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CalculateOrderError::StoreFailed)?;
        conn.query_row(
            "SELECT image_id, image_name, image_mime, image_size_bytes, body
             FROM calculate_order_images
             WHERE owner_key = ?1 AND image_id = ?2",
            params![owner_key.trim(), image_id.trim()],
            |row| {
                let size: i64 = row.get(3)?;
                Ok(CalculateOrderImage {
                    image_id: row.get(0)?,
                    image_name: row.get(1)?,
                    image_mime: row.get(2)?,
                    image_size_bytes: size.max(0) as u64,
                    body: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|_| CalculateOrderError::StoreFailed)
    }
}
