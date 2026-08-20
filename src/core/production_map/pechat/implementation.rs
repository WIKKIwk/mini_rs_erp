// Pechat (printing apparatus) compatibility rules.
//
// Mirror of the mobile client rules in
// `accord_mobile/lib/src/features/admin/logic/production_map_pechat_rules.dart`.
// The server is the source of truth: moves are validated here before any
// apparatus change is persisted. Apparatus classification and tooling policy
// come from the canonical apparatus configuration; display names are never
// interpreted as live policy input.

use crate::core::apparatus_standard::{
    ApparatusClassification, ApparatusFamily, ApparatusId, ApparatusKind, CanonicalApparatus,
    ToolingPolicy,
};

/// Canonical pechat policy projected from one immutable apparatus identity.
///
/// Queue and handler owners should resolve their apparatus ID to a validated
/// [`CanonicalApparatus`] before asking this module for a policy. The display
/// metadata is intentionally not part of the projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PechatPolicy<'a> {
    apparatus_id: &'a ApparatusId,
    classification: &'a ApparatusClassification,
    tooling: ToolingPolicy,
}

impl<'a> PechatPolicy<'a> {
    pub fn apparatus_id(self) -> &'a ApparatusId {
        self.apparatus_id
    }

    pub fn classification(self) -> &'a ApparatusClassification {
        self.classification
    }

    pub fn is_flexo(self) -> bool {
        self.classification.kind == ApparatusKind::Flexo
    }

    pub fn color_stations(self) -> Option<u8> {
        (self.classification.kind == ApparatusKind::ColorPechat)
            .then_some(self.classification.color_stations)
            .flatten()
    }

    pub fn requires_qolip_scan(self) -> bool {
        self.tooling == ToolingPolicy::QolipScanRequired
    }
}

/// Returns whether the canonical classification is a supported pechat type.
pub fn is_pechat_classification(classification: &ApparatusClassification) -> bool {
    classification.family == ApparatusFamily::Pechat
        && matches!(
            classification.kind,
            ApparatusKind::ColorPechat | ApparatusKind::Flexo
        )
}

/// Returns whether the canonical classification is Flexo.
pub fn is_flexo_classification(classification: &ApparatusClassification) -> bool {
    is_pechat_classification(classification) && classification.kind == ApparatusKind::Flexo
}

/// Returns a pechat policy for a canonical apparatus, keyed by its immutable ID.
pub fn policy_for(apparatus: &CanonicalApparatus) -> Option<PechatPolicy<'_>> {
    is_pechat_classification(&apparatus.classification).then_some(PechatPolicy {
        apparatus_id: &apparatus.identity.id,
        classification: &apparatus.classification,
        tooling: apparatus.policies.tooling,
    })
}

/// Returns whether a canonical apparatus is pechat.
pub fn is_pechat_apparatus(apparatus: &CanonicalApparatus) -> bool {
    policy_for(apparatus).is_some()
}

/// Returns whether a canonical apparatus is Flexo.
pub fn is_flexo_apparatus(apparatus: &CanonicalApparatus) -> bool {
    policy_for(apparatus).is_some_and(|policy| policy.is_flexo())
}

/// Check whether a configured apparatus replacement preserves the operation
/// represented by the source. Queue moves and transfers must resolve both
/// sides from canonical master data before calling this helper; display
/// snapshots are deliberately not part of the decision.
pub fn reroute_compatible(source: &CanonicalApparatus, target: &CanonicalApparatus) -> bool {
    if source.classification.family != target.classification.family {
        return false;
    }
    // Flexo and colour pechat share the broad pechat capability but are not
    // interchangeable process families. In particular, a Flexo order must
    // never be silently rerouted onto a colour-count station (or vice versa).
    if source.classification.family == ApparatusFamily::Pechat
        && source.classification.kind != target.classification.kind
    {
        return false;
    }
    source
        .capabilities
        .iter()
        .any(|capability| target.capabilities.contains(capability))
}

/// Check whether an order may be rerouted between two canonical apparatuses.
///
/// The broad apparatus compatibility check is not enough for colour pechat:
/// the target station must also satisfy the order's roll and rubber-width
/// limits. Non-colour pechat uses the broad compatibility result unchanged.
pub fn reroute_order_compatible(
    source: &CanonicalApparatus,
    target: &CanonicalApparatus,
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
pub fn pechat_color_stations(apparatus: &CanonicalApparatus) -> Option<u8> {
    policy_for(apparatus).and_then(PechatPolicy::color_stations)
}

/// Returns the explicit canonical Qolip tooling policy for supported pechat
/// apparatuses. Runtime checkout, custody, in-use, and QR state remain outside
/// this configuration helper.
pub fn requires_qolip_scan(apparatus: &CanonicalApparatus) -> bool {
    policy_for(apparatus).is_some_and(|policy| policy.requires_qolip_scan())
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
    use crate::core::apparatus_standard::{
        ApparatusDisplayMetadata, ApparatusIdentity, CapabilityCode, CapacityConfiguration,
        CatalogSource, MaterialPolicy, OperationalPolicies, Provenance, QueuePolicy,
        TrainingReference, Versioning, aas_package_metadata_for_apparatus,
    };

    fn canonical_apparatus(
        id: &str,
        display_name: &str,
        family: ApparatusFamily,
        kind: ApparatusKind,
        color_stations: Option<u8>,
        tooling: ToolingPolicy,
    ) -> CanonicalApparatus {
        let capabilities = match kind {
            ApparatusKind::ColorPechat => vec![CapabilityCode::Print, CapabilityCode::Pechat],
            ApparatusKind::Flexo => {
                vec![
                    CapabilityCode::Print,
                    CapabilityCode::Pechat,
                    CapabilityCode::Flexo,
                ]
            }
            ApparatusKind::Laminatsiya => vec![CapabilityCode::Laminate],
            _ => vec![CapabilityCode::Apparatus],
        };
        let id = ApparatusId::new(id).unwrap();
        CanonicalApparatus {
            identity: ApparatusIdentity {
                id: id.clone(),
                display: ApparatusDisplayMetadata {
                    display_name: display_name.to_string(),
                    description: String::new(),
                    catalog_order: 1,
                },
            },
            classification: ApparatusClassification {
                family,
                kind,
                color_stations,
            },
            capabilities,
            capability_profiles: Vec::new(),
            policies: OperationalPolicies {
                queue: QueuePolicy::StrictSequence,
                material: MaterialPolicy::default(),
                tooling,
            },
            capacity: CapacityConfiguration {
                capacity_slots: 1,
                setup_minutes: 0,
                cleanup_minutes: 0,
                efficiency_percent: 100,
                finite_capacity: true,
                working_windows: Vec::new(),
            },
            placement: None,
            training: TrainingReference { enabled: true },
            provenance: Provenance {
                source: CatalogSource::Default,
                source_ref: None,
            },
            versioning: Versioning { revision: 1 },
            aas: aas_package_metadata_for_apparatus(&id),
        }
    }

    #[test]
    fn canonical_color_pechat_7_8_9_is_pechat_and_qolip_enabled() {
        for stations in 7..=9 {
            let apparatus = canonical_apparatus(
                &format!("apparatus:test:color-{stations}"),
                &format!("renamed color machine {stations}"),
                ApparatusFamily::Pechat,
                ApparatusKind::ColorPechat,
                Some(stations),
                ToolingPolicy::QolipScanRequired,
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
            ApparatusFamily::Pechat,
            ApparatusKind::Flexo,
            None,
            ToolingPolicy::QolipScanRequired,
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
            ApparatusFamily::Pechat,
            ApparatusKind::ColorPechat,
            Some(8),
            ToolingPolicy::QolipScanRequired,
        );
        let seven_color_target = canonical_apparatus(
            "apparatus:test:color-target-7",
            "target 7",
            ApparatusFamily::Pechat,
            ApparatusKind::ColorPechat,
            Some(7),
            ToolingPolicy::QolipScanRequired,
        );
        let nine_color_target = canonical_apparatus(
            "apparatus:test:color-target-9",
            "target 9",
            ApparatusFamily::Pechat,
            ApparatusKind::ColorPechat,
            Some(9),
            ToolingPolicy::QolipScanRequired,
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
            ApparatusFamily::Laminatsiya,
            ApparatusKind::Laminatsiya,
            None,
            ToolingPolicy::QolipScanRequired,
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
            ApparatusFamily::Pechat,
            ApparatusKind::ColorPechat,
            Some(7),
            ToolingPolicy::QolipScanRequired,
        );
        let original_id = apparatus.identity.id.clone();
        let (original_classification, original_color_stations, original_requires_qolip_scan) = {
            let original_policy = policy_for(&apparatus).expect("ColorPechat policy");
            (
                original_policy.classification().clone(),
                original_policy.color_stations(),
                original_policy.requires_qolip_scan(),
            )
        };
        apparatus.identity.display.display_name = "Laminatsiya renamed".to_string();
        let renamed_policy = policy_for(&apparatus).expect("ColorPechat policy");
        assert_eq!(original_id.as_str(), renamed_policy.apparatus_id().as_str());
        assert_eq!(&original_classification, renamed_policy.classification());
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
