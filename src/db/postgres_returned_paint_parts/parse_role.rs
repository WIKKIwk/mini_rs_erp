fn parse_role(value: &str) -> Result<PrincipalRole, ReturnedPaintError> {
    match value.trim() {
        "supplier" => Ok(PrincipalRole::Supplier),
        "werka" | "omborchi" => Ok(PrincipalRole::Werka),
        "customer" => Ok(PrincipalRole::Customer),
        "aparatchi" => Ok(PrincipalRole::Aparatchi),
        "qolipchi" => Ok(PrincipalRole::Qolipchi),
        "boyoqchi" => Ok(PrincipalRole::Boyoqchi),
        "tayyorlov_masteri" => Ok(PrincipalRole::TayyorlovMasteri),
        "homashyo_rezkachi" => Ok(PrincipalRole::HomashyoRezkachi),
        "sales_manager" => Ok(PrincipalRole::SalesManager),
        "brigader" => Ok(PrincipalRole::Brigader),
        "metalayzer" => Ok(PrincipalRole::Metalayzer),
        "chiqindi_qabulchi" => Ok(PrincipalRole::ChiqindiQabulchi),
        "material_taminotchi" => Ok(PrincipalRole::MaterialTaminotchi),
        "admin" => Ok(PrincipalRole::Admin),
        _ => Err(ReturnedPaintError::StoreFailed),
    }
}
