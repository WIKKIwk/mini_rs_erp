use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use data_encoding::HEXLOWER;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::core::apparatus_standard::{ApparatusId, CanonicalApparatusService, LifecycleState};

const MAX_COLLECTION_NAME_CHARS: usize = 80;
const MAX_COLLECTION_APPARATUS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApparatusCollection {
    pub id: String,
    pub name: String,
    pub apparatus_ids: Vec<ApparatusId>,
    pub revision: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApparatusCollectionCreate {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub apparatus_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApparatusCollectionUpdate {
    pub expected_revision: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub apparatus_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApparatusCollectionDelete {
    pub expected_revision: u64,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ApparatusCollectionError {
    #[error("collection name is required")]
    MissingName,
    #[error("collection name is too long")]
    NameTooLong,
    #[error("collection has too many apparatus")]
    TooManyApparatus,
    #[error("apparatus id is invalid")]
    InvalidApparatus,
    #[error("collection name already exists")]
    DuplicateName,
    #[error("collection not found")]
    NotFound,
    #[error("collection revision is invalid")]
    InvalidRevision,
    #[error("collection revision conflicts with current state")]
    RevisionConflict,
    #[error("collection store failed")]
    StoreFailed,
}

#[async_trait]
pub trait ApparatusCollectionStorePort: Send + Sync {
    async fn list(&self) -> Result<Vec<ApparatusCollection>, ApparatusCollectionError>;

    async fn create(
        &self,
        id: &str,
        name: &str,
        apparatus_ids: &[ApparatusId],
    ) -> Result<ApparatusCollection, ApparatusCollectionError>;

    async fn update(
        &self,
        id: &str,
        expected_revision: u64,
        name: &str,
        apparatus_ids: &[ApparatusId],
    ) -> Result<ApparatusCollection, ApparatusCollectionError>;

    async fn delete(
        &self,
        id: &str,
        expected_revision: u64,
    ) -> Result<(), ApparatusCollectionError>;
}

#[derive(Clone)]
pub struct ApparatusCollectionService {
    store: Arc<dyn ApparatusCollectionStorePort>,
    apparatus: CanonicalApparatusService,
}

impl ApparatusCollectionService {
    pub fn new(
        store: Arc<dyn ApparatusCollectionStorePort>,
        apparatus: CanonicalApparatusService,
    ) -> Self {
        Self { store, apparatus }
    }

    pub async fn list(&self) -> Result<Vec<ApparatusCollection>, ApparatusCollectionError> {
        self.store.list().await
    }

    pub async fn create(
        &self,
        input: ApparatusCollectionCreate,
    ) -> Result<ApparatusCollection, ApparatusCollectionError> {
        let name = normalized_name(&input.name)?;
        let apparatus_ids = self.resolve_apparatus(input.apparatus_ids).await?;
        let id = format!(
            "apparatus-collection:{}",
            HEXLOWER.encode(&rand::random::<[u8; 16]>())
        );
        self.store.create(&id, &name, &apparatus_ids).await
    }

    pub async fn update(
        &self,
        id: &str,
        input: ApparatusCollectionUpdate,
    ) -> Result<ApparatusCollection, ApparatusCollectionError> {
        let id = normalized_id(id)?;
        let expected_revision = valid_revision(input.expected_revision)?;
        let name = normalized_name(&input.name)?;
        let apparatus_ids = self.resolve_apparatus(input.apparatus_ids).await?;
        self.store
            .update(id, expected_revision, &name, &apparatus_ids)
            .await
    }

    pub async fn delete(
        &self,
        id: &str,
        input: ApparatusCollectionDelete,
    ) -> Result<(), ApparatusCollectionError> {
        self.store
            .delete(normalized_id(id)?, valid_revision(input.expected_revision)?)
            .await
    }

    async fn resolve_apparatus(
        &self,
        ids: Vec<String>,
    ) -> Result<Vec<ApparatusId>, ApparatusCollectionError> {
        if ids.len() > MAX_COLLECTION_APPARATUS {
            return Err(ApparatusCollectionError::TooManyApparatus);
        }
        let mut requested = BTreeSet::new();
        for id in ids {
            let id = ApparatusId::new(id.trim().to_string())
                .map_err(|_| ApparatusCollectionError::InvalidApparatus)?;
            requested.insert(id);
        }
        for id in &requested {
            let projection = self
                .apparatus
                .current_projection(id)
                .await
                .map_err(|_| ApparatusCollectionError::StoreFailed)?
                .ok_or(ApparatusCollectionError::InvalidApparatus)?;
            if projection.lifecycle.state != LifecycleState::Active {
                return Err(ApparatusCollectionError::InvalidApparatus);
            }
        }
        Ok(requested.into_iter().collect())
    }
}

fn normalized_name(value: &str) -> Result<String, ApparatusCollectionError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ApparatusCollectionError::MissingName);
    }
    if value.chars().count() > MAX_COLLECTION_NAME_CHARS {
        return Err(ApparatusCollectionError::NameTooLong);
    }
    Ok(value.to_string())
}

fn normalized_id(value: &str) -> Result<&str, ApparatusCollectionError> {
    let value = value.trim();
    if value.is_empty() {
        Err(ApparatusCollectionError::NotFound)
    } else {
        Ok(value)
    }
}

fn valid_revision(value: u64) -> Result<u64, ApparatusCollectionError> {
    if value == 0 {
        Err(ApparatusCollectionError::InvalidRevision)
    } else {
        Ok(value)
    }
}

#[derive(Default)]
pub struct MemoryApparatusCollectionStore {
    collections: RwLock<BTreeMap<String, ApparatusCollection>>,
}

impl MemoryApparatusCollectionStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ApparatusCollectionStorePort for MemoryApparatusCollectionStore {
    async fn list(&self) -> Result<Vec<ApparatusCollection>, ApparatusCollectionError> {
        let mut items = self
            .collections
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(items)
    }

    async fn create(
        &self,
        id: &str,
        name: &str,
        apparatus_ids: &[ApparatusId],
    ) -> Result<ApparatusCollection, ApparatusCollectionError> {
        let mut collections = self.collections.write().await;
        let normalized_name = name.to_lowercase();
        if collections
            .values()
            .any(|item| item.name.to_lowercase() == normalized_name)
        {
            return Err(ApparatusCollectionError::DuplicateName);
        }
        let collection = ApparatusCollection {
            id: id.to_string(),
            name: name.to_string(),
            apparatus_ids: apparatus_ids.to_vec(),
            revision: 1,
        };
        collections.insert(id.to_string(), collection.clone());
        Ok(collection)
    }

    async fn update(
        &self,
        id: &str,
        expected_revision: u64,
        name: &str,
        apparatus_ids: &[ApparatusId],
    ) -> Result<ApparatusCollection, ApparatusCollectionError> {
        let mut collections = self.collections.write().await;
        let normalized_name = name.to_lowercase();
        if collections
            .values()
            .any(|item| item.id != id && item.name.to_lowercase() == normalized_name)
        {
            return Err(ApparatusCollectionError::DuplicateName);
        }
        let collection = collections
            .get_mut(id)
            .ok_or(ApparatusCollectionError::NotFound)?;
        if collection.revision != expected_revision {
            return Err(ApparatusCollectionError::RevisionConflict);
        }
        collection.name = name.to_string();
        collection.apparatus_ids = apparatus_ids.to_vec();
        collection.revision = collection
            .revision
            .checked_add(1)
            .ok_or(ApparatusCollectionError::StoreFailed)?;
        Ok(collection.clone())
    }

    async fn delete(
        &self,
        id: &str,
        expected_revision: u64,
    ) -> Result<(), ApparatusCollectionError> {
        let mut collections = self.collections.write().await;
        let collection = collections
            .get(id)
            .ok_or(ApparatusCollectionError::NotFound)?;
        if collection.revision != expected_revision {
            return Err(ApparatusCollectionError::RevisionConflict);
        }
        collections.remove(id);
        Ok(())
    }
}
