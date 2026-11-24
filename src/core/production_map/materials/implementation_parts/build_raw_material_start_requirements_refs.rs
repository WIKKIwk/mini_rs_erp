pub(super) fn build_raw_material_start_requirements_refs(
    rule: Option<&ApparatusMaterialRule>,
    assignments: &[&RawMaterialAssignment],
    state_material_barcodes: &[String],
    scanned: &BTreeSet<String>,
) -> RawMaterialStartRequirements {
    let policy = rule.map(|rule| rule.start_policy).unwrap_or_default();
    let requires_material = rule.is_some_and(|rule| rule.requires_material);
    let assigned = assignments
        .iter()
        .map(|assignment| normalize_barcode(&assignment.barcode))
        .filter(|barcode| !barcode.is_empty())
        .collect::<BTreeSet<_>>();
    let staged = state_material_barcodes
        .iter()
        .map(|barcode| normalize_barcode(barcode))
        .filter(|barcode| assigned.contains(barcode))
        .collect::<BTreeSet<_>>();
    let requirement_groups = rule.map(effective_requirement_groups).unwrap_or_default();
    let assignments_satisfied = if assignments.is_empty() {
        !requires_material
    } else {
        !requires_material
            || policy != RawMaterialStartPolicy::RequirementGroups
            || rule.is_some_and(|rule| material_requirements_met_refs(rule, assignments))
    };
    let (eligible, required_scan_count, matched_scan_count, scan_satisfied) = match policy {
        RawMaterialStartPolicy::StateAll => {
            let matched = scanned.intersection(&staged).count();
            (
                staged.clone(),
                staged.len(),
                matched,
                assignments.is_empty() && !requires_material
                    || !assignments.is_empty()
                        && !scanned.is_empty()
                        && scanned.is_subset(&assigned)
                        && *scanned == staged,
            )
        }
        RawMaterialStartPolicy::RequirementGroups => {
            let scanned_assignments = assignments
                .iter()
                .copied()
                .filter(|assignment| scanned.contains(&normalize_barcode(&assignment.barcode)))
                .collect::<Vec<_>>();
            let required = rule
                .map(material_requirement_slot_count)
                .unwrap_or_default();
            let matched = rule
                .map(|rule| material_requirement_match_count_refs(rule, &scanned_assignments))
                .unwrap_or_default();
            (
                assigned.clone(),
                required,
                matched,
                assignments.is_empty() && !requires_material
                    || !assignments.is_empty()
                        && scanned.is_subset(&assigned)
                        && required > 0
                        && matched == required,
            )
        }
    };
    RawMaterialStartRequirements {
        policy,
        requires_material,
        material_scan_required: requires_material || !assignments.is_empty(),
        requirement_groups,
        assigned_barcodes: assigned.into_iter().collect(),
        staged_barcodes: staged.into_iter().collect(),
        eligible_barcodes: eligible.into_iter().collect(),
        required_scan_count,
        matched_scan_count,
        assignments_satisfied,
        scan_satisfied,
    }
}
