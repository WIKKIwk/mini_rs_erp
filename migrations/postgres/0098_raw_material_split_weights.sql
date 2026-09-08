SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- Historical splits did not record gross/core weights; keep these unknown.
ALTER TABLE mini_raw_material_split_outputs
    ADD COLUMN gross_kg NUMERIC(18,6),
    ADD COLUMN bobina_kg NUMERIC(18,6),
    ADD CONSTRAINT mini_raw_material_split_output_weights CHECK (
        (gross_kg IS NULL AND bobina_kg IS NULL)
        OR (gross_kg IS NOT NULL AND bobina_kg IS NOT NULL
            AND gross_kg <> 'NaN'::numeric AND bobina_kg <> 'NaN'::numeric
            AND bobina_kg >= 0 AND gross_kg > bobina_kg
            AND kg = gross_kg - bobina_kg)
    );
