#[test]
fn canonical_projection_tables_are_guarded_by_the_single_writer_transaction() {
    let migration = include_str!(
        "../../../migrations/postgres/0069_canonical_apparatus_revision_authority.sql"
    );

    for table in [
        "mini_apparatus",
        "mini_apparatus_queue_policies",
        "mini_apparatus_material_rules",
        "mini_apparatus_capacity_profiles",
    ] {
        assert!(
            migration.contains(table),
            "0069 must govern the {table} projection"
        );
    }
    assert!(migration.contains("mini_canonical_apparatus_writer_enabled"));
    assert!(migration.contains("canonical apparatus projections are read-only"));
}
