ALTER TABLE mini_opening_wip_batches
    ADD COLUMN finished_goods_meter NUMERIC(18, 6),
    ADD COLUMN finished_goods_kg NUMERIC(18, 6),
    ADD COLUMN bobina_kg NUMERIC(18, 6),
    ADD COLUMN diameter NUMERIC(18, 6);

ALTER TABLE mini_opening_wip_batches
    ADD CONSTRAINT mini_opening_wip_batches_finished_goods_meter_positive
        CHECK (finished_goods_meter IS NULL OR finished_goods_meter > 0),
    ADD CONSTRAINT mini_opening_wip_batches_finished_goods_kg_positive
        CHECK (finished_goods_kg IS NULL OR finished_goods_kg > 0),
    ADD CONSTRAINT mini_opening_wip_batches_bobina_kg_positive
        CHECK (bobina_kg IS NULL OR bobina_kg > 0),
    ADD CONSTRAINT mini_opening_wip_batches_diameter_positive
        CHECK (diameter IS NULL OR diameter > 0);
