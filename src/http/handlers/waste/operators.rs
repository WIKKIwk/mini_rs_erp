use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::core::{
    apparatus_standard::ExecutionOperation,
    authz::{capability_code, role_assignment_key},
};

use super::{
    ApiError, AppState, Principal, PrincipalRole, StatusCode, WasteBinAccessInput,
    WasteBinOperator, actor_role, error, operator_capability,
};

#[derive(Serialize)]
pub(super) struct Catalog {
    apparatuses: Vec<ApparatusOption>,
    operators: Vec<OperatorOption>,
}

#[derive(Serialize)]
struct ApparatusOption {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct OperatorOption {
    role: &'static str,
    #[serde(rename = "ref")]
    ref_: String,
    display_name: String,
    apparatus_ids: Vec<String>,
}

fn catalog_error(cause: impl std::fmt::Display) -> ApiError {
    tracing::warn!(%cause, "waste bin operator options lookup failed");
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "waste_options_unavailable",
        "Apparat va aparatchilar ro‘yxatini yuklab bo‘lmadi",
    )
}

fn access_error() -> ApiError {
    error(
        StatusCode::BAD_REQUEST,
        "waste_access_invalid",
        "Tanlangan har bir apparatga kamida bitta mos aparatchi biriktiring",
    )
}

fn subject(role: PrincipalRole, ref_: String, display_name: String) -> Principal {
    Principal {
        role,
        ref_,
        display_name,
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    }
}

pub(super) async fn catalog(
    state: &AppState,
    selected: Option<&[WasteBinOperator]>,
) -> Result<Catalog, ApiError> {
    let wanted = |role, ref_: &str| {
        selected.is_none_or(|items| {
            items
                .iter()
                .any(|item| item.role.trim() == actor_role(role) && item.r#ref.trim() == ref_)
        })
    };
    let (configurations, assignments) = tokio::try_join!(
        async {
            state
                .production_maps
                .active_canonical_apparatuses()
                .await
                .map_err(catalog_error)
        },
        async { state.admin.role_assignments().await.map_err(catalog_error) }
    )?;
    let mut apparatuses = Vec::new();
    let mut cutters = Vec::new();
    let mut active_ids = BTreeSet::new();
    for configuration in configurations {
        let runtime = &configuration.runtime;
        let id = runtime.apparatus_id.to_string();
        if runtime.execution_profile.operation == ExecutionOperation::Cut {
            cutters.push(id.clone());
        }
        active_ids.insert(id.clone());
        apparatuses.push(ApparatusOption {
            id,
            name: runtime.display.display_name.clone(),
        });
    }
    apparatuses.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));

    let mut worker_ids = BTreeSet::new();
    let mut system_ids = BTreeSet::new();
    for assignment in &assignments {
        if !assignment
            .assigned_apparatus
            .iter()
            .any(|id| active_ids.contains(id))
        {
            continue;
        }
        let id = &assignment.principal_ref;
        match assignment.principal_role {
            PrincipalRole::Aparatchi | PrincipalRole::Customer
                if wanted(PrincipalRole::Aparatchi, id) =>
            {
                worker_ids.insert(id.clone());
            }
            role @ (PrincipalRole::TayyorlovMasteri
            | PrincipalRole::HomashyoRezkachi
            | PrincipalRole::Metalayzer)
                if wanted(role, id) =>
            {
                system_ids.insert(id.clone());
            }
            _ => {}
        }
    }
    let worker_refs: Vec<_> = worker_ids.iter().cloned().collect();
    let mut system_refs: Vec<_> = system_ids.into_iter().collect();
    // A Rezka master is effectively assigned to all active cutting machines.
    // Look up selected IDs directly when validating a write, even beyond the
    // normal directory limit used for presenting choices.
    if !cutters.is_empty()
        && let Some(selected) = selected
    {
        system_refs.extend(
            selected
                .iter()
                .filter(|item| item.role.trim() == actor_role(PrincipalRole::RezkaMaster))
                .map(|item| item.r#ref.trim().to_owned()),
        );
    }
    let (workers, mut system_users, masters) = tokio::try_join!(
        async {
            if worker_refs.is_empty() {
                return Ok(Vec::new());
            }
            state
                .workers
                .workers_by_ids(&worker_refs)
                .await
                .map_err(catalog_error)
        },
        async {
            if system_refs.is_empty() {
                return Ok(Vec::new());
            }
            state
                .system_users
                .users_by_ids(&system_refs)
                .await
                .map_err(catalog_error)
        },
        async {
            if selected.is_some() || cutters.is_empty() {
                return Ok(Vec::new());
            }
            state
                .system_users
                .users(&PrincipalRole::RezkaMaster, "", 500)
                .await
                .map_err(catalog_error)
        },
    )?;
    system_users.extend(masters);
    let mut subjects = Vec::new();
    for worker in workers {
        worker_ids.remove(&worker.id);
        subjects.push(subject(PrincipalRole::Aparatchi, worker.id, worker.name));
    }
    // Some pre-native Aparatchi accounts still authenticate via the customer
    // directory. Only matching assigned identities enter this lookup.
    for id in worker_ids {
        if let Ok(Some(entry)) = state
            .admin
            .user_list_entry_for_principal(&PrincipalRole::Customer, &id)
            .await
        {
            subjects.push(subject(
                PrincipalRole::Aparatchi,
                entry.entity_ref,
                entry.name,
            ));
        }
    }
    subjects.extend(
        system_users
            .into_iter()
            .map(|user| subject(user.role, user.id, user.name)),
    );
    let assignments: BTreeMap<_, _> = assignments
        .iter()
        .map(|row| {
            (
                role_assignment_key(&row.principal_role, &row.principal_ref),
                row,
            )
        })
        .collect();
    let mut operators = Vec::new();
    let mut added = BTreeSet::new();
    for principal in subjects {
        if !wanted(principal.role, &principal.ref_)
            || !added.insert(role_assignment_key(&principal.role, &principal.ref_))
        {
            continue;
        }
        let scope = if principal.role == PrincipalRole::RezkaMaster {
            &cutters
        } else {
            let assignment = assignments
                .get(&role_assignment_key(&principal.role, &principal.ref_))
                .or_else(|| {
                    (principal.role == PrincipalRole::Aparatchi)
                        .then(|| {
                            assignments.get(&role_assignment_key(
                                &PrincipalRole::Customer,
                                &principal.ref_,
                            ))
                        })
                        .flatten()
                });
            let Some(assignment) = assignment else {
                continue;
            };
            &assignment.assigned_apparatus
        };
        let apparatus_ids: Vec<_> = scope
            .iter()
            .filter(|id| active_ids.contains(*id))
            .cloned()
            .collect();
        let (Some(read), Some(write)) = (
            operator_capability(principal.role, false).and_then(capability_code),
            operator_capability(principal.role, true).and_then(capability_code),
        ) else {
            continue;
        };
        if apparatus_ids.is_empty() {
            continue;
        }
        let capabilities = state.admin.principal_capability_codes(&principal).await;
        if !capabilities.iter().any(|code| code == read)
            || !capabilities.iter().any(|code| code == write)
        {
            continue;
        }
        operators.push(OperatorOption {
            role: actor_role(principal.role),
            ref_: principal.ref_,
            display_name: principal.display_name,
            apparatus_ids,
        });
    }
    operators.sort_by(|left, right| {
        left.display_name
            .cmp(&right.display_name)
            .then(left.ref_.cmp(&right.ref_))
    });
    Ok(Catalog {
        apparatuses,
        operators,
    })
}

pub(super) async fn validate_access(
    state: &AppState,
    input: &WasteBinAccessInput,
) -> Result<(), ApiError> {
    if !(1..=128).contains(&input.apparatus_ids.len())
        || !(1..=128).contains(&input.operators.len())
    {
        return Err(access_error());
    }
    let selected: BTreeSet<_> = input.apparatus_ids.iter().map(|id| id.trim()).collect();
    let catalog = catalog(state, Some(&input.operators)).await?;
    let mut covered = BTreeSet::new();
    for operator in &input.operators {
        let entry = catalog
            .operators
            .iter()
            .find(|entry| entry.role == operator.role.trim() && entry.ref_ == operator.r#ref.trim())
            .ok_or_else(access_error)?;
        let assigned: BTreeSet<_> = entry
            .apparatus_ids
            .iter()
            .map(String::as_str)
            .filter(|id| selected.contains(id))
            .collect();
        if assigned.is_empty() {
            return Err(access_error());
        }
        covered.extend(assigned);
    }
    if covered != selected {
        return Err(access_error());
    }
    Ok(())
}
