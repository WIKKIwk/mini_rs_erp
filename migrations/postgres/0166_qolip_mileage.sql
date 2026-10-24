-- Tracking begins only at an effective peredelka return. Never backfill old WIP.
CREATE TABLE public.mini_qolip_mileage_cycles (
    id TEXT PRIMARY KEY,
    qolip_code TEXT NOT NULL CHECK (btrim(qolip_code) <> ''),
    qolip_code_key TEXT GENERATED ALWAYS AS (lower(btrim(qolip_code))) STORED,
    rework_id TEXT NOT NULL CHECK (btrim(rework_id) <> ''),
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ,
    meter NUMERIC(18,6) NOT NULL DEFAULT 0 CHECK (meter >= 0),
    CONSTRAINT mini_qolip_mileage_cycle_interval CHECK (ended_at IS NULL OR ended_at >= started_at),
    UNIQUE (rework_id, qolip_code_key),
    UNIQUE (id, qolip_code_key)
);
CREATE UNIQUE INDEX idx_mini_qolip_mileage_current
    ON public.mini_qolip_mileage_cycles(qolip_code_key) WHERE ended_at IS NULL;
CREATE INDEX idx_mini_qolip_mileage_history
    ON public.mini_qolip_mileage_cycles(qolip_code_key, started_at DESC);

-- A nullable cycle freezes the fact that tracking was off at this actual work.
-- The physical source, cycle and interval never change on reprint/correction.
CREATE TABLE public.mini_qolip_mileage_contributions (
    batch_id TEXT NOT NULL CHECK (btrim(batch_id) <> ''),
    qolip_code_key TEXT NOT NULL CHECK (qolip_code_key = lower(btrim(qolip_code_key)) AND qolip_code_key <> ''),
    cycle_id TEXT,
    session_id TEXT NOT NULL,
    canonical_apparatus_id TEXT NOT NULL,
    production_started_at TIMESTAMPTZ NOT NULL,
    production_completed_at TIMESTAMPTZ NOT NULL,
    meter NUMERIC(18,6) NOT NULL CHECK (meter >= 0),
    PRIMARY KEY (batch_id, qolip_code_key),
    FOREIGN KEY (cycle_id, qolip_code_key)
        REFERENCES public.mini_qolip_mileage_cycles(id, qolip_code_key),
    CHECK (production_completed_at >= production_started_at)
);
CREATE INDEX idx_mini_qolip_mileage_contribution_cycle
    ON public.mini_qolip_mileage_contributions(cycle_id) WHERE cycle_id IS NOT NULL;

ALTER TABLE public.mini_qolip_mileage_cycles OWNER TO mini_rs_erp_owner;
ALTER TABLE public.mini_qolip_mileage_contributions OWNER TO mini_rs_erp_owner;
GRANT SELECT, INSERT, UPDATE ON public.mini_qolip_mileage_cycles TO mini_rs_erp;
GRANT SELECT, INSERT, UPDATE ON public.mini_qolip_mileage_contributions TO mini_rs_erp;

-- The existing editable catalog code is this feature's physical identity.
-- Preserve its cycle/source history instead of silently creating a fresh mold.
CREATE FUNCTION public.mini_qolip_preserve_mileage_identity() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND lower(btrim(NEW.qolip_code)) = lower(btrim(OLD.qolip_code)) THEN
        RETURN NEW;
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('qolip:' || lower(btrim(OLD.qolip_code)), 0));
    IF EXISTS (SELECT 1 FROM public.mini_qolip_mileage_cycles
               WHERE qolip_code_key = lower(btrim(OLD.qolip_code))) THEN
        RAISE EXCEPTION 'qolip_history_locked' USING ERRCODE = '23514',
            CONSTRAINT = 'mini_qolip_mileage_identity_locked';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END;
$$;
ALTER FUNCTION public.mini_qolip_preserve_mileage_identity() OWNER TO mini_rs_erp_owner;
CREATE TRIGGER mini_qolip_preserve_mileage_identity
    BEFORE DELETE OR UPDATE OF qolip_code ON public.mini_qolip_product_specs
    FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_preserve_mileage_identity();
