-- One Astatka business submission owns the output, paused session, source
-- consumption and every remaining input label. Immutable replay survives a
-- lost HTTP response without another production report or inventory return.
CREATE TABLE mini_lamination_astatka_operations (
    operation_id TEXT PRIMARY KEY,
    apparatus TEXT NOT NULL,
    order_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    recorded_by_ref TEXT NOT NULL,
    request_id TEXT NOT NULL CHECK (length(request_id) BETWEEN 8 AND 128),
    report_id TEXT NOT NULL,
    request_json JSONB NOT NULL,
    response_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (recorded_by_ref, request_id),
    CHECK (btrim(operation_id)<>'' AND btrim(apparatus)<>'' AND btrim(order_id)<>''
        AND btrim(session_id)<>'' AND btrim(recorded_by_ref)<>'' AND btrim(report_id)<>'')
);
CREATE INDEX mini_lamination_astatka_order_history
    ON mini_lamination_astatka_operations(order_id,apparatus,created_at DESC);
CREATE TRIGGER mini_lamination_astatka_operations_immutable
    BEFORE UPDATE OR DELETE ON mini_lamination_astatka_operations
    FOR EACH ROW EXECUTE FUNCTION mini_raw_material_events_block_mutation();
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='mini_rs_erp') THEN
    REVOKE UPDATE,DELETE ON mini_lamination_astatka_operations FROM mini_rs_erp;
    GRANT SELECT,INSERT ON mini_lamination_astatka_operations TO mini_rs_erp;
 END IF;
END $$;
