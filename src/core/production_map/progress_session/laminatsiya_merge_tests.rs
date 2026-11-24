use super::*;

#[test]
fn laminatsiya_merge_requires_matching_complete_kadr_metadata() {
    assert_eq!(
        laminatsiya_merge_contained_kadr_count(Some(2), Some(2)).unwrap(),
        Some(2)
    );
    assert_eq!(
        laminatsiya_merge_contained_kadr_count(None, None).unwrap(),
        None
    );
    assert!(matches!(
        laminatsiya_merge_contained_kadr_count(Some(2), Some(1)),
        Err(ProductionMapError::MergeInputFrameCountMismatch {
            active_kadr_count: 2,
            scanned_kadr_count: 1,
        })
    ));
    assert!(matches!(
        laminatsiya_merge_contained_kadr_count(Some(2), None),
        Err(ProductionMapError::MergeInputNotAccepted)
    ));
}
