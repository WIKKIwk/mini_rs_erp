-- Match the normalized exact-code lookup without joining or scanning mold rows.
-- Keep this non-unique: existing catalogs may contain case variants of a code.
CREATE INDEX IF NOT EXISTS idx_mini_items_normalized_code
    ON mini_items (lower(btrim(code)));
