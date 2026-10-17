-- The immutable receipt and accounting report are committed together. A lost
-- response may be retried after the execution has closed or been reactivated.
CREATE TABLE mini_rezka_astatka_operations (
    actor_role TEXT NOT NULL,
    actor_ref TEXT NOT NULL CHECK (btrim(actor_ref) <> ''),
    request_id TEXT NOT NULL CHECK (length(request_id) BETWEEN 8 AND 128),
    order_id TEXT NOT NULL CHECK (btrim(order_id) <> ''),
    apparatus TEXT NOT NULL CHECK (btrim(apparatus) <> ''),
    session_id TEXT NOT NULL CHECK (btrim(session_id) <> ''),
    report_id TEXT NOT NULL CHECK (btrim(report_id) <> ''),
    request_json JSONB NOT NULL,
    report_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (actor_role, actor_ref, request_id)
);
CREATE INDEX mini_rezka_astatka_operations_order_idx
    ON mini_rezka_astatka_operations(order_id, apparatus, created_at DESC);
CREATE TRIGGER mini_rezka_astatka_operations_immutable
    BEFORE UPDATE OR DELETE ON mini_rezka_astatka_operations
    FOR EACH ROW EXECUTE FUNCTION mini_raw_material_events_block_mutation();
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='mini_rs_erp') THEN
    REVOKE UPDATE, DELETE ON mini_rezka_astatka_operations FROM mini_rs_erp;
    GRANT SELECT, INSERT ON mini_rezka_astatka_operations TO mini_rs_erp;
 END IF;
END $$;
