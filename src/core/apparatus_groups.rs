use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
#[cfg(test)]
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusGroup {
    pub name: String,
    #[serde(default)]
    pub apparatus: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusGroupUpsert {
    pub name: String,
    #[serde(default)]
    pub apparatus: Vec<String>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ApparatusGroupError {
    #[error("group name is required")]
    MissingName,
    #[error("apparatus is required")]
    MissingApparatus,
    #[error("apparatus group store failed")]
    StoreFailed,
}

#[async_trait]
pub trait ApparatusGroupStorePort: Send + Sync {
    async fn groups(&self) -> Result<Vec<ApparatusGroup>, ApparatusGroupError>;
    async fn put_group(&self, group: ApparatusGroup) -> Result<(), ApparatusGroupError>;
}

#[derive(Clone)]
pub struct ApparatusGroupService {
    store: Arc<dyn ApparatusGroupStorePort>,
}

impl ApparatusGroupService {
    pub fn new(store: Arc<dyn ApparatusGroupStorePort>) -> Self {
        Self { store }
    }

    pub async fn groups(&self) -> Result<Vec<ApparatusGroup>, ApparatusGroupError> {
        self.store.groups().await
    }

    pub async fn upsert_group(
        &self,
        input: ApparatusGroupUpsert,
    ) -> Result<ApparatusGroup, ApparatusGroupError> {
        let group = normalize_group(input)?;
        self.store.put_group(group.clone()).await?;
        Ok(group)
    }
}

fn normalize_group(input: ApparatusGroupUpsert) -> Result<ApparatusGroup, ApparatusGroupError> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(ApparatusGroupError::MissingName);
    }
    let mut seen = BTreeSet::new();
    let apparatus = input
        .apparatus
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .filter(|item| seen.insert(item.to_lowercase()))
        .collect::<Vec<_>>();
    if apparatus.is_empty() {
        return Err(ApparatusGroupError::MissingApparatus);
    }
    Ok(ApparatusGroup { name, apparatus })
}

#[derive(Default)]
#[cfg(test)]
pub struct MemoryApparatusGroupStore {
    groups: RwLock<Vec<ApparatusGroup>>,
}

#[cfg(test)]
impl MemoryApparatusGroupStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
#[cfg(test)]
impl ApparatusGroupStorePort for MemoryApparatusGroupStore {
    async fn groups(&self) -> Result<Vec<ApparatusGroup>, ApparatusGroupError> {
        Ok(self.groups.read().await.clone())
    }

    async fn put_group(&self, group: ApparatusGroup) -> Result<(), ApparatusGroupError> {
        let mut groups = self.groups.write().await;
        let key = group.name.to_lowercase();
        if let Some(index) = groups
            .iter()
            .position(|item| item.name.to_lowercase() == key)
        {
            groups[index] = group;
        } else {
            groups.push(group);
        }
        groups.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        Ok(())
    }
}
