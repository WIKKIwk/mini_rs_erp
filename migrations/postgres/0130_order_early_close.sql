ALTER TABLE mini_order_control_states ADD COLUMN early_close JSONB;

ALTER TABLE mini_order_control_states ADD CONSTRAINT mini_order_early_close_valid CHECK (
    early_close IS NULL OR (
        jsonb_typeof(early_close) = 'object'
        AND length(btrim(early_close->>'comment')) BETWEEN 1 AND 2000
        AND state IN ('freeze_requested', 'frozen')
        AND ((early_close->>'closed_at_unix') IS NULL) = (state = 'freeze_requested')
    )
);

-- A stale request must never erase/change the reason or reopen a closed order.
CREATE FUNCTION mini_preserve_order_early_close() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.early_close IS NOT NULL AND (
        NEW.early_close IS NULL
        OR (NEW.early_close - 'closed_at_unix') IS DISTINCT FROM (OLD.early_close - 'closed_at_unix')
        OR ((OLD.early_close->>'closed_at_unix') IS NOT NULL AND NEW.early_close IS DISTINCT FROM OLD.early_close)
    ) THEN
        RAISE EXCEPTION 'order early closure is immutable';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER mini_preserve_order_early_close BEFORE UPDATE ON mini_order_control_states
FOR EACH ROW EXECUTE FUNCTION mini_preserve_order_early_close();
