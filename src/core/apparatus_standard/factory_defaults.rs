use super::{
    ApparatusCapacity, ApparatusDisplay, ApparatusId, ApparatusLifecycle,
    ApparatusOperationalPolicies, CanonicalApparatusDraft, CapacityAvailability,
    EquipmentCapability, EquipmentCapabilityCode, EquipmentClassId, EquipmentHierarchyScope,
    ExecutionOperation, ExecutionProfile, HierarchyLevelId, LifecycleState,
    MaterialExecutionPolicy, PhysicalAssetId, ProcessTechnology, QueueDiscipline,
    ToolingExecutionPolicy, TrainingProfile, VirtualTaskPolicy,
};

pub(super) const FACTORY_DEFAULT_COMMITTED_AT_UNIX_MS: i64 = 1_700_000_000_000;

pub(super) struct FactoryDefaultApparatus {
    pub(super) apparatus_id: ApparatusId,
    pub(super) draft: CanonicalApparatusDraft,
}

struct FactoryDefaultSpec {
    apparatus_id: &'static str,
    display_name: &'static str,
    asset_key: &'static str,
    catalog_order: u32,
    operation: ExecutionOperation,
    technology: ProcessTechnology,
    color_station_count: Option<u16>,
    tooling_required: bool,
}

pub(super) fn factory_default_apparatus() -> Vec<FactoryDefaultApparatus> {
    factory_default_specs()
        .into_iter()
        .map(|spec| FactoryDefaultApparatus {
            apparatus_id: ApparatusId::new(spec.apparatus_id)
                .expect("factory default apparatus IDs are canonical"),
            draft: factory_default_draft(&spec),
        })
        .collect()
}

fn factory_default_specs() -> [FactoryDefaultSpec; 10] {
    [
        FactoryDefaultSpec {
            apparatus_id: "apparatus:default:bosma_7",
            display_name: "7 ta rangli bosma aparat",
            asset_key: "bosma-7",
            catalog_order: 0,
            operation: ExecutionOperation::Print,
            technology: ProcessTechnology::Rotogravure,
            color_station_count: Some(7),
            tooling_required: true,
        },
        FactoryDefaultSpec {
            apparatus_id: "apparatus:default:bosma_8",
            display_name: "8 ta rangli bosma aparat",
            asset_key: "bosma-8",
            catalog_order: 1,
            operation: ExecutionOperation::Print,
            technology: ProcessTechnology::Rotogravure,
            color_station_count: Some(8),
            tooling_required: true,
        },
        FactoryDefaultSpec {
            apparatus_id: "apparatus:default:bosma_9",
            display_name: "9 ta rangli bosma aparat",
            asset_key: "bosma-9",
            catalog_order: 2,
            operation: ExecutionOperation::Print,
            technology: ProcessTechnology::Rotogravure,
            color_station_count: Some(9),
            tooling_required: true,
        },
        FactoryDefaultSpec {
            apparatus_id: "apparatus:default:asset-004",
            display_name: "Extruder laminatsiya",
            asset_key: "extruder-laminatsiya",
            catalog_order: 3,
            operation: ExecutionOperation::Laminate,
            technology: ProcessTechnology::ExtrusionLamination,
            color_station_count: None,
            tooling_required: false,
        },
        FactoryDefaultSpec {
            apparatus_id: "apparatus:default:asset-005",
            display_name: "Flexo pechat",
            asset_key: "flexo-pechat",
            catalog_order: 4,
            operation: ExecutionOperation::Print,
            technology: ProcessTechnology::Flexographic,
            color_station_count: None,
            tooling_required: true,
        },
        FactoryDefaultSpec {
            apparatus_id: "apparatus:default:holodniy_kley",
            display_name: "Holodniy kley aparat",
            asset_key: "holodniy-kley",
            catalog_order: 5,
            operation: ExecutionOperation::Glue,
            technology: ProcessTechnology::ColdGlue,
            color_station_count: None,
            tooling_required: false,
        },
        FactoryDefaultSpec {
            apparatus_id: "apparatus:default:asset-007",
            display_name: "Laminatsiya 1",
            asset_key: "laminatsiya-1",
            catalog_order: 6,
            operation: ExecutionOperation::Laminate,
            technology: ProcessTechnology::AdhesiveLamination,
            color_station_count: None,
            tooling_required: false,
        },
        FactoryDefaultSpec {
            apparatus_id: "apparatus:default:asset-008",
            display_name: "Laminatsiya 2",
            asset_key: "laminatsiya-2",
            catalog_order: 7,
            operation: ExecutionOperation::Laminate,
            technology: ProcessTechnology::AdhesiveLamination,
            color_station_count: None,
            tooling_required: false,
        },
        FactoryDefaultSpec {
            apparatus_id: "apparatus:default:paket",
            display_name: "Paket aparat",
            asset_key: "paket",
            catalog_order: 8,
            operation: ExecutionOperation::Package,
            technology: ProcessTechnology::BagMaking,
            color_station_count: None,
            tooling_required: false,
        },
        FactoryDefaultSpec {
            apparatus_id: "apparatus:default:asset-010",
            display_name: "Rezka",
            asset_key: "rezka",
            catalog_order: 9,
            operation: ExecutionOperation::Cut,
            technology: ProcessTechnology::Slitting,
            color_station_count: None,
            tooling_required: false,
        },
    ]
}

fn factory_default_draft(spec: &FactoryDefaultSpec) -> CanonicalApparatusDraft {
    let operation_capability = match spec.operation {
        ExecutionOperation::Print => EquipmentCapabilityCode::Print,
        ExecutionOperation::Laminate => EquipmentCapabilityCode::Laminate,
        ExecutionOperation::Cut => EquipmentCapabilityCode::Cut,
        ExecutionOperation::Package => EquipmentCapabilityCode::Package,
        ExecutionOperation::Glue => EquipmentCapabilityCode::Glue,
    };
    let mut capabilities = vec![EquipmentCapability {
        code: operation_capability,
        level: 1,
    }];
    if spec.tooling_required {
        capabilities.push(EquipmentCapability {
            code: EquipmentCapabilityCode::Tooling,
            level: 1,
        });
    }
    capabilities.sort_by_key(|capability| capability.code);

    let operation_key = match spec.operation {
        ExecutionOperation::Print => "print",
        ExecutionOperation::Laminate => "laminate",
        ExecutionOperation::Cut => "cut",
        ExecutionOperation::Package => "package",
        ExecutionOperation::Glue => "glue",
    };
    let technology_key = match spec.technology {
        ProcessTechnology::Rotogravure => "rotogravure",
        ProcessTechnology::Flexographic => "flexographic",
        ProcessTechnology::AdhesiveLamination => "adhesive-lamination",
        ProcessTechnology::ExtrusionLamination => "extrusion-lamination",
        ProcessTechnology::Slitting => "slitting",
        ProcessTechnology::BagMaking => "bag-making",
        ProcessTechnology::ColdGlue => "cold-glue",
    };

    CanonicalApparatusDraft {
        display: ApparatusDisplay {
            display_name: spec.display_name.to_string(),
            description: String::new(),
            catalog_order: spec.catalog_order,
        },
        equipment_class_id: EquipmentClassId::new(format!(
            "equipment-class:default:{technology_key}"
        ))
        .expect("factory default equipment class IDs are canonical"),
        physical_asset_id: PhysicalAssetId::new(format!(
            "physical-asset:default:{}",
            spec.asset_key
        ))
        .expect("factory default physical asset IDs are canonical"),
        hierarchy: EquipmentHierarchyScope {
            enterprise_id: HierarchyLevelId::new("enterprise:accord")
                .expect("factory enterprise ID is canonical"),
            site_id: HierarchyLevelId::new("site:default").expect("factory site ID is canonical"),
            area_id: HierarchyLevelId::new("area:production")
                .expect("factory area ID is canonical"),
            work_center_id: HierarchyLevelId::new(format!("work-center:default:{operation_key}"))
                .expect("factory work center IDs are canonical"),
            work_unit_id: HierarchyLevelId::new(format!("work-unit:default:{}", spec.asset_key))
                .expect("factory work unit IDs are canonical"),
        },
        capabilities,
        execution_profile: ExecutionProfile {
            operation: spec.operation,
            technology: spec.technology,
            color_station_count: spec.color_station_count,
            max_web_width_mm: None,
            virtual_tasks: VirtualTaskPolicy::Disabled,
            capability_compatible_reroute: true,
        },
        policies: ApparatusOperationalPolicies {
            queue: QueueDiscipline::StrictSequence,
            material: MaterialExecutionPolicy::NotRequired {
                item_group_ids: Vec::new(),
            },
            tooling: if spec.tooling_required {
                ToolingExecutionPolicy::QolipScanRequired {
                    tooling_class_id: "tooling-class:qolip".to_string(),
                }
            } else {
                ToolingExecutionPolicy::NotRequired
            },
        },
        capacity: ApparatusCapacity {
            capacity_slots: 1,
            setup_minutes: 0,
            cleanup_minutes: 0,
            efficiency_percent: 100,
            finite_capacity: true,
            availability: CapacityAvailability::Always,
        },
        placement: None,
        training: TrainingProfile {
            enabled: false,
            queue_enabled: false,
            material_tracking_enabled: false,
        },
        lifecycle: ApparatusLifecycle {
            state: LifecycleState::Active,
            retirement_reason: None,
        },
    }
}
