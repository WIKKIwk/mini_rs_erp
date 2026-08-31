use super::*;
use crate::ApparatusId;

pub fn revision_with(
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
                min_web_width_mm: Some(150),
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
