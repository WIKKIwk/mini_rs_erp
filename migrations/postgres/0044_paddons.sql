CREATE TABLE IF NOT EXISTS mini_paddons (
    id TEXT PRIMARY KEY,
    code TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'open',
    location TEXT NOT NULL DEFAULT '',
    note TEXT NOT NULL DEFAULT '',
    created_by_ref TEXT NOT NULL DEFAULT '',
    created_by_display_name TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    closed_at TIMESTAMPTZ,
    CONSTRAINT mini_paddons_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT mini_paddons_code_not_blank CHECK (btrim(code) <> ''),
    CONSTRAINT mini_paddons_status_allowed CHECK (status IN ('open', 'closed'))
);

CREATE TABLE IF NOT EXISTS mini_paddon_items (
    id TEXT PRIMARY KEY,
    paddon_id TEXT NOT NULL REFERENCES mini_paddons(id) ON DELETE CASCADE,
    progress_batch_id TEXT NOT NULL REFERENCES mini_progress_batches(batch_id) ON DELETE RESTRICT,
    added_by_ref TEXT NOT NULL DEFAULT '',
    added_by_display_name TEXT NOT NULL DEFAULT '',
    added_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    removed_by_ref TEXT NOT NULL DEFAULT '',
    removed_by_display_name TEXT NOT NULL DEFAULT '',
    removed_at TIMESTAMPTZ,
    CONSTRAINT mini_paddon_items_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT mini_paddon_items_batch_not_blank CHECK (btrim(progress_batch_id) <> '')
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_paddon_items_active_batch
    ON mini_paddon_items(progress_batch_id)
    WHERE removed_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_mini_paddons_updated
    ON mini_paddons(updated_at DESC, code ASC);

CREATE INDEX IF NOT EXISTS idx_mini_paddon_items_paddon_active
    ON mini_paddon_items(paddon_id, added_at DESC)
    WHERE removed_at IS NULL;
