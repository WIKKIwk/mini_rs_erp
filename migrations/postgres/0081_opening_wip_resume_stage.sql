ALTER TABLE mini_opening_wip_intakes
    ADD COLUMN resume_apparatus TEXT,
    ADD COLUMN resume_stage_node_id TEXT;

WITH candidates AS (
    SELECT intake.intake_id,
           node.node_id,
           COALESCE(
               node.canonical_alternative_apparatus_id,
               node.canonical_apparatus_id
           ) AS apparatus_id,
           count(*) OVER (PARTITION BY intake.intake_id) AS candidate_count
    FROM mini_opening_wip_intakes AS intake
    JOIN mini_production_map_nodes AS node
      ON node.map_id = intake.order_id
     AND node.kind = 'apparatus'
     AND (
         COALESCE(
             node.canonical_alternative_apparatus_id,
             node.canonical_apparatus_id
         ) = btrim(intake.current_location)
         OR btrim(
             CASE
                 WHEN btrim(COALESCE(node.payload_json->>'alternative_assigned_title', '')) <> ''
                     THEN node.payload_json->>'alternative_assigned_title'
                 ELSE node.title
             END
         ) = btrim(intake.current_location)
     )
)
UPDATE mini_opening_wip_intakes AS intake
SET resume_apparatus = candidates.apparatus_id,
    resume_stage_node_id = candidates.node_id
FROM candidates
WHERE candidates.intake_id = intake.intake_id
  AND candidates.candidate_count = 1;

DO $$
DECLARE
    unresolved BIGINT;
BEGIN
    SELECT count(*)
    INTO unresolved
    FROM mini_opening_wip_intakes
    WHERE resume_apparatus IS NULL
       OR resume_stage_node_id IS NULL;

    IF unresolved > 0 THEN
        RAISE EXCEPTION
            '0081 Opening WIP resume-stage backfill failed for % intake(s): current_location is missing or ambiguous in the order production map',
            unresolved;
    END IF;
END
$$;

ALTER TABLE mini_opening_wip_intakes
    ALTER COLUMN resume_apparatus SET NOT NULL,
    ALTER COLUMN resume_stage_node_id SET NOT NULL,
    ADD CONSTRAINT mini_opening_wip_intakes_resume_apparatus_not_blank
        CHECK (btrim(resume_apparatus) <> ''),
    ADD CONSTRAINT mini_opening_wip_intakes_resume_stage_node_not_blank
        CHECK (btrim(resume_stage_node_id) <> ''),
    ADD CONSTRAINT mini_opening_wip_intakes_resume_apparatus_fk
        FOREIGN KEY (resume_apparatus) REFERENCES mini_apparatus(id) ON DELETE RESTRICT;

CREATE INDEX idx_mini_opening_wip_intakes_resume_apparatus_order
    ON mini_opening_wip_intakes (resume_apparatus, order_id, created_at DESC);
