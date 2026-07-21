CREATE INDEX IF NOT EXISTS idx_mini_qolip_locations_qolip_code
    ON mini_qolip_locations (lower(qolip_code), updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_mini_qolip_checkouts_qolip_code_status
    ON mini_qolip_checkouts (lower(qolip_code), lower(status), updated_at DESC);
