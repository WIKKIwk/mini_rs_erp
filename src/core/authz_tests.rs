use super::*;
use crate::core::auth::models::PrincipalRole;

#[test]
fn aparatchi_system_role_assigns_to_customer_principal() {
    let roles = system_role_definitions();
    let role = roles
        .iter()
        .find(|role| role.id == "aparatchi")
        .expect("aparatchi role");
    assert_eq!(role.base_role, None);

    let assignment = normalize_role_assignment(
        RoleAssignmentUpsert {
            principal_role: PrincipalRole::Customer,
            principal_ref: "CUS-1".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec![" Godex aparat ".to_string()],
            assigned_item_groups: Vec::new(),
        },
        &roles,
    )
    .expect("aparatchi assignment");

    assert_eq!(assignment.principal_role, PrincipalRole::Customer);
    assert_eq!(assignment.principal_ref, "CUS-1");
    assert_eq!(assignment.role_id, "aparatchi");
    assert_eq!(assignment.assigned_apparatus, vec!["Godex aparat"]);
}

#[test]
fn aparatchi_system_role_assigns_to_aparatchi_principal() {
    let roles = system_role_definitions();
    let assignment = normalize_role_assignment(
        RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "aparatchi - 4".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat - A".to_string()],
            assigned_item_groups: Vec::new(),
        },
        &roles,
    )
    .expect("aparatchi assignment");

    assert_eq!(assignment.principal_role, PrincipalRole::Aparatchi);
    assert_eq!(assignment.principal_ref, "aparatchi - 4");
}

#[test]
fn qolipchi_system_role_has_qolip_capability() {
    let roles = system_role_definitions();
    let role = roles
        .iter()
        .find(|role| role.id == "qolipchi")
        .expect("qolipchi role");

    assert_eq!(role.base_role, Some(PrincipalRole::Qolipchi));
    assert!(
        role.capability_codes
            .iter()
            .any(|code| code == "qolip.manage")
    );
}

#[test]
fn material_taminotchi_system_role_has_raw_material_and_gscale_capabilities() {
    let roles = system_role_definitions();
    let role = roles
        .iter()
        .find(|role| role.id == "material_taminotchi")
        .expect("material taminotchi role");

    assert_eq!(role.base_role, Some(PrincipalRole::MaterialTaminotchi));
    for expected in [
        "gscale.catalog.read",
        "gscale.print",
        "rps.batch.manage",
        "catalog.item.create",
        "raw_material.assign",
    ] {
        assert!(
            role.capability_codes.iter().any(|code| code == expected),
            "missing capability {expected}"
        );
    }
    assert!(
        !role
            .capability_codes
            .iter()
            .any(|code| code == "admin.access")
    );
}

#[test]
fn material_taminotchi_assignment_keeps_item_group_scope() {
    let assignment = normalize_role_assignment(
        RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-1".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec![
                " Kraska ".to_string(),
                "Kley".to_string(),
                "Kraska".to_string(),
            ],
        },
        &system_role_definitions(),
    )
    .expect("material assignment");

    assert_eq!(assignment.principal_role, PrincipalRole::MaterialTaminotchi);
    assert_eq!(assignment.role_id, "material_taminotchi");
    assert_eq!(assignment.assigned_item_groups, vec!["Kley", "Kraska"]);
}

#[test]
fn material_taminotchi_assignment_requires_item_group_scope() {
    let error = normalize_role_assignment(
        RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-1".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: Vec::new(),
        },
        &system_role_definitions(),
    )
    .expect_err("material assignment needs item groups");

    assert_eq!(error, RoleAssignmentError::MissingAssignedItemGroups);
}

#[test]
fn core_system_role_rejects_wrong_principal_role() {
    let error = normalize_role_assignment(
        RoleAssignmentUpsert {
            principal_role: PrincipalRole::Customer,
            principal_ref: "CUS-1".to_string(),
            role_id: "werka".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: Vec::new(),
        },
        &system_role_definitions(),
    )
    .expect_err("werka customer assignment must fail");

    assert_eq!(error, RoleAssignmentError::RoleBaseMismatch);
}
