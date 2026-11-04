ALTER TABLE mini_progress_batches
    DROP CONSTRAINT mini_progress_batches_wip_status_allowed,
    ADD CONSTRAINT mini_progress_batches_wip_status_allowed
        CHECK (wip_status IN ('waiting', 'in_use', 'processed', 'peremotka'));
