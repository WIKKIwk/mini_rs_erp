use std::time::Duration;

use rusqlite::Connection;

use crate::core::apparatus_standard::ApparatusId;

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
            canonical_apparatus_id TEXT,
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
        );
        CREATE TABLE IF NOT EXISTS apparatus_capacity_profiles (
            apparatus_id TEXT PRIMARY KEY,
            apparatus TEXT NOT NULL,
            capacity_slots INTEGER NOT NULL,
            setup_minutes INTEGER NOT NULL,
            cleanup_minutes INTEGER NOT NULL,
            efficiency_percent INTEGER NOT NULL,
            finite_capacity INTEGER NOT NULL,
            working_windows_json TEXT NOT NULL,
            capabilities_json TEXT NOT NULL,
            capability_levels_json TEXT NOT NULL,
            notes TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS apparatus_downtimes (
            id TEXT PRIMARY KEY,
            apparatus_id TEXT NOT NULL,
            apparatus TEXT NOT NULL,
            starts_at_unix INTEGER NOT NULL,
            ends_at_unix INTEGER NOT NULL,
            reason TEXT NOT NULL,
            active INTEGER NOT NULL,
            actor_json TEXT NOT NULL,
            created_at_unix INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS apparatus_schedule_reservations (
            reservation_id TEXT PRIMARY KEY,
            idempotency_key TEXT NOT NULL UNIQUE,
            order_id TEXT NOT NULL,
            apparatus_id TEXT NOT NULL,
            apparatus TEXT NOT NULL,
            starts_at_unix INTEGER NOT NULL,
            ends_at_unix INTEGER NOT NULL,
            requested_duration_minutes INTEGER NOT NULL,
            reserved_duration_minutes INTEGER NOT NULL,
            status TEXT NOT NULL,
            priority INTEGER NOT NULL,
            source TEXT NOT NULL,
            reason TEXT NOT NULL,
            capability_requirements_json TEXT NOT NULL,
            actor_json TEXT NOT NULL,
            created_at_unix INTEGER NOT NULL
        );",
    )?;

    let has_canonical_apparatus_id = {
        let mut statement = conn.prepare("PRAGMA table_info(apparatus_queue_policies)")?;
        statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .any(|name| name == "canonical_apparatus_id")
    };
    if !has_canonical_apparatus_id {
        conn.execute(
            "ALTER TABLE apparatus_queue_policies ADD COLUMN canonical_apparatus_id TEXT",
            [],
        )?;
    }
    let legacy_rows = {
        let mut statement = conn.prepare(
            "SELECT rowid, apparatus FROM apparatus_queue_policies
             WHERE canonical_apparatus_id IS NULL",
        )?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (row_id, apparatus) in legacy_rows {
        let Ok(apparatus_id) = ApparatusId::new(apparatus) else {
            continue;
        };
        let already_present = conn.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM apparatus_queue_policies
                 WHERE canonical_apparatus_id = ?1
             )",
            rusqlite::params![apparatus_id.as_str()],
            |row| row.get::<_, bool>(0),
        )?;
        if already_present {
            continue;
        }
        conn.execute(
            "UPDATE apparatus_queue_policies
             SET canonical_apparatus_id = ?1
             WHERE rowid = ?2 AND canonical_apparatus_id IS NULL",
            rusqlite::params![apparatus_id.as_str(), row_id],
        )?;
    }
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_apparatus_queue_policies_canonical_id
         ON apparatus_queue_policies(canonical_apparatus_id)",
        [],
    )?;
    Ok(())
}
