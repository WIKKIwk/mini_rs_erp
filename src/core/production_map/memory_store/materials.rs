use super::*;

pub(super) async fn apparatus_material_rules(
    store: &MemoryProductionMapStore,
) -> Result<Vec<ApparatusMaterialRule>, ProductionMapError> {
    Ok(store
        .material_rules
        .read()
        .await
        .values()
        .cloned()
        .collect())
}

pub(super) async fn put_apparatus_material_rule(
    store: &MemoryProductionMapStore,
    rule: ApparatusMaterialRule,
) -> Result<(), ProductionMapError> {
    store
        .material_rules
        .write()
        .await
        .insert(rule.apparatus.to_lowercase(), rule);
    Ok(())
}

pub(super) async fn raw_material_assignments(
    store: &MemoryProductionMapStore,
) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
    Ok(store
        .material_assignments
        .read()
        .await
        .values()
        .cloned()
        .collect())
}

pub(super) async fn put_raw_material_assignment(
    store: &MemoryProductionMapStore,
    assignment: RawMaterialAssignment,
) -> Result<(), ProductionMapError> {
    store
        .material_assignments
        .write()
        .await
        .insert(assignment.barcode.to_uppercase(), assignment);
    Ok(())
}
