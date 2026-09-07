-- A receipt is retained on the physical pallet for safe scan/retry and audit.
ALTER TABLE mini_paddons ADD COLUMN IF NOT EXISTS receipt_json JSONB;
