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
        "CREATE TABLE IF NOT EXISTS production_maps (
            id TEXT PRIMARY KEY,
            product_code TEXT NOT NULL,
            title TEXT NOT NULL,
            saved_at TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_production_maps_saved
            ON production_maps(saved_at DESC);
        CREATE INDEX IF NOT EXISTS idx_production_maps_product_code
            ON production_maps(product_code);
        CREATE TABLE IF NOT EXISTS apparatus_sequences (
            apparatus TEXT PRIMARY KEY,
            order_ids_json TEXT NOT NULL,
            saved_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS apparatus_queue_states (
            apparatus TEXT NOT NULL,
            order_id TEXT NOT NULL,
            state TEXT NOT NULL,
            saved_at TEXT NOT NULL,
            PRIMARY KEY (apparatus, order_id)
        );
        CREATE TABLE IF NOT EXISTS apparatus_queue_policies (
            apparatus TEXT PRIMARY KEY,
            policy TEXT NOT NULL,
            actor_role TEXT NOT NULL DEFAULT '',
            actor_ref TEXT NOT NULL DEFAULT '',
            actor_display_name TEXT NOT NULL DEFAULT '',
            payload_json TEXT NOT NULL DEFAULT '{}',
            saved_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS apparatus_queue_action_events (
            event_id TEXT PRIMARY KEY,
            apparatus TEXT NOT NULL,
            order_id TEXT NOT NULL,
            action TEXT NOT NULL,
            from_state TEXT NOT NULL,
            to_state TEXT NOT NULL,
            policy TEXT NOT NULL,
            actor_role TEXT NOT NULL DEFAULT '',
            actor_ref TEXT NOT NULL DEFAULT '',
            actor_display_name TEXT NOT NULL DEFAULT '',
            assigned_apparatus_json TEXT NOT NULL DEFAULT '[]',
            payload_json TEXT NOT NULL DEFAULT '{}',
            saved_at TEXT NOT NULL
        );",
    )
}
