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
        },
        &roles,
    )
    .expect("aparatchi assignment");

    assert_eq!(assignment.principal_role, PrincipalRole::Aparatchi);
    assert_eq!(assignment.principal_ref, "aparatchi - 4");
}

#[test]
fn core_system_role_rejects_wrong_principal_role() {
    let error = normalize_role_assignment(
        RoleAssignmentUpsert {
            principal_role: PrincipalRole::Customer,
            principal_ref: "CUS-1".to_string(),
            role_id: "werka".to_string(),
            assigned_apparatus: Vec::new(),
        },
        &system_role_definitions(),
    )
    .expect_err("werka customer assignment must fail");

    assert_eq!(error, RoleAssignmentError::RoleBaseMismatch);
}
