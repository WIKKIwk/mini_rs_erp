use super::test_support::revision_with;
use super::*;

#[test]
fn duplicate_display_names_are_not_identity() {
    let first = revision_with(
        "apparatus:test:01aa",
        "physical-asset:press-01",
        "Shared display",
    );
    let second = revision_with(
        "apparatus:test:02bb",
        "physical-asset:press-02",
        "Shared display",
    );

    first.validate().unwrap();
    second.validate().unwrap();
    assert_eq!(first.display.display_name, second.display.display_name);
    assert_ne!(first.apparatus_id, second.apparatus_id);
    assert_ne!(first.physical_asset_id, second.physical_asset_id);
    assert_ne!(first.aas_identity.shell_id, second.aas_identity.shell_id);
}

#[test]
fn draft_normalization_makes_collections_canonical() {
    let mut revision = revision_with(
        "apparatus:test:03cc",
        "physical-asset:press-03",
        "Canonical order",
    );
    revision.capabilities.reverse();
    assert_eq!(
        revision.validate(),
        Err(CanonicalApparatusValidationError::InvalidCapabilities)
    );

    let rebuilt = CanonicalApparatusRevision::from_draft(
        revision.apparatus_id.clone(),
        revision.to_draft(),
        revision.revision_metadata.clone(),
    )
    .unwrap();
    rebuilt.validate().unwrap();
}

#[test]
fn behavior_requires_explicit_capability_and_profile_consistency() {
    let mut revision = revision_with(
        "apparatus:test:04dd",
        "physical-asset:press-04",
        "Explicit behavior",
    );
    revision
        .capabilities
        .retain(|capability| capability.code != EquipmentCapabilityCode::Print);
    assert_eq!(
        revision.validate(),
        Err(CanonicalApparatusValidationError::InvalidExecutionProfile)
    );
}

#[test]
fn execution_profile_rejects_inverted_web_width_range() {
    let mut revision = revision_with(
        "apparatus:test:04ee",
        "physical-asset:press-04e",
        "Invalid width range",
    );
    revision.execution_profile.min_web_width_mm = Some(1_100);
    revision.execution_profile.max_web_width_mm = Some(800);
    assert_eq!(
        revision.validate(),
        Err(CanonicalApparatusValidationError::InvalidExecutionProfile)
    );
}

#[test]
fn required_fields_have_no_deserialization_defaults() {
    let revision = revision_with(
        "apparatus:test:05ee",
        "physical-asset:press-05",
        "Complete payload",
    );
    let mut value = serde_json::to_value(revision).unwrap();
    value.as_object_mut().unwrap().remove("capacity");
    assert!(serde_json::from_value::<CanonicalApparatusRevision>(value).is_err());
}

#[test]
fn empty_optional_material_policy_keeps_the_existing_canonical_json_shape() {
    let legacy_shape = serde_json::json!({"mode": "not_required"});
    let policy: MaterialExecutionPolicy = serde_json::from_value(legacy_shape.clone()).unwrap();

    assert_eq!(
        policy,
        MaterialExecutionPolicy::NotRequired {
            item_group_ids: Vec::new(),
        }
    );
    assert_eq!(serde_json::to_value(policy).unwrap(), legacy_shape);
}
