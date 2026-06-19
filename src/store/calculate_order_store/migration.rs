use std::time::Duration;

use rusqlite::Connection;

pub(super) fn configure_connection(conn: &Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

pub(super) fn migrate(conn: &Connection) -> rusqlite::Result<()> {
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
            ON calculate_order_templates(owner_key, lower_name);
        CREATE TABLE IF NOT EXISTS calculate_order_images (
            owner_key TEXT NOT NULL,
            image_id TEXT NOT NULL,
            image_name TEXT NOT NULL,
            image_mime TEXT NOT NULL,
            image_size_bytes INTEGER NOT NULL,
            body BLOB NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY(owner_key, image_id)
        );",
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
