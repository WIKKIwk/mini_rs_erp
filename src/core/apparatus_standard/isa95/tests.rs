use super::*;
use crate::core::apparatus_standard::ApparatusId;

pub(crate) fn revision_with(
    apparatus_id: &str,
    physical_asset_id: &str,
    display_name: &str,
) -> CanonicalApparatusRevision {
    let id = ApparatusId::new(apparatus_id).unwrap();
    CanonicalApparatusRevision::from_draft(
        id,
        CanonicalApparatusDraft {
            display: ApparatusDisplay {
                display_name: display_name.to_string(),
                description: "Fixture apparatus".to_string(),
                catalog_order: 1,
            },
            equipment_class_id: EquipmentClassId::new("equipment-class:printing").unwrap(),
            physical_asset_id: PhysicalAssetId::new(physical_asset_id).unwrap(),
            hierarchy: EquipmentHierarchyScope {
                enterprise_id: HierarchyLevelId::new("enterprise:accord").unwrap(),
                site_id: HierarchyLevelId::new("site:tashkent").unwrap(),
                area_id: HierarchyLevelId::new("area:production").unwrap(),
                work_center_id: HierarchyLevelId::new("work-center:printing").unwrap(),
                work_unit_id: HierarchyLevelId::new("work-unit:print-01").unwrap(),
            },
            capabilities: vec![
                EquipmentCapability {
                    code: EquipmentCapabilityCode::Tooling,
                    level: 1,
                },
                EquipmentCapability {
                    code: EquipmentCapabilityCode::Print,
                    level: 1,
                },
                EquipmentCapability {
                    code: EquipmentCapabilityCode::Training,
                    level: 1,
                },
            ],
            execution_profile: ExecutionProfile {
                operation: ExecutionOperation::Print,
                technology: ProcessTechnology::Rotogravure,
                color_station_count: Some(7),
                max_web_width_mm: Some(1_050),
                virtual_tasks: VirtualTaskPolicy::Disabled,
                capability_compatible_reroute: true,
            },
            policies: ApparatusOperationalPolicies {
                queue: QueueDiscipline::StrictSequence,
                material: MaterialExecutionPolicy::AllRequired {
                    item_group_ids: vec!["item-group:paint".to_string()],
                },
                tooling: ToolingExecutionPolicy::QolipScanRequired {
                    tooling_class_id: "tooling-class:qolip".to_string(),
                },
            },
            capacity: ApparatusCapacity {
                capacity_slots: 1,
                setup_minutes: 10,
                cleanup_minutes: 5,
                efficiency_percent: 100,
                finite_capacity: true,
                availability: CapacityAvailability::Scheduled {
                    working_windows: vec![WorkingWindowV1 {
                        weekday: 1,
                        start_minute: 480,
                        end_minute: 1020,
                    }],
                },
            },
            placement: Some(FactoryMapPlacement {
                factory_map_object_id: "factory-map-object:print-01".to_string(),
            }),
            training: TrainingProfile {
                enabled: true,
                queue_enabled: true,
                material_tracking_enabled: true,
            },
            lifecycle: ApparatusLifecycle {
                state: LifecycleState::Active,
                retirement_reason: None,
            },
        },
        RevisionMetadata {
            revision: 1,
            committed_at_unix_ms: 1_800_000_000_000,
            actor_id: "user:admin-1".to_string(),
            command_id: "command:create-1".to_string(),
            source: RevisionSource::Admin,
            source_reference: None,
        },
    )
    .unwrap()
}

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
