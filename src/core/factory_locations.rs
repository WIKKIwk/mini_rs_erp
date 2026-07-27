use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use data_encoding::HEXLOWER;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::core::apparatus_groups::{
    ApparatusCatalogEntry, ApparatusGroupError, ApparatusGroupService,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactoryLocation {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub apparatus: Vec<ApparatusCatalogEntry>,
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
        apparatus: &[ApparatusCatalogEntry],
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
        apparatus: &[ApparatusCatalogEntry],
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
        self.store.list().await
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
        self.store.update(id, name.as_deref(), input.active).await
    }

    pub async fn replace_apparatus(
        &self,
        id: &str,
        input: FactoryLocationApparatusReplace,
    ) -> Result<FactoryLocation, FactoryLocationError> {
        let id = required_id(id)?;
        let apparatus = self.resolve_apparatus(input.apparatus_ids).await?;
        self.store.replace_apparatus(id, &apparatus).await
    }

    async fn resolve_apparatus(
        &self,
        ids: Vec<String>,
    ) -> Result<Vec<ApparatusCatalogEntry>, FactoryLocationError> {
        let mut requested = BTreeSet::new();
        for id in ids {
            let id = id.trim();
            if id.is_empty() {
                return Err(FactoryLocationError::InvalidApparatus);
            }
            requested.insert(id.to_string());
        }
        if requested.is_empty() {
            return Ok(Vec::new());
        }
        let catalog = self
            .apparatus_groups
            .apparatus_catalog("", 10_000)
            .await
            .map_err(map_apparatus_error)?;
        let mut selected = catalog
            .into_iter()
            .filter(|item| requested.remove(&item.id))
            .collect::<Vec<_>>();
        if !requested.is_empty() {
            return Err(FactoryLocationError::InvalidApparatus);
        }
        selected.sort_by(|left, right| {
            left.sort_order
                .cmp(&right.sort_order)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        Ok(selected)
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
        items.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        Ok(items)
    }

    async fn create(
        &self,
        id: &str,
        name: &str,
        apparatus: &[ApparatusCatalogEntry],
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
        if let Some(name) = name {
            if locations
                .values()
                .any(|item| item.id != id && item.name.eq_ignore_ascii_case(name))
            {
                return Err(FactoryLocationError::DuplicateName);
            }
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
        apparatus: &[ApparatusCatalogEntry],
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
    use crate::core::apparatus_groups::MemoryApparatusGroupStore;

    fn service() -> FactoryLocationService {
        FactoryLocationService::new(
            Arc::new(MemoryFactoryLocationStore::new()),
            ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new())),
        )
    }

    #[tokio::test]
    async fn creates_unique_immutable_id_and_derives_apparatus_state() {
        let service = service();
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

        let updated = service
            .replace_apparatus(
                &created.id,
                FactoryLocationApparatusReplace {
                    apparatus_ids: vec!["apparatus:default:rezka".to_string()],
                },
            )
            .await
            .expect("replace apparatus");
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.name, created.name);
        assert_eq!(updated.apparatus[0].name, "Rezka");
    }

    #[tokio::test]
    async fn rejects_duplicate_names_and_unknown_apparatus() {
        let service = service();
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
}
