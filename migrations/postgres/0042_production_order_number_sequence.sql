CREATE SEQUENCE IF NOT EXISTS mini_production_order_number_seq
    MINVALUE 1
    MAXVALUE 9999
    START WITH 1
    INCREMENT BY 1
    NO CYCLE;

DO $$
DECLARE
    current_max INTEGER;
BEGIN
    SELECT MAX(
        CASE
            WHEN btrim(order_number) ~ '^[0-9]{1,4}$'
            THEN btrim(order_number)::INTEGER
        END
    )
    INTO current_max
    FROM mini_production_maps;

    IF current_max IS NULL OR current_max < 1 THEN
        PERFORM setval('mini_production_order_number_seq', 1, false);
    ELSE
        PERFORM setval('mini_production_order_number_seq', current_max, true);
    END IF;
END
$$;

GRANT USAGE, SELECT
    ON SEQUENCE mini_production_order_number_seq TO mini_rs_erp;
