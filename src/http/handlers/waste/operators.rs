use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::core::{
    apparatus_standard::ExecutionOperation,
    authz::{capability_code, role_assignment_key},
};

use super::{
    ApiError, AppState, Principal, PrincipalRole, StatusCode, actor_role, error, operator_capability,
};

#[derive(Serialize)]
pub(super) struct Catalog {
    apparatuses: Vec<ApparatusOption>,
    operators: Vec<OperatorOption>,
}

#[derive(Serialize)]
pub(super) struct ApparatusOption {
    id: String,
    name: String,
    #[serde(skip)]
    is_cutting: bool,
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
    tracing::warn!(%cause, "waste bin options lookup failed");
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "waste_options_unavailable",
        "Chiqindi qutisi uchun tanlovlarni yuklab bo‘lmadi",
    )
}

fn access_error() -> ApiError {
    error(
        StatusCode::BAD_REQUEST,
        "waste_access_invalid",
        "Kamida bitta faol apparatni tanlang",
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

pub(super) async fn apparatus_options(state: &AppState) -> Result<Vec<ApparatusOption>, ApiError> {
    let mut apparatuses: Vec<_> = state
        .production_maps
        .active_canonical_apparatuses()
        .await
        .map_err(catalog_error)?
        .into_iter()
        .map(|configuration| ApparatusOption {
            id: configuration.runtime.apparatus_id.to_string(),
            name: configuration.runtime.display.display_name.clone(),
            is_cutting: configuration.runtime.execution_profile.operation
                == ExecutionOperation::Cut,
        })
        .collect();
    apparatuses.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    Ok(apparatuses)
}

pub(super) async fn catalog(state: &AppState) -> Result<Catalog, ApiError> {
    let (apparatuses, assignments) = tokio::try_join!(apparatus_options(state), async {
        state.admin.role_assignments().await.map_err(catalog_error)
    })?;
    let active_ids: BTreeSet<_> = apparatuses.iter().map(|option| option.id.as_str()).collect();
    let cutters: Vec<_> = apparatuses
        .iter()
        .filter(|option| option.is_cutting)
        .map(|option| option.id.clone())
        .collect();

    let mut worker_ids = BTreeSet::new();
    let mut system_ids = BTreeSet::new();
    for assignment in &assignments {
        if !assignment
            .assigned_apparatus
            .iter()
            .any(|id| active_ids.contains(id.as_str()))
        {
            continue;
        }
        let id = &assignment.principal_ref;
        match assignment.principal_role {
            PrincipalRole::Aparatchi | PrincipalRole::Customer => {
                worker_ids.insert(id.clone());
            }
            PrincipalRole::TayyorlovMasteri
            | PrincipalRole::HomashyoRezkachi
            | PrincipalRole::Metalayzer => {
                system_ids.insert(id.clone());
            }
            _ => {}
        }
    }
    let worker_refs: Vec<_> = worker_ids.iter().cloned().collect();
    let system_refs: Vec<_> = system_ids.into_iter().collect();
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
            if cutters.is_empty() {
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
        if !added.insert(role_assignment_key(&principal.role, &principal.ref_)) {
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
            .filter(|id| active_ids.contains(id.as_str()))
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

pub(super) async fn validate_apparatuses(
    state: &AppState,
    ids: &[String],
) -> Result<(), ApiError> {
    if !(1..=128).contains(&ids.len()) {
        return Err(access_error());
    }
    let options = apparatus_options(state).await?;
    if ids
        .iter()
        .any(|id| !options.iter().any(|option| option.id == id.trim()))
    {
        return Err(access_error());
    }
    Ok(())
}
