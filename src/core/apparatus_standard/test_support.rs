use super::*;

#[derive(Debug, Clone)]
pub(crate) struct TestApparatusSpec<'a> {
    pub(crate) apparatus_id: &'a str,
    pub(crate) display_name: &'a str,
    pub(crate) operation: ExecutionOperation,
    pub(crate) technology: ProcessTechnology,
    pub(crate) color_station_count: Option<u16>,
    pub(crate) tooling_required: bool,
    pub(crate) capability_level: u16,
    pub(crate) capacity_slots: u16,
    pub(crate) setup_minutes: u32,
    pub(crate) cleanup_minutes: u32,
    pub(crate) finite_capacity: bool,
}

impl<'a> TestApparatusSpec<'a> {
    pub(crate) fn print(
        apparatus_id: &'a str,
        display_name: &'a str,
        technology: ProcessTechnology,
        color_station_count: Option<u16>,
    ) -> Self {
        Self {
            apparatus_id,
            display_name,
            operation: ExecutionOperation::Print,
            technology,
            color_station_count,
            tooling_required: false,
            capability_level: 1,
            capacity_slots: 1,
            setup_minutes: 0,
            cleanup_minutes: 0,
            finite_capacity: true,
        }
    }

    pub(crate) fn laminate(apparatus_id: &'a str, display_name: &'a str) -> Self {
        Self::operation(
            apparatus_id,
            display_name,
            ExecutionOperation::Laminate,
            ProcessTechnology::AdhesiveLamination,
        )
    }

    pub(crate) fn cut(apparatus_id: &'a str, display_name: &'a str) -> Self {
        Self::operation(
            apparatus_id,
            display_name,
            ExecutionOperation::Cut,
            ProcessTechnology::Slitting,
        )
    }

    pub(crate) fn package(apparatus_id: &'a str, display_name: &'a str) -> Self {
        Self::operation(
            apparatus_id,
            display_name,
            ExecutionOperation::Package,
            ProcessTechnology::BagMaking,
        )
    }

    pub(crate) fn operation(
        apparatus_id: &'a str,
        display_name: &'a str,
        operation: ExecutionOperation,
        technology: ProcessTechnology,
    ) -> Self {
        Self {
            apparatus_id,
            display_name,
            operation,
            technology,
            color_station_count: None,
            tooling_required: false,
            capability_level: 1,
            capacity_slots: 1,
            setup_minutes: 0,
            cleanup_minutes: 0,
            finite_capacity: true,
        }
    }
}

pub(crate) fn canonical_draft(spec: &TestApparatusSpec<'_>) -> CanonicalApparatusDraft {
    let operation_capability = match spec.operation {
        ExecutionOperation::Print => EquipmentCapabilityCode::Print,
        ExecutionOperation::Laminate => EquipmentCapabilityCode::Laminate,
        ExecutionOperation::Cut => EquipmentCapabilityCode::Cut,
        ExecutionOperation::Package => EquipmentCapabilityCode::Package,
        ExecutionOperation::Glue => EquipmentCapabilityCode::Glue,
    };
    let mut capabilities = vec![
        EquipmentCapability {
            code: operation_capability,
            level: spec.capability_level,
        },
        EquipmentCapability {
            code: EquipmentCapabilityCode::Training,
            level: 1,
        },
    ];
    if spec.tooling_required {
        capabilities.push(EquipmentCapability {
            code: EquipmentCapabilityCode::Tooling,
            level: 1,
        });
    }
    capabilities.sort_by_key(|capability| capability.code);
    CanonicalApparatusDraft {
        display: ApparatusDisplay {
            display_name: spec.display_name.to_string(),
            description: "Explicit canonical test fixture".to_string(),
            catalog_order: 1,
        },
        equipment_class_id: EquipmentClassId::new(format!(
            "equipment-class:test:{:?}",
            spec.operation
        ))
        .expect("test equipment class"),
        physical_asset_id: PhysicalAssetId::new(format!(
            "physical-asset:test:{}",
            spec.apparatus_id.replace(':', "-")
        ))
        .expect("test physical asset"),
        hierarchy: EquipmentHierarchyScope {
            enterprise_id: HierarchyLevelId::new("enterprise:test").unwrap(),
            site_id: HierarchyLevelId::new("site:test").unwrap(),
            area_id: HierarchyLevelId::new("area:test").unwrap(),
            work_center_id: HierarchyLevelId::new("work-center:test").unwrap(),
            work_unit_id: HierarchyLevelId::new(format!(
                "work-unit:test:{}",
                spec.apparatus_id.replace(':', "-")
            ))
            .unwrap(),
        },
        capabilities,
        execution_profile: ExecutionProfile {
            operation: spec.operation,
            technology: spec.technology,
            color_station_count: spec.color_station_count,
            virtual_tasks: VirtualTaskPolicy::Disabled,
            capability_compatible_reroute: true,
        },
        policies: ApparatusOperationalPolicies {
            queue: QueueDiscipline::StrictSequence,
            material: MaterialExecutionPolicy::NotRequired,
            tooling: if spec.tooling_required {
                ToolingExecutionPolicy::QolipScanRequired {
                    tooling_class_id: "tooling-class:qolip".to_string(),
                }
            } else {
                ToolingExecutionPolicy::NotRequired
            },
        },
        capacity: ApparatusCapacity {
            capacity_slots: spec.capacity_slots,
            setup_minutes: spec.setup_minutes,
            cleanup_minutes: spec.cleanup_minutes,
            efficiency_percent: 100,
            finite_capacity: spec.finite_capacity,
            availability: CapacityAvailability::Always,
        },
        placement: None,
        training: TrainingProfile {
            enabled: true,
            queue_enabled: true,
            material_tracking_enabled: true,
        },
        lifecycle: ApparatusLifecycle {
            state: LifecycleState::Active,
            retirement_reason: None,
        },
    }
}

pub(crate) fn runtime_configuration(spec: TestApparatusSpec<'_>) -> RuntimeApparatusConfiguration {
    let apparatus_id = ApparatusId::new(spec.apparatus_id).expect("test apparatus id");
    let revision = CanonicalApparatusRevision::from_draft(
        apparatus_id,
        canonical_draft(&spec),
        RevisionMetadata {
            revision: 1,
            committed_at_unix_ms: 1_800_000_000_000,
            actor_id: "user:test".to_string(),
            command_id: format!("command:test:{}", spec.apparatus_id.replace(':', "-")),
            source: RevisionSource::Admin,
            source_reference: None,
        },
    )
    .expect("valid canonical test revision");
    let artifact = export_canonical_aasx(&revision).expect("canonical test AASX");
    project_apparatus_revision(&revision, artifact.sha256()).into()
}

pub(crate) fn standard_runtime_configurations() -> Vec<RuntimeApparatusConfiguration> {
    use ProcessTechnology::{Flexographic, Rotogravure};

    vec![
        runtime_configuration(TestApparatusSpec::print(
            "apparatus:default:bosma_7",
            "Bosma 7",
            Rotogravure,
            Some(7),
        )),
        runtime_configuration(TestApparatusSpec::print(
            "apparatus:default:bosma_8",
            "Bosma 8",
            Rotogravure,
            Some(8),
        )),
        runtime_configuration(TestApparatusSpec::print(
            "apparatus:default:bosma_9",
            "Bosma 9",
            Rotogravure,
            Some(9),
        )),
        runtime_configuration(TestApparatusSpec::print(
            "apparatus:default:flexo_pechat",
            "Flexo",
            Flexographic,
            None,
        )),
        runtime_configuration(TestApparatusSpec::laminate(
            "apparatus:default:asset-007",
            "Laminatsiya 1",
        )),
        runtime_configuration(TestApparatusSpec::laminate(
            "apparatus:default:asset-008",
            "Laminatsiya 2",
        )),
        runtime_configuration(TestApparatusSpec::cut(
            "apparatus:default:asset-010",
            "Rezka",
        )),
        runtime_configuration(TestApparatusSpec::package(
            "apparatus:default:paket",
            "Paket",
        )),
    ]
}
