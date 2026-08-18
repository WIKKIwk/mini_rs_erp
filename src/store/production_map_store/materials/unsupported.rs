use crate::core::production_map::{
    ApparatusMaterialRule, ProductionMapError, RawMaterialAssignment,
};

pub(super) async fn apparatus_material_rules()
-> Result<Vec<ApparatusMaterialRule>, ProductionMapError> {
    Err(ProductionMapError::StoreFailed)
}

pub(super) async fn put_apparatus_material_rule(
    _rule: ApparatusMaterialRule,
) -> Result<(), ProductionMapError> {
    Err(ProductionMapError::StoreFailed)
}

pub(super) async fn raw_material_assignments()
-> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
    Err(ProductionMapError::StoreFailed)
}

pub(super) async fn put_raw_material_assignment(
    _assignment: RawMaterialAssignment,
) -> Result<(), ProductionMapError> {
    Err(ProductionMapError::StoreFailed)
}

pub(super) async fn delete_raw_material_assignment(
    _order_id: &str,
    _barcode: &str,
) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
    Err(ProductionMapError::StoreFailed)
}
