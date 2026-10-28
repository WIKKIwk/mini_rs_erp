SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

CREATE SEQUENCE public.mini_preparation_formula_code_seq AS BIGINT
    MINVALUE 1 MAXVALUE 99999999 NO CYCLE;
ALTER SEQUENCE public.mini_preparation_formula_code_seq OWNER TO mini_rs_erp_owner;
REVOKE ALL ON SEQUENCE public.mini_preparation_formula_code_seq FROM PUBLIC, mini_rs_erp;

CREATE FUNCTION public.mini_preparation_next_formula_code() RETURNS TEXT
LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
    SELECT '9503' || lpad(nextval('public.mini_preparation_formula_code_seq')::TEXT, 8, '0')
$$;
ALTER FUNCTION public.mini_preparation_next_formula_code() OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_preparation_next_formula_code() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.mini_preparation_next_formula_code() TO mini_rs_erp;

-- The volatile default assigns distinct codes to historical and new formulas.
-- An upsert keeps the existing code; deleted codes are never reused.
ALTER TABLE public.mini_preparation_formulas
    ADD COLUMN formula_code TEXT NOT NULL DEFAULT public.mini_preparation_next_formula_code()
        CHECK (formula_code ~ '^9503[0-9]{8}$'),
    ADD CONSTRAINT mini_preparation_formula_code_unique UNIQUE (formula_code);
CREATE FUNCTION public.mini_preparation_preserve_formula_code() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    IF NEW.formula_code IS DISTINCT FROM OLD.formula_code THEN
        RAISE EXCEPTION 'preparation_formula_code_immutable' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END
$$;
ALTER FUNCTION public.mini_preparation_preserve_formula_code() OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_preparation_preserve_formula_code() FROM PUBLIC, mini_rs_erp;
CREATE TRIGGER mini_preparation_preserve_formula_code
    BEFORE UPDATE OF formula_code ON public.mini_preparation_formulas
    FOR EACH ROW EXECUTE FUNCTION public.mini_preparation_preserve_formula_code();
