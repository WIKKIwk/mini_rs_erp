use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use data_encoding::HEXLOWER;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::core::apparatus_groups::{
    ApparatusCatalogEntry, ApparatusGroupError, ApparatusGroupService, ApparatusMasterData,
    ApparatusSource,
};
use crate::core::apparatus_standard::ApparatusId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactoryLocationApparatus {
    pub id: ApparatusId,
    pub name: String,
    pub source: ApparatusSource,
    pub sort_order: usize,
    #[serde(flatten, default)]
    pub master: ApparatusMasterData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactoryLocation {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub apparatus: Vec<FactoryLocationApparatus>,
    pub created_at_unix: i64,
    pub updated_at_unix: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct FactoryLocationCreate {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub apparatus_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct FactoryLocationUpdate {
    pub name: Option<String>,
    pub active: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct FactoryLocationApparatusReplace {
    #[serde(default)]
    pub apparatus_ids: Vec<String>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FactoryLocationError {
    #[error("state name is required")]
    MissingName,
    #[error("state update is required")]
    MissingUpdate,
    #[error("apparatus id is invalid")]
    InvalidApparatus,
    #[error("state name already exists")]
    DuplicateName,
    #[error("state not found")]
    NotFound,
    #[error("factory location store failed")]
    StoreFailed,
}

#[async_trait]
pub trait FactoryLocationStorePort: Send + Sync {
    async fn list(&self) -> Result<Vec<FactoryLocation>, FactoryLocationError>;
    async fn create(
        &self,
        id: &str,
        name: &str,
        apparatus: &[FactoryLocationApparatus],
    ) -> Result<FactoryLocation, FactoryLocationError>;
    async fn update(
        &self,
        id: &str,
        name: Option<&str>,
        active: Option<bool>,
    ) -> Result<FactoryLocation, FactoryLocationError>;
    async fn replace_apparatus(
        &self,
        id: &str,
        apparatus: &[FactoryLocationApparatus],
    ) -> Result<FactoryLocation, FactoryLocationError>;
}

#[derive(Clone)]
pub struct FactoryLocationService {
    store: Arc<dyn FactoryLocationStorePort>,
    apparatus_groups: ApparatusGroupService,
}

impl FactoryLocationService {
    pub fn new(
        store: Arc<dyn FactoryLocationStorePort>,
        apparatus_groups: ApparatusGroupService,
    ) -> Self {
        Self {
            store,
            apparatus_groups,
        }
    }

    pub async fn list(&self) -> Result<Vec<FactoryLocation>, FactoryLocationError> {
        let mut locations = self.store.list().await?;
        self.refresh_apparatus_snapshots(&mut locations).await?;
        Ok(locations)
    }

    pub async fn create(
        &self,
        input: FactoryLocationCreate,
    ) -> Result<FactoryLocation, FactoryLocationError> {
        let name = required_name(&input.name)?;
        let apparatus = self.resolve_apparatus(input.apparatus_ids).await?;
        let id = format!("state_{}", HEXLOWER.encode(&rand::random::<[u8; 16]>()));
        self.store.create(&id, &name, &apparatus).await
    }

    pub async fn update(
        &self,
        id: &str,
        input: FactoryLocationUpdate,
    ) -> Result<FactoryLocation, FactoryLocationError> {
        let id = required_id(id)?;
        if input.name.is_none() && input.active.is_none() {
            return Err(FactoryLocationError::MissingUpdate);
        }
        let name = input.name.as_deref().map(required_name).transpose()?;
        let mut location = self.store.update(id, name.as_deref(), input.active).await?;
        self.refresh_apparatus_snapshots(std::slice::from_mut(&mut location))
            .await?;
        Ok(location)
    }

    pub async fn replace_apparatus(
        &self,
        id: &str,
        input: FactoryLocationApparatusReplace,
    ) -> Result<FactoryLocation, FactoryLocationError> {
        let id = required_id(id)?;
        let apparatus = self.resolve_apparatus(input.apparatus_ids).await?;
        let mut location = self.store.replace_apparatus(id, &apparatus).await?;
        self.refresh_apparatus_snapshots(std::slice::from_mut(&mut location))
            .await?;
        Ok(location)
    }

    async fn refresh_apparatus_snapshots(
        &self,
        locations: &mut [FactoryLocation],
    ) -> Result<(), FactoryLocationError> {
        if locations
            .iter()
            .all(|location| location.apparatus.is_empty())
        {
            return Ok(());
        }
        let catalog = self
            .apparatus_groups
            .apparatus_catalog("", 10_000)
            .await
            .map_err(map_apparatus_error)?;
        let catalog_by_id = catalog
            .into_iter()
            .map(|entry| {
                let id = ApparatusId::new(entry.id.clone())
                    .map_err(|_| FactoryLocationError::InvalidApparatus)?;
                Ok((id, entry))
            })
            .collect::<Result<BTreeMap<_, _>, FactoryLocationError>>()?;
        for location in locations {
            for apparatus in &mut location.apparatus {
                let Some(entry) = catalog_by_id.get(&apparatus.id) else {
                    return Err(FactoryLocationError::InvalidApparatus);
                };
                apparatus.name = entry.name.clone();
                apparatus.source = entry.source;
                apparatus.sort_order = entry.sort_order;
                apparatus.master = entry.master.clone();
            }
        }
        Ok(())
    }

    async fn resolve_apparatus(
        &self,
        ids: Vec<String>,
    ) -> Result<Vec<FactoryLocationApparatus>, FactoryLocationError> {
        let mut requested = BTreeSet::new();
        for id in ids {
            let id = ApparatusId::new(id.trim().to_string())
                .map_err(|_| FactoryLocationError::InvalidApparatus)?;
            requested.insert(id);
        }
        if requested.is_empty() {
            return Ok(Vec::new());
        }
        for apparatus_id in &requested {
            if self
                .apparatus_groups
                .canonical_apparatus_by_id(apparatus_id)
                .await
                .map_err(map_apparatus_error)?
                .is_none()
            {
                // Catalog/master projections and display snapshots are not
                // sufficient to create live placement configuration.
                return Err(FactoryLocationError::InvalidApparatus);
            }
        }
        let catalog = self
            .apparatus_groups
            .apparatus_catalog("", 10_000)
            .await
            .map_err(map_apparatus_error)?;
        let mut selected = Vec::new();
        for item in catalog {
            let Ok(id) = ApparatusId::new(item.id.trim().to_string()) else {
                continue;
            };
            if requested.remove(&id) {
                selected.push(factory_location_apparatus(item, id));
            }
        }
        if !requested.is_empty() {
            return Err(FactoryLocationError::InvalidApparatus);
        }
        selected.sort_by(|left, right| {
            left.sort_order
                .cmp(&right.sort_order)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(selected)
    }
}

fn factory_location_apparatus(
    item: ApparatusCatalogEntry,
    id: ApparatusId,
) -> FactoryLocationApparatus {
    FactoryLocationApparatus {
        id,
        name: item.name,
        source: item.source,
        sort_order: item.sort_order,
        master: item.master,
    }
}

fn required_name(value: &str) -> Result<String, FactoryLocationError> {
    let value = value.trim();
    if value.is_empty() {
        Err(FactoryLocationError::MissingName)
    } else {
        Ok(value.to_string())
    }
}

fn required_id(value: &str) -> Result<&str, FactoryLocationError> {
    let value = value.trim();
    if value.is_empty() {
        Err(FactoryLocationError::NotFound)
    } else {
        Ok(value)
    }
}

fn map_apparatus_error(_: ApparatusGroupError) -> FactoryLocationError {
    FactoryLocationError::StoreFailed
}

#[derive(Default)]
pub struct MemoryFactoryLocationStore {
    locations: RwLock<BTreeMap<String, FactoryLocation>>,
}

impl MemoryFactoryLocationStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl FactoryLocationStorePort for MemoryFactoryLocationStore {
    async fn list(&self) -> Result<Vec<FactoryLocation>, FactoryLocationError> {
        let mut items = self
            .locations
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by_key(|left| left.name.to_lowercase());
        Ok(items)
    }

    async fn create(
        &self,
        id: &str,
        name: &str,
        apparatus: &[FactoryLocationApparatus],
    ) -> Result<FactoryLocation, FactoryLocationError> {
        let mut locations = self.locations.write().await;
        if locations
            .values()
            .any(|item| item.name.eq_ignore_ascii_case(name))
        {
            return Err(FactoryLocationError::DuplicateName);
        }
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let location = FactoryLocation {
            id: id.to_string(),
            name: name.to_string(),
            active: true,
            apparatus: apparatus.to_vec(),
            created_at_unix: now,
            updated_at_unix: now,
        };
        locations.insert(id.to_string(), location.clone());
        Ok(location)
    }

    async fn update(
        &self,
        id: &str,
        name: Option<&str>,
        active: Option<bool>,
    ) -> Result<FactoryLocation, FactoryLocationError> {
        let mut locations = self.locations.write().await;
        if let Some(name) = name
            && locations
                .values()
                .any(|item| item.id != id && item.name.eq_ignore_ascii_case(name))
        {
            return Err(FactoryLocationError::DuplicateName);
        }
        let location = locations
            .get_mut(id)
            .ok_or(FactoryLocationError::NotFound)?;
        if let Some(name) = name {
            location.name = name.to_string();
        }
        if let Some(active) = active {
            location.active = active;
        }
        location.updated_at_unix = time::OffsetDateTime::now_utc().unix_timestamp();
        Ok(location.clone())
    }

    async fn replace_apparatus(
        &self,
        id: &str,
        apparatus: &[FactoryLocationApparatus],
    ) -> Result<FactoryLocation, FactoryLocationError> {
        let mut locations = self.locations.write().await;
        let location = locations
            .get_mut(id)
            .ok_or(FactoryLocationError::NotFound)?;
        location.apparatus = apparatus.to_vec();
        location.updated_at_unix = time::OffsetDateTime::now_utc().unix_timestamp();
        Ok(location.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::apparatus_groups::{ApparatusUpsert, MemoryApparatusGroupStore};

    async fn service() -> FactoryLocationService {
        let apparatus_groups = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));
        for (id, name) in [
            ("apparatus:default:bosma_7", "7 ta rangli bosma aparat"),
            ("apparatus:default:asset-010", "Rezka"),
        ] {
            apparatus_groups
                .upsert_apparatus(ApparatusUpsert {
                    id: Some(id.to_string()),
                    name: name.to_string(),
                    master: ApparatusMasterData::default(),
                })
                .await
                .expect("seed canonical apparatus");
        }
        FactoryLocationService::new(Arc::new(MemoryFactoryLocationStore::new()), apparatus_groups)
    }

    #[tokio::test]
    async fn creates_unique_immutable_id_and_resolves_apparatus_by_id() {
        let service = service().await;
        let created = service
            .create(FactoryLocationCreate {
                name: " Bosma oldi ".to_string(),
                apparatus_ids: vec!["apparatus:default:bosma_7".to_string()],
            })
            .await
            .expect("create state");
        assert!(created.id.starts_with("state_"));
        assert_eq!(created.id.len(), "state_".len() + 32);
        assert_eq!(created.name, "Bosma oldi");
        assert_eq!(created.apparatus.len(), 1);

        let inactive = service
            .update(
                &created.id,
                FactoryLocationUpdate {
                    active: Some(false),
                    ..Default::default()
                },
            )
            .await
            .expect("deactivate location");
        assert!(!inactive.active);

        let updated = service
            .replace_apparatus(
                &created.id,
                FactoryLocationApparatusReplace {
                    apparatus_ids: vec!["apparatus:default:asset-010".to_string()],
                },
            )
            .await
            .expect("replace apparatus");
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.name, created.name);
        assert!(!updated.active);
        assert_eq!(
            updated.apparatus[0].id.as_str(),
            "apparatus:default:asset-010"
        );
        assert_eq!(updated.apparatus[0].name, "Rezka");
    }

    #[tokio::test]
    async fn renaming_apparatus_display_name_does_not_change_placement_id() {
        let apparatus_store = Arc::new(MemoryApparatusGroupStore::new());
        let apparatus_groups = ApparatusGroupService::new(apparatus_store);
        apparatus_groups
            .upsert_apparatus(ApparatusUpsert {
                id: Some("apparatus:custom:placement-rename-proof".to_string()),
                name: "Original display".to_string(),
                master: ApparatusMasterData::default(),
            })
            .await
            .expect("seed canonical apparatus");
        let service = FactoryLocationService::new(
            Arc::new(MemoryFactoryLocationStore::new()),
            apparatus_groups.clone(),
        );
        let created = service
            .create(FactoryLocationCreate {
                name: "Rename proof".to_string(),
                apparatus_ids: vec!["apparatus:custom:placement-rename-proof".to_string()],
            })
            .await
            .expect("create placement");

        apparatus_groups
            .upsert_apparatus(ApparatusUpsert {
                id: Some("apparatus:custom:placement-rename-proof".to_string()),
                name: "Renamed display".to_string(),
                master: Default::default(),
            })
            .await
            .expect("rename apparatus");
        let updated = service
            .replace_apparatus(
                &created.id,
                FactoryLocationApparatusReplace {
                    apparatus_ids: vec!["apparatus:custom:placement-rename-proof".to_string()],
                },
            )
            .await
            .expect("re-resolve placement");

        assert_eq!(updated.apparatus[0].id, created.apparatus[0].id);
        assert_eq!(updated.apparatus[0].name, "Renamed display");
    }

    #[tokio::test]
    async fn rejects_display_names_and_legacy_title_ids_as_placement_keys() {
        let service = service().await;
        for apparatus_id in [
            "Rezka",
            "apparatus:Rezka",
            "apparatus:missing",
            "apparatus:custom:rezka",
        ] {
            assert_eq!(
                service
                    .create(FactoryLocationCreate {
                        name: format!("Invalid {apparatus_id}"),
                        apparatus_ids: vec![apparatus_id.to_string()],
                    })
                    .await,
                Err(FactoryLocationError::InvalidApparatus),
                "placement must not resolve apparatus by display title: {apparatus_id}"
            );
        }
    }

    #[tokio::test]
    async fn rejects_duplicate_names_and_unknown_apparatus() {
        let service = service().await;
        service
            .create(FactoryLocationCreate {
                name: "Laminat oldi".to_string(),
                apparatus_ids: Vec::new(),
            })
            .await
            .expect("create state");
        assert_eq!(
            service
                .create(FactoryLocationCreate {
                    name: " laminat OLDI ".to_string(),
                    apparatus_ids: Vec::new(),
                })
                .await,
            Err(FactoryLocationError::DuplicateName)
        );
        assert_eq!(
            service
                .create(FactoryLocationCreate {
                    name: "Noma'lum".to_string(),
                    apparatus_ids: vec!["apparatus:missing".to_string()],
                })
                .await,
            Err(FactoryLocationError::InvalidApparatus)
        );
    }

    #[tokio::test]
    async fn legacy_only_apparatus_cannot_be_used_for_placement() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        store
            .put_apparatus_with_id(
                Some("apparatus:custom:legacy-placement"),
                "Legacy placement display",
                &ApparatusMasterData::default(),
            )
            .await
            .expect("seed legacy projection");
        let service = FactoryLocationService::new(
            Arc::new(MemoryFactoryLocationStore::new()),
            ApparatusGroupService::new(store),
        );

        assert_eq!(
            service
                .create(FactoryLocationCreate {
                    name: "Legacy placement".to_string(),
                    apparatus_ids: vec!["apparatus:custom:legacy-placement".to_string()],
                })
                .await,
            Err(FactoryLocationError::InvalidApparatus)
        );
    }
}
