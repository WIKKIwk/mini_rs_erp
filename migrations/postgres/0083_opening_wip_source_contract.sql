ALTER TABLE mini_opening_wip_intakes
    DROP CONSTRAINT mini_opening_wip_intakes_location_not_blank,
    DROP CONSTRAINT mini_opening_wip_intakes_resume_apparatus_not_blank,
    ALTER COLUMN resume_apparatus DROP NOT NULL;

ALTER TABLE mini_opening_wip_intakes
    ADD CONSTRAINT mini_opening_wip_intakes_contract_shape CHECK (
        (
            btrim(source_apparatus) = ''
            AND btrim(current_location) <> ''
            AND resume_apparatus IS NOT NULL
            AND btrim(resume_apparatus) <> ''
            AND btrim(resume_stage_node_id) <> ''
        )
        OR
        (
            btrim(source_apparatus) <> ''
            AND btrim(current_location) = ''
            AND resume_apparatus IS NULL
            AND btrim(resume_stage_node_id) <> ''
        )
    );
