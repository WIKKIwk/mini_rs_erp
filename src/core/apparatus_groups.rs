use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
#[cfg(test)]
use tokio::sync::RwLock;

use crate::core::production_map::pechat;

const DEFAULT_BOSMA_GROUP_NAME: &str = "Bosma aparat";
const DEFAULT_LAMINATSIYA_GROUP_NAME: &str = "Laminatsiya";
const DEFAULT_REZKA_GROUP_NAME: &str = "Rezka";
const DEFAULT_APPARATUS: [&str; 10] = [
    "7 ta rangli bosma aparat",
    "8 ta rangli bosma aparat",
    "9 ta rangli bosma aparat",
    "Extruder laminatsiya",
    "Flexo pechat",
    "Holodniy kley aparat",
    "Laminatsiya 1",
    "Laminatsiya 2",
    "Paket aparat",
    "Rezka",
];

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusUpsert {
    #[serde(default, alias = "warehouse")]
    pub name: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ApparatusGroupError {
    #[error("group name is required")]
    MissingName,
    #[error("apparatus is required")]
    MissingApparatus,
    #[error("apparatus is invalid")]
    InvalidApparatus,
    #[error("apparatus group store failed")]
    StoreFailed,
}

#[async_trait]
pub trait ApparatusGroupStorePort: Send + Sync {
    async fn groups(&self) -> Result<Vec<ApparatusGroup>, ApparatusGroupError>;
    async fn put_group(&self, group: ApparatusGroup) -> Result<(), ApparatusGroupError>;
    async fn apparatus(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<String>, ApparatusGroupError>;
    async fn put_apparatus(&self, name: &str) -> Result<String, ApparatusGroupError>;
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
        let groups = self.store.groups().await?;
        if groups.is_empty() {
            return Ok(default_apparatus_groups());
        }
        Ok(normalize_groups(groups))
    }

    pub async fn upsert_group(
        &self,
        input: ApparatusGroupUpsert,
    ) -> Result<ApparatusGroup, ApparatusGroupError> {
        let group = normalize_group(input)?;
        self.store.put_group(group.clone()).await?;
        Ok(group)
    }

    pub async fn apparatus(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<String>, ApparatusGroupError> {
        let limit = limit.max(1);
        let needle = query.trim().to_lowercase();
        let mut seen = BTreeSet::new();
        let mut result = default_apparatus()
            .into_iter()
            .filter(|item| needle.is_empty() || item.to_lowercase().contains(&needle))
            .filter(|item| seen.insert(item.to_lowercase()))
            .take(limit)
            .collect::<Vec<_>>();
        if result.len() >= limit {
            return Ok(result);
        }
        for item in self.store.apparatus(query, limit).await? {
            let name = item.trim().to_string();
            if name.is_empty()
                || is_invalid_legacy_apparatus_name(&name)
                || !seen.insert(name.to_lowercase())
            {
                continue;
            }
            result.push(name);
            if result.len() >= limit {
                break;
            }
        }
        Ok(result)
    }

    pub async fn upsert_apparatus(
        &self,
        input: ApparatusUpsert,
    ) -> Result<String, ApparatusGroupError> {
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err(ApparatusGroupError::MissingApparatus);
        }
        if is_invalid_legacy_apparatus_name(&name) {
            return Err(ApparatusGroupError::InvalidApparatus);
        }
        self.store.put_apparatus(&name).await
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
    Ok(canonical_group(ApparatusGroup { name, apparatus }))
}

fn normalize_groups(groups: Vec<ApparatusGroup>) -> Vec<ApparatusGroup> {
    let mut normalized = Vec::<ApparatusGroup>::new();
    for group in groups.into_iter().map(canonical_group) {
        if let Some(index) = normalized
            .iter()
            .position(|item| item.name.eq_ignore_ascii_case(&group.name))
        {
            normalized[index] = merge_groups(normalized[index].clone(), group);
        } else {
            normalized.push(group);
        }
    }
    normalized
}

fn canonical_group(group: ApparatusGroup) -> ApparatusGroup {
    if group_is_bosma(&group) {
        return ApparatusGroup {
            name: DEFAULT_BOSMA_GROUP_NAME.to_string(),
            apparatus: default_bosma_apparatus(),
        };
    }
    if group_is_laminatsiya(&group) {
        return ApparatusGroup {
            name: DEFAULT_LAMINATSIYA_GROUP_NAME.to_string(),
            apparatus: group.apparatus,
        };
    }
    if group_is_rezka(&group) {
        return ApparatusGroup {
            name: DEFAULT_REZKA_GROUP_NAME.to_string(),
            apparatus: group.apparatus,
        };
    }
    group
}

fn merge_groups(left: ApparatusGroup, right: ApparatusGroup) -> ApparatusGroup {
    if left.name == DEFAULT_BOSMA_GROUP_NAME {
        return ApparatusGroup {
            name: DEFAULT_BOSMA_GROUP_NAME.to_string(),
            apparatus: default_bosma_apparatus(),
        };
    }
    let mut seen = BTreeSet::new();
    let apparatus = left
        .apparatus
        .into_iter()
        .chain(right.apparatus)
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .filter(|item| seen.insert(item.to_lowercase()))
        .collect();
    ApparatusGroup {
        name: left.name,
        apparatus,
    }
}

fn default_bosma_apparatus() -> Vec<String> {
    [7_u8, 8, 9]
        .into_iter()
        .map(|count| format!("{count} ta rangli bosma aparat"))
        .collect()
}

fn default_laminatsiya_apparatus() -> Vec<String> {
    vec!["Laminatsiya 1".to_string(), "Laminatsiya 2".to_string()]
}

fn default_rezka_apparatus() -> Vec<String> {
    vec![DEFAULT_REZKA_GROUP_NAME.to_string()]
}

fn default_apparatus_groups() -> Vec<ApparatusGroup> {
    vec![
        ApparatusGroup {
            name: DEFAULT_BOSMA_GROUP_NAME.to_string(),
            apparatus: default_bosma_apparatus(),
        },
        ApparatusGroup {
            name: DEFAULT_LAMINATSIYA_GROUP_NAME.to_string(),
            apparatus: default_laminatsiya_apparatus(),
        },
        ApparatusGroup {
            name: DEFAULT_REZKA_GROUP_NAME.to_string(),
            apparatus: default_rezka_apparatus(),
        },
    ]
}

fn default_apparatus() -> Vec<String> {
    DEFAULT_APPARATUS.into_iter().map(str::to_string).collect()
}

fn is_invalid_legacy_apparatus_name(value: &str) -> bool {
    matches!(
        value.trim().to_lowercase().as_str(),
        "7 ta rangli pechat" | "8 ta rangli pechat" | "9 ta rangli pechat"
    )
}

fn group_is_bosma(group: &ApparatusGroup) -> bool {
    pechat::pechat_color_count(&group.name).is_some()
        || group.apparatus.iter().any(|item| {
            pechat::pechat_color_count(item).is_some()
                || item.trim().eq_ignore_ascii_case(DEFAULT_BOSMA_GROUP_NAME)
        })
        || group
            .name
            .trim()
            .eq_ignore_ascii_case(DEFAULT_BOSMA_GROUP_NAME)
}

fn group_is_laminatsiya(group: &ApparatusGroup) -> bool {
    text_contains_word(&group.name, "laminatsiya")
        || group
            .apparatus
            .iter()
            .any(|item| text_contains_word(item, "laminatsiya"))
}

fn group_is_rezka(group: &ApparatusGroup) -> bool {
    text_contains_word(&group.name, "rezka")
        || group
            .apparatus
            .iter()
            .any(|item| text_contains_word(item, "rezka"))
}

fn text_contains_word(value: &str, needle: &str) -> bool {
    value.trim().to_lowercase().contains(needle)
}

#[derive(Default)]
#[cfg(test)]
pub struct MemoryApparatusGroupStore {
    groups: RwLock<Vec<ApparatusGroup>>,
    apparatus: RwLock<Vec<String>>,
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
        groups.sort_by_key(|group| group.name.to_lowercase());
        Ok(())
    }

    async fn apparatus(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<String>, ApparatusGroupError> {
        let needle = query.trim().to_lowercase();
        let result = self
            .apparatus
            .read()
            .await
            .iter()
            .filter_map(|item| {
                let name = item.trim();
                if name.is_empty() || (!needle.is_empty() && !name.to_lowercase().contains(&needle))
                {
                    return None;
                }
                Some(name.to_string())
            })
            .take(limit)
            .collect();
        Ok(result)
    }

    async fn put_apparatus(&self, name: &str) -> Result<String, ApparatusGroupError> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(ApparatusGroupError::MissingApparatus);
        }
        let mut apparatus = self.apparatus.write().await;
        if !apparatus
            .iter()
            .any(|item| item.to_lowercase() == name.to_lowercase())
        {
            apparatus.push(name.clone());
            apparatus.sort_by_key(|item| item.to_lowercase());
        }
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn apparatus_catalog_returns_one_default_each_and_keeps_custom_names() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        for name in DEFAULT_APPARATUS.into_iter().chain([
            "7 ta rangli pechat",
            "8 ta rangli pechat",
            "9 ta rangli pechat",
        ]) {
            store
                .put_apparatus(name)
                .await
                .expect("seed stored apparatus");
        }
        store
            .put_apparatus("Maxsus aparat")
            .await
            .expect("seed custom apparatus");
        let service = ApparatusGroupService::new(store);

        let apparatus = service.apparatus("", 50).await.expect("list apparatus");

        let mut expected = default_apparatus();
        expected.push("Maxsus aparat".to_string());
        assert_eq!(apparatus, expected);
        assert_eq!(
            service
                .upsert_apparatus(ApparatusUpsert {
                    name: "7 ta rangli pechat".to_string(),
                })
                .await,
            Err(ApparatusGroupError::InvalidApparatus)
        );
    }
}
