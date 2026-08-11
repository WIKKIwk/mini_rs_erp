ALTER TABLE mini_apparatus
    ADD CONSTRAINT mini_apparatus_id_name_unique UNIQUE (id, name);

-- NOT VALID preserves legacy rows during rollout while PostgreSQL enforces
-- canonical catalog identity for every new or changed scheduling row.
ALTER TABLE mini_apparatus_capacity_profiles
    ADD CONSTRAINT mini_apparatus_capacity_profiles_identity_fk
    FOREIGN KEY (apparatus_id, apparatus)
    REFERENCES mini_apparatus(id, name)
    ON UPDATE CASCADE
    ON DELETE RESTRICT
    NOT VALID;

ALTER TABLE mini_apparatus_downtimes
    ADD CONSTRAINT mini_apparatus_downtimes_identity_fk
    FOREIGN KEY (apparatus_id, apparatus)
    REFERENCES mini_apparatus(id, name)
    ON UPDATE CASCADE
    ON DELETE RESTRICT
    NOT VALID;

ALTER TABLE mini_apparatus_schedule_reservations
    ADD CONSTRAINT mini_apparatus_schedule_reservations_identity_fk
    FOREIGN KEY (apparatus_id, apparatus)
    REFERENCES mini_apparatus(id, name)
    ON UPDATE CASCADE
    ON DELETE RESTRICT
    NOT VALID;
