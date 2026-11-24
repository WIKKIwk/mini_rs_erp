use super::{
    apparatus_ids_match, canonical_apparatus_id, canonical_apparatus_key, stage_ids_match,
};

#[test]
fn progress_identity_requires_canonical_ids_and_ignores_display_titles() {
    assert!(apparatus_ids_match(
        "apparatus:catalog:press-001",
        "apparatus:catalog:press-001"
    ));
    assert!(!apparatus_ids_match(
        "apparatus:catalog:press-001",
        "apparatus:catalog:press-002"
    ));
    assert!(!apparatus_ids_match(
        "apparatus:press-001",
        "apparatus:catalog:press-001"
    ));
    assert!(!apparatus_ids_match(
        "8 ta rangli pechat",
        "apparatus:catalog:press-001"
    ));
    assert!(!apparatus_ids_match(
        "task:lamination-1",
        "task:lamination-1"
    ));
    assert!(stage_ids_match("task:lamination-1", "task:lamination-1"));
    assert!(!stage_ids_match("task:", "task:"));
    assert!(!stage_ids_match("Laminatsiya", "laminatsiya"));
    assert!(!stage_ids_match("laminatsiya", "laminatsiya"));
}

#[test]
fn progress_key_preserves_id_across_display_rename() {
    let id = canonical_apparatus_id("apparatus:catalog:press-001").unwrap();
    assert_eq!(
        canonical_apparatus_key(id.as_str()),
        "apparatus:catalog:press-001"
    );
    assert_eq!(canonical_apparatus_key("8 ta rangli pechat"), "");
}

#[test]
fn progress_qr_remains_owned_by_the_batch_identity() {
    let batch_id = "progress-batch:123:apparatus:catalog:press-001:order-7";
    let renamed_display_batch_id = batch_id;
    assert_eq!(
        crate::core::production_map::progress_qr_payload(batch_id),
        crate::core::production_map::progress_qr_payload(renamed_display_batch_id)
    );
    assert_ne!(
        crate::core::production_map::progress_qr_payload(batch_id),
        crate::core::production_map::progress_qr_payload(
            "progress-batch:123:apparatus:catalog:press-002:order-7"
        )
    );
}
