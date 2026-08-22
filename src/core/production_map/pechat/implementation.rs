// Pechat (printing apparatus) compatibility rules.
//
// Mirror of the mobile client rules in
// `accord_mobile/lib/src/features/admin/logic/production_map_pechat_rules.dart`.
// The server is the source of truth: moves are validated here before any
// apparatus change is persisted. Operation, technology, capability and
// tooling policy come from PostgreSQL canonical projections; display names
// are never interpreted as live policy input.

use crate::core::apparatus_standard::{
    ApparatusId, EquipmentCapabilityCode, ExecutionOperation, ProcessTechnology,
    RuntimeApparatusConfiguration, ToolingExecutionPolicy,
};

/// Canonical pechat policy projected from one immutable apparatus identity.
///
/// Queue and handler owners should resolve their apparatus ID to a validated
/// [`RuntimeApparatusConfiguration`] before asking this module for a policy.
/// The display metadata is intentionally not part of the decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PechatPolicy<'a> {
    apparatus_id: &'a ApparatusId,
    technology: ProcessTechnology,
    color_stations: Option<u16>,
    requires_qolip_scan: bool,
}

impl<'a> PechatPolicy<'a> {
    pub fn apparatus_id(self) -> &'a ApparatusId {
        self.apparatus_id
    }

    pub fn technology(self) -> ProcessTechnology {
        self.technology
    }

    pub fn is_flexo(self) -> bool {
        self.technology == ProcessTechnology::Flexographic
    }

    pub fn color_stations(self) -> Option<u8> {
        self.color_stations.and_then(|value| u8::try_from(value).ok())
    }

    pub fn requires_qolip_scan(self) -> bool {
        self.requires_qolip_scan
    }
}

/// Returns a pechat policy for a canonical apparatus, keyed by its immutable ID.
pub fn policy_for(apparatus: &RuntimeApparatusConfiguration) -> Option<PechatPolicy<'_>> {
    let profile = &apparatus.runtime.execution_profile;
    (apparatus.is_active()
        && profile.operation == ExecutionOperation::Print
        && apparatus.supports(EquipmentCapabilityCode::Print))
    .then_some(PechatPolicy {
        apparatus_id: &apparatus.runtime.apparatus_id,
        technology: profile.technology,
        color_stations: profile.color_station_count,
        requires_qolip_scan: apparatus.supports(EquipmentCapabilityCode::Tooling)
            && matches!(
                apparatus.material.tooling,
                ToolingExecutionPolicy::QolipScanRequired { .. }
            ),
    })
}

/// Returns whether a canonical apparatus is pechat.
pub fn is_pechat_apparatus(apparatus: &RuntimeApparatusConfiguration) -> bool {
    policy_for(apparatus).is_some()
}

/// Returns whether a canonical apparatus is Flexo.
pub fn is_flexo_apparatus(apparatus: &RuntimeApparatusConfiguration) -> bool {
    policy_for(apparatus).is_some_and(|policy| policy.is_flexo())
}

/// Check whether a configured apparatus replacement preserves the operation
/// represented by the source. Queue moves and transfers must resolve both
/// sides from canonical master data before calling this helper; display
/// snapshots are deliberately not part of the decision.
pub fn reroute_compatible(
    source: &RuntimeApparatusConfiguration,
    target: &RuntimeApparatusConfiguration,
) -> bool {
    let source_profile = &source.runtime.execution_profile;
    let target_profile = &target.runtime.execution_profile;
    if !source.is_active()
        || !target.is_active()
        || !source_profile.capability_compatible_reroute
        || !target_profile.capability_compatible_reroute
        || source_profile.operation != target_profile.operation
        || source_profile.technology != target_profile.technology
    {
        return false;
    }
    let capability = operation_capability(source_profile.operation);
    source
        .runtime
        .capabilities
        .get(&capability)
        .zip(target.runtime.capabilities.get(&capability))
        .is_some_and(|(source_level, target_level)| target_level >= source_level)
}

/// Check whether an order may be rerouted between two canonical apparatuses.
///
/// The broad apparatus compatibility check is not enough for colour pechat:
/// the target station must also satisfy the order's roll and rubber-width
/// limits. Non-colour pechat uses the broad compatibility result unchanged.
pub fn reroute_order_compatible(
    source: &RuntimeApparatusConfiguration,
    target: &RuntimeApparatusConfiguration,
    roll_count: Option<i64>,
    width_mm: Option<f64>,
) -> bool {
    if !reroute_compatible(source, target) {
        return false;
    }
    let Some(target_color_stations) = pechat_color_stations(target) else {
        return true;
    };
    pechat_can_move_order(
        target_color_stations,
        roll_count,
        width_mm,
        pechat_color_stations(source),
    )
}

/// Returns the configured ColorPechat station count, or `None` for Flexo and
/// non-pechat apparatuses.
pub fn pechat_color_stations(apparatus: &RuntimeApparatusConfiguration) -> Option<u8> {
    policy_for(apparatus).and_then(PechatPolicy::color_stations)
}

/// Returns the explicit canonical Qolip tooling policy for supported pechat
/// apparatuses. Runtime checkout, custody, in-use, and QR state remain outside
/// this configuration helper.
pub fn requires_qolip_scan(apparatus: &RuntimeApparatusConfiguration) -> bool {
    policy_for(apparatus).is_some_and(|policy| policy.requires_qolip_scan())
}

fn operation_capability(operation: ExecutionOperation) -> EquipmentCapabilityCode {
    match operation {
        ExecutionOperation::Print => EquipmentCapabilityCode::Print,
        ExecutionOperation::Laminate => EquipmentCapabilityCode::Laminate,
        ExecutionOperation::Cut => EquipmentCapabilityCode::Cut,
        ExecutionOperation::Package => EquipmentCapabilityCode::Package,
        ExecutionOperation::Glue => EquipmentCapabilityCode::Glue,
    }
}

/// Rubber plate size derived from order width, in 50mm steps (50..=1350).
pub fn rubber_size_from_width(width_mm: f64) -> i64 {
    let steps = (width_mm / 50.0).ceil() as i64;
    steps.clamp(1, 27) * 50
}

/// Minimal pechat color count required by the order, or `None` when the
/// order data does not constrain the pechat (or exceeds all pechats).
pub fn recommended_pechat_color_count(
    roll_count: Option<i64>,
    width_mm: Option<f64>,
) -> Option<u8> {
    let roll = roll_count.filter(|value| *value > 0);
    let width = width_mm.filter(|value| *value > 0.0);
    if roll.is_none() && width.is_none() {
        return None;
    }

    let mut required: u8 = 0;
    if let Some(roll) = roll {
        if roll > 9 {
            return None;
        }
        required = if roll > 8 {
            9
        } else if roll > 7 {
            8
        } else {
            7
        };
    }
    if let Some(width) = width {
        let rubber = rubber_size_from_width(width);
        if rubber > 1350 {
            return None;
        }
        let rubber_required = if rubber > 1050 {
            9
        } else if rubber > 850 {
            8
        } else {
            7
        };
        required = required.max(rubber_required);
    }
    if required == 0 { None } else { Some(required) }
}

/// Whether a pechat with the given color count can physically handle the order.
pub fn pechat_can_handle_order(
    apparatus_color_count: u8,
    roll_count: Option<i64>,
    width_mm: Option<f64>,
) -> bool {
    if let Some(roll) = roll_count
        && roll > i64::from(apparatus_color_count)
    {
        return false;
    }
    let Some(width) = width_mm.filter(|value| *value > 0.0) else {
        return true;
    };
    let rubber = rubber_size_from_width(width);
    match apparatus_color_count {
        7 => rubber <= 850,
        8 => (150..=1050).contains(&rubber),
        9 => (800..=1350).contains(&rubber),
        _ => false,
    }
}

/// Whether the order may be moved onto a pechat with the given color count.
pub fn pechat_can_move_order(
    apparatus_color_count: u8,
    roll_count: Option<i64>,
    width_mm: Option<f64>,
    source_apparatus_color_count: Option<u8>,
) -> bool {
    if let Some(recommended) = recommended_pechat_color_count(roll_count, width_mm)
        && apparatus_color_count < recommended
    {
        return false;
    }
    let moving_down = source_apparatus_color_count
        .map(|source| apparatus_color_count < source)
        .unwrap_or(false);
    if moving_down {
        if width_mm.filter(|value| *value > 0.0).is_none() {
            return false;
        }
        return pechat_can_handle_order(apparatus_color_count, roll_count, width_mm);
    }
    let has_roll = roll_count.filter(|value| *value > 0).is_some();
    let has_width = width_mm.filter(|value| *value > 0.0).is_some();
    if !has_roll || !has_width {
        return apparatus_color_count != 9;
    }
    pechat_can_handle_order(apparatus_color_count, roll_count, width_mm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::apparatus_standard::test_support::{
        TestApparatusSpec, runtime_configuration,
    };

    fn canonical_apparatus(
        id: &str,
        display_name: &str,
        operation: ExecutionOperation,
        technology: ProcessTechnology,
        color_stations: Option<u8>,
        tooling_required: bool,
    ) -> RuntimeApparatusConfiguration {
        let mut spec = TestApparatusSpec::operation(id, display_name, operation, technology);
        spec.color_station_count = color_stations.map(u16::from);
        spec.tooling_required = tooling_required;
        runtime_configuration(spec)
    }

    #[test]
    fn canonical_color_pechat_7_8_9_is_pechat_and_qolip_enabled() {
        for stations in 7..=9 {
            let apparatus = canonical_apparatus(
                &format!("apparatus:test:color-{stations}"),
                &format!("renamed color machine {stations}"),
                ExecutionOperation::Print,
                ProcessTechnology::Rotogravure,
                Some(stations),
                true,
            );
            let policy = policy_for(&apparatus).expect("ColorPechat policy");
            assert!(is_pechat_apparatus(&apparatus));
            assert!(!is_flexo_apparatus(&apparatus));
            assert_eq!(policy.color_stations(), Some(stations));
            assert_eq!(pechat_color_stations(&apparatus), Some(stations));
            assert!(requires_qolip_scan(&apparatus));
        }
    }

    #[test]
    fn canonical_flexo_is_pechat_and_uses_explicit_qolip_policy() {
        let apparatus = canonical_apparatus(
            "apparatus:test:flexo-001",
            "any display name",
            ExecutionOperation::Print,
            ProcessTechnology::Flexographic,
            None,
            true,
        );
        assert!(is_pechat_apparatus(&apparatus));
        assert!(is_flexo_apparatus(&apparatus));
        assert_eq!(pechat_color_stations(&apparatus), None);
        assert!(requires_qolip_scan(&apparatus));
    }

    #[test]
    fn reroute_order_applies_colour_station_capacity() {
        let source = canonical_apparatus(
            "apparatus:test:color-source",
            "source",
            ExecutionOperation::Print,
            ProcessTechnology::Rotogravure,
            Some(8),
            true,
        );
        let seven_color_target = canonical_apparatus(
            "apparatus:test:color-target-7",
            "target 7",
            ExecutionOperation::Print,
            ProcessTechnology::Rotogravure,
            Some(7),
            true,
        );
        let nine_color_target = canonical_apparatus(
            "apparatus:test:color-target-9",
            "target 9",
            ExecutionOperation::Print,
            ProcessTechnology::Rotogravure,
            Some(9),
            true,
        );

        assert!(!reroute_order_compatible(
            &source,
            &seven_color_target,
            Some(7),
            Some(900.0)
        ));
        assert!(reroute_order_compatible(
            &source,
            &nine_color_target,
            Some(7),
            Some(1250.0)
        ));
    }

    #[test]
    fn non_pechat_never_inherits_qolip_requirement() {
        let apparatus = canonical_apparatus(
            "apparatus:test:lam-001",
            "7 ta rangli pechat",
            ExecutionOperation::Laminate,
            ProcessTechnology::AdhesiveLamination,
            None,
            false,
        );
        assert!(!is_pechat_apparatus(&apparatus));
        assert!(!is_flexo_apparatus(&apparatus));
        assert_eq!(pechat_color_stations(&apparatus), None);
        assert!(!requires_qolip_scan(&apparatus));
    }

    #[test]
    fn canonical_policy_is_independent_of_display_name() {
        let mut apparatus = canonical_apparatus(
            "apparatus:test:stable-001",
            "7 ta rangli pechat",
            ExecutionOperation::Print,
            ProcessTechnology::Rotogravure,
            Some(7),
            true,
        );
        let original_id = apparatus.runtime.apparatus_id.clone();
        let (original_technology, original_color_stations, original_requires_qolip_scan) = {
            let original_policy = policy_for(&apparatus).expect("ColorPechat policy");
            (
                original_policy.technology(),
                original_policy.color_stations(),
                original_policy.requires_qolip_scan(),
            )
        };
        apparatus.runtime.display.display_name = "Laminatsiya renamed".to_string();
        let renamed_policy = policy_for(&apparatus).expect("ColorPechat policy");
        assert_eq!(original_id.as_str(), renamed_policy.apparatus_id().as_str());
        assert_eq!(original_technology, renamed_policy.technology());
        assert_eq!(original_color_stations, renamed_policy.color_stations());
        assert_eq!(
            original_requires_qolip_scan,
            renamed_policy.requires_qolip_scan()
        );
    }

    #[test]
    fn recommended_color_count_uses_roll_and_rubber() {
        assert_eq!(recommended_pechat_color_count(Some(7), None), Some(7));
        assert_eq!(recommended_pechat_color_count(Some(8), None), Some(8));
        assert_eq!(recommended_pechat_color_count(Some(9), None), Some(9));
        assert_eq!(recommended_pechat_color_count(Some(10), None), None);
        assert_eq!(recommended_pechat_color_count(None, Some(650.0)), Some(7));
        assert_eq!(recommended_pechat_color_count(None, Some(815.0)), Some(7));
        assert_eq!(recommended_pechat_color_count(None, Some(900.0)), Some(8));
        assert_eq!(recommended_pechat_color_count(None, Some(1050.0)), Some(8));
        assert_eq!(recommended_pechat_color_count(None, Some(1250.0)), Some(9));
        // Width is clamped to 27 rubber steps (1350mm), matching the client.
        assert_eq!(recommended_pechat_color_count(None, Some(1500.0)), Some(9));
        assert_eq!(
            recommended_pechat_color_count(Some(7), Some(1250.0)),
            Some(9)
        );
        assert_eq!(recommended_pechat_color_count(None, None), None);
    }

    #[test]
    fn move_allows_compatible_order_from_seven_to_eight_color_pechat() {
        assert!(pechat_can_move_order(8, Some(7), Some(650.0), Some(7)));
    }

    #[test]
    fn move_blocks_nine_color_rubber_on_seven_color_pechat() {
        assert!(!pechat_can_move_order(7, Some(7), Some(1250.0), Some(8)));
        assert!(pechat_can_move_order(9, Some(7), Some(1250.0), Some(8)));
    }

    #[test]
    fn move_down_requires_width_and_compatibility() {
        assert!(!pechat_can_move_order(7, Some(7), None, Some(8)));
        assert!(pechat_can_move_order(7, Some(7), Some(650.0), Some(8)));
        assert!(pechat_can_move_order(7, Some(7), Some(815.0), Some(8)));
        assert!(!pechat_can_move_order(7, Some(7), Some(900.0), Some(8)));
    }

    #[test]
    fn expanded_rubber_limits_are_enforced() {
        assert_eq!(rubber_size_from_width(815.0), 850);
        assert!(pechat_can_handle_order(7, Some(7), Some(815.0)));
        assert!(!pechat_can_handle_order(7, Some(7), Some(851.0)));
        assert!(pechat_can_handle_order(8, Some(8), Some(1050.0)));
        assert!(!pechat_can_handle_order(8, Some(8), Some(1051.0)));
        assert_eq!(rubber_size_from_width(1500.0), 1350);
        assert!(pechat_can_handle_order(9, Some(9), Some(1350.0)));
    }

    #[test]
    fn move_without_order_data_avoids_nine_color() {
        assert!(pechat_can_move_order(8, None, None, None));
        assert!(!pechat_can_move_order(9, None, None, None));
    }
}
