use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::core::apparatus_standard::{ApparatusId, canonical_factory_apparatus_id_for_legacy};
use crate::core::authz::{
    RoleAssignment, RoleDefinition, RoleDefinitionStorePort, RoleStoreError, role_assignment_key,
};
use crate::store::json_file::{read_map, write_pretty};

#[derive(Clone)]
pub struct RoleDefinitionStore {
    path: PathBuf,
    state: Arc<Mutex<RoleDefinitionStoreState>>,
}

#[derive(Default)]
struct RoleDefinitionStoreState {
    loaded: bool,
    roles: BTreeMap<String, RoleDefinition>,
    assignments: BTreeMap<String, RoleAssignment>,
}

#[derive(Default, Serialize, Deserialize)]
struct RoleDefinitionStoreFile {
    #[serde(default)]
    roles: BTreeMap<String, RoleDefinition>,
    #[serde(default)]
    assignments: BTreeMap<String, RoleAssignment>,
}

impl RoleDefinitionStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            state: Arc::new(Mutex::new(RoleDefinitionStoreState::default())),
        }
    }
}

#[async_trait]
impl RoleDefinitionStorePort for RoleDefinitionStore {
    async fn role_definitions(&self) -> Result<Vec<RoleDefinition>, RoleStoreError> {
        let mut state = self.state.lock().await;
        load_if_needed(&self.path, &mut state).await?;
        Ok(state.roles.values().cloned().collect())
    }

    async fn put_role_definition(&self, role: RoleDefinition) -> Result<(), RoleStoreError> {
        let mut state = self.state.lock().await;
        load_if_needed(&self.path, &mut state).await?;
        state.roles.insert(role.id.clone(), role);
        save(&self.path, &state).await
    }

    async fn role_assignments(&self) -> Result<Vec<RoleAssignment>, RoleStoreError> {
        let mut state = self.state.lock().await;
        load_if_needed(&self.path, &mut state).await?;
        Ok(state.assignments.values().cloned().collect())
    }

    async fn put_role_assignment(&self, assignment: RoleAssignment) -> Result<(), RoleStoreError> {
        let mut state = self.state.lock().await;
        load_if_needed(&self.path, &mut state).await?;
        let assignment = canonicalize_assignment(assignment)?;
        state.assignments.insert(
            role_assignment_key(&assignment.principal_role, &assignment.principal_ref),
            assignment,
        );
        save(&self.path, &state).await
    }

    async fn delete_role_assignment(
        &self,
        role: &crate::core::auth::models::PrincipalRole,
        ref_: &str,
    ) -> Result<(), RoleStoreError> {
        let mut state = self.state.lock().await;
        load_if_needed(&self.path, &mut state).await?;
        state.assignments.remove(&role_assignment_key(role, ref_));
        save(&self.path, &state).await
    }
}

async fn load_if_needed(
    path: &Path,
    state: &mut RoleDefinitionStoreState,
) -> Result<(), RoleStoreError> {
    if state.loaded {
        return Ok(());
    }
    match tokio::fs::metadata(path).await {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            tracing::error!(path = %path.display(), "role store path is not a regular file");
            return Err(RoleStoreError::StoreFailed);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            state.loaded = true;
            return Ok(());
        }
        Err(error) => {
            tracing::error!(path = %path.display(), %error, "failed to inspect role store snapshot");
            return Err(RoleStoreError::StoreFailed);
        }
    }
    let raw = tokio::fs::read(path).await.map_err(|error| {
        tracing::error!(path = %path.display(), %error, "failed to read role store snapshot");
        RoleStoreError::StoreFailed
    })?;
    let value = serde_json::from_slice::<serde_json::Value>(&raw).map_err(|error| {
        tracing::error!(path = %path.display(), %error, "invalid role store snapshot");
        RoleStoreError::StoreFailed
    })?;
    let current_shape = value
        .as_object()
        .map(|object| object.contains_key("roles") || object.contains_key("assignments"))
        .unwrap_or(false);
    if current_shape {
        let file: RoleDefinitionStoreFile = serde_json::from_value(value).map_err(|error| {
            tracing::error!(path = %path.display(), %error, "invalid role store schema");
            RoleStoreError::StoreFailed
        })?;
        state.roles = file.roles;
        let mut migrated = false;
        let mut assignments = BTreeMap::new();
        for (key, assignment) in file.assignments {
            let (assignment, assignment_migrated) = migrate_legacy_assignment(assignment)?;
            migrated |= assignment_migrated;
            assignments.insert(key, assignment);
        }
        state.assignments = assignments;
        if migrated {
            backup_before_canonical_migration(path).await?;
            save(path, state).await?;
            tracing::info!(
                path = %path.display(),
                "migrated legacy role apparatus assignments to canonical IDs"
            );
        }
    } else {
        state.roles = read_map::<RoleDefinition>(path)
            .await
            .map_err(|_| RoleStoreError::StoreFailed)?
            .into_iter()
            .collect();
    }
    state.loaded = true;
    Ok(())
}

fn migrate_legacy_assignment(
    mut assignment: RoleAssignment,
) -> Result<(RoleAssignment, bool), RoleStoreError> {
    let mut migrated = false;
    let mut canonical = Vec::with_capacity(assignment.assigned_apparatus.len());
    for original in std::mem::take(&mut assignment.assigned_apparatus) {
        let value = original.trim();
        let id = match canonical_factory_apparatus_id_for_legacy(value) {
            Some(id) => {
                migrated |= id.as_str() != value;
                id
            }
            None => parse_canonical_apparatus_id(&assignment, value)?,
        };
        canonical.push(id.as_str().to_string());
    }
    canonical.sort();
    canonical.dedup();
    assignment.assigned_apparatus = canonical;
    Ok((assignment, migrated))
}

fn canonicalize_assignment(
    mut assignment: RoleAssignment,
) -> Result<RoleAssignment, RoleStoreError> {
    let mut canonical = Vec::with_capacity(assignment.assigned_apparatus.len());
    for value in std::mem::take(&mut assignment.assigned_apparatus) {
        let value = value.trim();
        let id = parse_canonical_apparatus_id(&assignment, value)?;
        canonical.push(id.as_str().to_string());
    }
    canonical.sort();
    canonical.dedup();
    assignment.assigned_apparatus = canonical;
    Ok(assignment)
}

fn parse_canonical_apparatus_id(
    assignment: &RoleAssignment,
    value: &str,
) -> Result<ApparatusId, RoleStoreError> {
    ApparatusId::new(value.to_string()).map_err(|_| {
        tracing::error!(
            role = ?assignment.principal_role,
            principal_ref = %assignment.principal_ref,
            apparatus = %value,
            "role store contains a non-canonical apparatus identity"
        );
        RoleStoreError::StoreFailed
    })
}

async fn backup_before_canonical_migration(path: &Path) -> Result<(), RoleStoreError> {
    let backup_path = path.with_extension("pre-canonical-apparatus.json");
    match tokio::fs::hard_link(path, &backup_path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => {
            tracing::error!(
                path = %path.display(),
                backup_path = %backup_path.display(),
                %error,
                "failed to back up legacy role store before canonical migration"
            );
            Err(RoleStoreError::StoreFailed)
        }
    }
}

async fn save(path: &Path, state: &RoleDefinitionStoreState) -> Result<(), RoleStoreError> {
    write_pretty(
        path,
        &RoleDefinitionStoreFile {
            roles: state.roles.clone(),
            assignments: state.assignments.clone(),
        },
    )
    .await
    .map_err(|_| RoleStoreError::StoreFailed)
}

#[cfg(test)]
mod tests {
    use crate::core::auth::models::PrincipalRole;
    use crate::core::authz::{
        RoleAssignment, RoleDefinition, RoleDefinitionStorePort, RoleStoreError,
    };

    use super::RoleDefinitionStore;

    #[tokio::test]
    async fn role_definition_store_persists_custom_roles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("roles.json");
        let store = RoleDefinitionStore::new(path.clone());

        store
            .put_role_definition(RoleDefinition {
                id: "scale_operator".to_string(),
                label: "Scale operator".to_string(),
                base_role: None,
                capability_codes: vec!["gscale.print".to_string()],
                system: false,
            })
            .await
            .expect("put role");
        drop(store);

        let reloaded = RoleDefinitionStore::new(path);
        let roles = reloaded.role_definitions().await.expect("role definitions");
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].id, "scale_operator");
        assert_eq!(roles[0].capability_codes, vec!["gscale.print"]);
    }

    #[tokio::test]
    async fn role_definition_store_persists_assignments_with_roles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("roles.json");
        let store = RoleDefinitionStore::new(path.clone());

        store
            .put_role_definition(RoleDefinition {
                id: "catalog_only".to_string(),
                label: "Catalog only".to_string(),
                base_role: None,
                capability_codes: vec!["gscale.catalog.read".to_string()],
                system: false,
            })
            .await
            .expect("put role");
        store
            .put_role_assignment(RoleAssignment {
                principal_role: PrincipalRole::Werka,
                principal_ref: "werka".to_string(),
                role_id: "catalog_only".to_string(),
                assigned_apparatus: vec!["apparatus:catalog:godex-demo-001".to_string()],
                assigned_item_groups: Vec::new(),
            })
            .await
            .expect("put assignment");
        drop(store);

        let reloaded = RoleDefinitionStore::new(path);
        assert_eq!(
            reloaded
                .role_definitions()
                .await
                .expect("roles")
                .first()
                .expect("role")
                .id,
            "catalog_only"
        );
        assert_eq!(
            reloaded
                .role_assignments()
                .await
                .expect("assignments")
                .first()
                .expect("assignment")
                .role_id,
            "catalog_only"
        );
    }

    #[tokio::test]
    async fn role_definition_store_does_not_reload_title_as_apparatus_scope() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("roles.json");
        tokio::fs::write(
            &path,
            serde_json::json!({
                "roles": {},
                "assignments": {
                    "aparatchi:worker-1": {
                        "principal_role": "aparatchi",
                        "principal_ref": "worker-1",
                        "role_id": "aparatchi",
                        "assigned_apparatus": ["Renamed laminator"],
                        "assigned_item_groups": []
                    }
                }
            })
            .to_string(),
        )
        .await
        .expect("write legacy assignment");

        let store = RoleDefinitionStore::new(path);
        assert!(matches!(
            store.role_assignments().await,
            Err(RoleStoreError::StoreFailed)
        ));
    }

    #[tokio::test]
    async fn role_definition_store_migrates_factory_aliases_once_and_preserves_backup() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mobile_roles.json");
        let original = serde_json::to_vec_pretty(&serde_json::json!({
            "roles": {},
            "assignments": {
                "aparatchi:worker-1": {
                    "principal_role": "aparatchi",
                    "principal_ref": "worker-1",
                    "role_id": "aparatchi",
                    "assigned_apparatus": [
                        "Extruder laminatsiya",
                        "apparatus:default:extruder_laminatsiya",
                        "Flexo pechat",
                        "apparatus:default:flexo_pechat",
                        "Laminatsiya 1",
                        "apparatus:default:laminatsiya_1",
                        "Laminatsiya 2",
                        "apparatus:default:laminatsiya_2",
                        "Rezka",
                        "apparatus:default:rezka",
                        "7 ta rangli pechat",
                        "8 ta rangli pechat",
                        "9 ta rangli pechat",
                        "Holodniy kley aparat",
                        "Paket aparat"
                    ],
                    "assigned_item_groups": []
                }
            }
        }))
        .expect("serialize legacy role store");
        tokio::fs::write(&path, &original)
            .await
            .expect("write legacy role store");

        let store = RoleDefinitionStore::new(path.clone());
        let assignments = store
            .role_assignments()
            .await
            .expect("migrate legacy apparatus scopes");
        assert_eq!(assignments.len(), 1);
        assert_eq!(
            assignments[0].assigned_apparatus,
            vec![
                "apparatus:default:asset-004",
                "apparatus:default:asset-005",
                "apparatus:default:asset-007",
                "apparatus:default:asset-008",
                "apparatus:default:asset-010",
                "apparatus:default:bosma_7",
                "apparatus:default:bosma_8",
                "apparatus:default:bosma_9",
                "apparatus:default:holodniy_kley",
                "apparatus:default:paket",
            ]
        );

        let backup_path = path.with_extension("pre-canonical-apparatus.json");
        assert_eq!(
            tokio::fs::read(&backup_path)
                .await
                .expect("read role store backup"),
            original
        );
        let migrated = tokio::fs::read(&path)
            .await
            .expect("read migrated role store");
        drop(store);

        let restarted = RoleDefinitionStore::new(path.clone());
        restarted
            .role_assignments()
            .await
            .expect("reload canonical role store");
        assert_eq!(
            tokio::fs::read(&path)
                .await
                .expect("read role store after restart"),
            migrated
        );
        assert_eq!(
            tokio::fs::read(backup_path)
                .await
                .expect("read preserved role store backup"),
            original
        );
    }

    #[tokio::test]
    async fn role_definition_store_reads_legacy_role_map_shape() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("roles.json");
        tokio::fs::write(
            &path,
            r#"{
                "scale_operator": {
                    "id": "scale_operator",
                    "label": "Scale operator",
                    "base_role": "werka",
                    "capability_codes": ["gscale.print"],
                    "system": false
                }
            }"#,
        )
        .await
        .expect("write legacy roles");

        let store = RoleDefinitionStore::new(path);
        let roles = store.role_definitions().await.expect("legacy roles");

        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].id, "scale_operator");
        assert!(roles[0].base_role.is_some());
        assert_eq!(roles[0].capability_codes, vec!["gscale.print"]);
    }
}
