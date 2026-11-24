use super::*;
use crate::core::apparatus_standard::test_support::{TestApparatusSpec, runtime_configuration};

#[test]
fn reroute_respects_print_profiles_and_flexo_capacity() {
    let source = canonical_apparatus("apparatus:test:source", "Source",
        ExecutionOperation::Print, ProcessTechnology::Rotogravure, Some(8), false);
    let mut target = canonical_apparatus("apparatus:test:target", "Target",
        ExecutionOperation::Print, ProcessTechnology::Rotogravure, Some(8), false);
    target.runtime.execution_profile.max_web_width_mm = Some(600);
    assert!(!reroute_order_compatible(&source, &target, Some(7), Some(650.0)));
    target.runtime.execution_profile.max_web_width_mm = Some(1500);
    target.runtime.execution_profile.technology = ProcessTechnology::Flexographic;
    assert!(reroute_order_compatible(&source, &target, Some(8), Some(1500.0)));
    assert!(!reroute_order_compatible(&source, &target, Some(8), Some(1501.0)));
    assert!(!reroute_order_compatible(&source, &target, Some(9), Some(650.0)));
    assert!(!pechat_can_handle_order(9, Some(7), Some(1500.0)));
}

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
    let mut apparatus = canonical_apparatus(
        "apparatus:test:flexo-001",
        "any display name",
        ExecutionOperation::Print,
        ProcessTechnology::Flexographic,
        Some(8),
        true,
    );
    apparatus.runtime.execution_profile.min_web_width_mm = Some(400);
    apparatus.runtime.execution_profile.max_web_width_mm = Some(800);
    let policy = policy_for(&apparatus).expect("Flexo policy");
    assert!(is_pechat_apparatus(&apparatus));
    assert!(is_flexo_apparatus(&apparatus));
    assert_eq!(policy.color_stations(), Some(8));
    assert_eq!(pechat_color_stations(&apparatus), Some(8));
    assert!(requires_qolip_scan(&apparatus));
    assert!(apparatus_profile_can_handle_order(
        &apparatus,
        Some(1),
        Some(400.0)
    ));
    assert!(apparatus_profile_can_handle_order(
        &apparatus,
        Some(8),
        Some(800.0)
    ));
    assert!(!apparatus_profile_can_handle_order(
        &apparatus,
        Some(8),
        Some(399.0)
    ));
    assert!(!apparatus_profile_can_handle_order(
        &apparatus,
        Some(8),
        Some(801.0)
    ));
    assert!(!apparatus_profile_can_handle_order(
        &apparatus,
        Some(9),
        Some(650.0)
    ));
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
