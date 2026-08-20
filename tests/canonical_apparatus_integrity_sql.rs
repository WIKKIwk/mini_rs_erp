const FOLLOW_UP_MIGRATION: &str =
    include_str!("../migrations/postgres/0067_canonical_apparatus_fk_indexes.sql");

const EXPECTED_INDEXES: &[&str] = &[
    "idx_mini_apparatus_order_transfers_canonical_to",
    "idx_mini_production_map_nodes_canonical_apparatus",
    "idx_mini_production_map_nodes_canonical_alternative",
    "idx_mini_progress_batches_canonical_apparatus",
    "idx_mini_progress_batches_canonical_current_apparatus",
    "idx_mini_progress_batches_canonical_next_apparatus",
    "idx_mini_progress_batches_canonical_used_by_apparatus",
    "idx_mini_progress_batches_canonical_processed_by_apparatus",
    "idx_mini_training_queue_events_canonical_apparatus",
    "idx_mini_training_raw_assignments_canonical_apparatus",
    "idx_mini_training_input_batches_canonical_apparatus",
    "idx_mini_laminatsiya_astatka_canonical_apparatus",
    "idx_mini_rezka_astatka_canonical_apparatus",
];

#[test]
fn canonical_fk_follow_up_covers_unindexed_typed_references() {
    for index_name in EXPECTED_INDEXES {
        assert!(
            FOLLOW_UP_MIGRATION.contains(index_name),
            "missing expected canonical FK support index: {index_name}"
        );
    }

    assert_eq!(
        FOLLOW_UP_MIGRATION
            .matches("CREATE INDEX IF NOT EXISTS")
            .count(),
        EXPECTED_INDEXES.len(),
        "follow-up must contain exactly the audited index set"
    );
}

#[test]
fn canonical_fk_follow_up_is_schema_only_and_rerunnable() {
    for forbidden in [
        "INSERT INTO",
        "UPDATE ",
        "DELETE FROM",
        "TRUNCATE ",
        "DROP TABLE",
        "DROP INDEX",
    ] {
        assert!(
            !FOLLOW_UP_MIGRATION.contains(forbidden),
            "schema-only follow-up contains forbidden data/destructive SQL: {forbidden}"
        );
    }

    assert!(FOLLOW_UP_MIGRATION.contains("IF NOT EXISTS"));
}
