-- Preserve the previous automatic-print choice for both sides. The switches
-- now control printing only; recording remainders has one common workflow.
ALTER TABLE mini_lamination_settings
    ADD COLUMN print_wip_remainder_labels BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN print_film_remainder_labels BOOLEAN NOT NULL DEFAULT TRUE;

UPDATE mini_lamination_settings SET
    print_wip_remainder_labels = layer_remainders_required,
    print_film_remainder_labels = layer_remainders_required;
