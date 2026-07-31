use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::collections::BTreeMap;
use thiserror::Error;
#[cfg(test)]
use tokio::sync::RwLock;

use crate::core::production_map::pechat;

const DEFAULT_BOSMA_GROUP_NAME: &str = "Bosma aparat";
const DEFAULT_LAMINATSIYA_GROUP_NAME: &str = "Laminatsiya";
const DEFAULT_REZKA_GROUP_NAME: &str = "Rezka";
const DEFAULT_APPARATUS: [(&str, &str); 10] = [
    ("apparatus:default:bosma_7", "7 ta rangli bosma aparat"),
    ("apparatus:default:bosma_8", "8 ta rangli bosma aparat"),
    ("apparatus:default:bosma_9", "9 ta rangli bosma aparat"),
    (
        "apparatus:default:extruder_laminatsiya",
        "Extruder laminatsiya",
    ),
    ("apparatus:default:flexo_pechat", "Flexo pechat"),
    ("apparatus:default:holodniy_kley", "Holodniy kley aparat"),
    ("apparatus:default:laminatsiya_1", "Laminatsiya 1"),
    ("apparatus:default:laminatsiya_2", "Laminatsiya 2"),
    ("apparatus:default:paket", "Paket aparat"),
    ("apparatus:default:rezka", "Rezka"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApparatusSource {
    Default,
    Custom,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusMasterData {
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_stations: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusCatalogEntry {
    pub id: String,
    pub name: String,
    pub source: ApparatusSource,
    pub sort_order: usize,
    #[serde(flatten, default)]
    pub master: ApparatusMasterData,
}

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
    #[serde(flatten, default)]
    pub master: ApparatusMasterData,
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
    async fn apparatus_catalog(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ApparatusCatalogEntry>, ApparatusGroupError> {
        self.apparatus(query, limit).await.map(|items| {
            items
                .into_iter()
                .enumerate()
                .map(|(sort_order, name)| ApparatusCatalogEntry {
                    id: custom_apparatus_id(&name),
                    master: apparatus_master_data_for_name(&name),
                    name,
                    source: ApparatusSource::Custom,
                    sort_order,
                })
                .collect()
        })
    }
    async fn put_apparatus(&self, name: &str) -> Result<String, ApparatusGroupError>;
    async fn put_apparatus_with_master_data(
        &self,
        name: &str,
        _master: &ApparatusMasterData,
    ) -> Result<String, ApparatusGroupError> {
        self.put_apparatus(name).await
    }
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
        self.apparatus_catalog(query, limit)
            .await
            .map(|items| items.into_iter().map(|item| item.name).collect())
    }

    pub async fn apparatus_catalog(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ApparatusCatalogEntry>, ApparatusGroupError> {
        let limit = limit.max(1);
        let needle = query.trim().to_lowercase();
        let mut seen = BTreeSet::new();
        let mut result = default_apparatus_catalog()
            .into_iter()
            .filter(|item| needle.is_empty() || item.name.to_lowercase().contains(&needle))
            .filter(|item| seen.insert(item.name.to_lowercase()))
            .take(limit)
            .collect::<Vec<_>>();
        if result.len() >= limit {
            return Ok(result);
        }
        for item in self.store.apparatus_catalog(query, limit).await? {
            let name = item.name.trim().to_string();
            if name.is_empty()
                || is_invalid_legacy_apparatus_name(&name)
                || !seen.insert(name.to_lowercase())
            {
                continue;
            }
            let id = if item.id.trim().is_empty() {
                custom_apparatus_id(&name)
            } else {
                item.id
            };
            let master = normalize_apparatus_master_data(item.master, &name);
            result.push(ApparatusCatalogEntry {
                id,
                name,
                source: ApparatusSource::Custom,
                sort_order: result.len(),
                master,
            });
            if result.len() >= limit {
                break;
            }
        }
        Ok(result)
    }

    pub async fn upsert_apparatus(
        &self,
        input: ApparatusUpsert,
    ) -> Result<ApparatusCatalogEntry, ApparatusGroupError> {
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err(ApparatusGroupError::MissingApparatus);
        }
        if is_invalid_legacy_apparatus_name(&name) {
            return Err(ApparatusGroupError::InvalidApparatus);
        }
        let master = normalize_apparatus_master_data(input.master, &name);
        self.store
            .put_apparatus_with_master_data(&name, &master)
            .await?;
        Ok(ApparatusCatalogEntry {
            id: custom_apparatus_id(&name),
            name,
            source: ApparatusSource::Custom,
            sort_order: 0,
            master,
        })
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

#[cfg(test)]
fn default_apparatus() -> Vec<String> {
    DEFAULT_APPARATUS
        .into_iter()
        .map(|(_, name)| name.to_string())
        .collect()
}

fn default_apparatus_catalog() -> Vec<ApparatusCatalogEntry> {
    DEFAULT_APPARATUS
        .into_iter()
        .enumerate()
        .map(|(sort_order, (id, name))| ApparatusCatalogEntry {
            id: id.to_string(),
            name: name.to_string(),
            source: ApparatusSource::Default,
            sort_order,
            master: apparatus_master_data_for_name(name),
        })
        .collect()
}

pub fn apparatus_master_data_for_name(name: &str) -> ApparatusMasterData {
    let normalized = name.trim().to_lowercase();
    if pechat::is_flexo_apparatus(&normalized) {
        return ApparatusMasterData {
            family: "pechat".to_string(),
            kind: "flexo".to_string(),
            capabilities: vec![
                "print".to_string(),
                "pechat".to_string(),
                "flexo".to_string(),
            ],
            color_stations: None,
        };
    }
    if let Some(color_stations) = pechat::pechat_color_count(&normalized) {
        return ApparatusMasterData {
            family: "pechat".to_string(),
            kind: "color_pechat".to_string(),
            capabilities: vec!["print".to_string(), "pechat".to_string()],
            color_stations: Some(color_stations),
        };
    }
    if normalized.contains("extruder") && normalized.contains("laminatsiya") {
        return apparatus_master_data("laminatsiya", "extruder_laminatsiya", ["laminate"], None);
    }
    if normalized.contains("laminatsiya") {
        return apparatus_master_data("laminatsiya", "laminatsiya", ["laminate"], None);
    }
    if normalized.contains("rezka") {
        return apparatus_master_data("rezka", "rezka", ["cut"], None);
    }
    if normalized.contains("paket") {
        return apparatus_master_data("paket", "paket", ["package"], None);
    }
    if normalized.contains("kley") {
        return apparatus_master_data("kley", "holodniy_kley", ["glue"], None);
    }
    apparatus_master_data("other", "other", ["apparatus"], None)
}

fn apparatus_master_data(
    family: &str,
    kind: &str,
    capabilities: impl IntoIterator<Item = &'static str>,
    color_stations: Option<u8>,
) -> ApparatusMasterData {
    ApparatusMasterData {
        family: family.to_string(),
        kind: kind.to_string(),
        capabilities: capabilities.into_iter().map(str::to_string).collect(),
        color_stations,
    }
}

pub fn normalize_apparatus_master_data(
    mut master: ApparatusMasterData,
    name: &str,
) -> ApparatusMasterData {
    let inferred = apparatus_master_data_for_name(name);
    if master.family.trim().is_empty() {
        master.family = inferred.family;
    } else {
        master.family = master.family.trim().to_lowercase();
    }
    if master.kind.trim().is_empty() {
        master.kind = inferred.kind;
    } else {
        master.kind = master.kind.trim().to_lowercase();
    }
    if master.capabilities.is_empty() {
        master.capabilities = inferred.capabilities;
    } else {
        master.capabilities = master
            .capabilities
            .into_iter()
            .map(|capability| capability.trim().to_lowercase())
            .filter(|capability| !capability.is_empty())
            .fold(Vec::new(), |mut values, capability| {
                if !values.iter().any(|item| item == &capability) {
                    values.push(capability);
                }
                values
            });
    }
    if master.color_stations.is_none() {
        master.color_stations = inferred.color_stations;
    }
    master
}

pub fn custom_apparatus_id(name: &str) -> String {
    format!("apparatus:{}", name.trim().to_lowercase())
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
    apparatus_master_data: RwLock<BTreeMap<String, ApparatusMasterData>>,
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
        self.put_apparatus_with_master_data(name, &apparatus_master_data_for_name(name))
            .await
    }

    async fn apparatus_catalog(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ApparatusCatalogEntry>, ApparatusGroupError> {
        let names = self.apparatus(query, limit).await?;
        let master_data = self.apparatus_master_data.read().await;
        Ok(names
            .into_iter()
            .enumerate()
            .map(|(sort_order, name)| ApparatusCatalogEntry {
                id: custom_apparatus_id(&name),
                master: master_data
                    .get(&name.to_lowercase())
                    .cloned()
                    .unwrap_or_else(|| apparatus_master_data_for_name(&name)),
                name,
                source: ApparatusSource::Custom,
                sort_order,
            })
            .collect())
    }

    async fn put_apparatus_with_master_data(
        &self,
        name: &str,
        master: &ApparatusMasterData,
    ) -> Result<String, ApparatusGroupError> {
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
        drop(apparatus);
        self.apparatus_master_data
            .write()
            .await
            .insert(name.to_lowercase(), master.clone());
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn apparatus_catalog_returns_one_default_each_and_keeps_custom_names() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        for name in DEFAULT_APPARATUS.into_iter().map(|(_, name)| name).chain([
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

        let catalog = service
            .apparatus_catalog("", 50)
            .await
            .expect("list apparatus catalog");
        assert_eq!(catalog[0].id, "apparatus:default:bosma_7");
        assert_eq!(catalog[0].source, ApparatusSource::Default);
        assert_eq!(catalog[0].master.family, "pechat");
        assert_eq!(catalog[0].master.kind, "color_pechat");
        assert_eq!(catalog[0].master.color_stations, Some(7));
        let flexo = catalog
            .iter()
            .find(|item| item.name == "Flexo pechat")
            .expect("flexo catalog entry");
        assert_eq!(flexo.master.family, "pechat");
        assert_eq!(flexo.master.kind, "flexo");
        assert!(flexo.master.capabilities.iter().any(|item| item == "print"));
        assert_eq!(flexo.master.color_stations, None);
        let custom = catalog.last().expect("custom apparatus");
        assert_eq!(custom.id, "apparatus:maxsus aparat");
        assert_eq!(custom.source, ApparatusSource::Custom);
        assert_eq!(
            service
                .upsert_apparatus(ApparatusUpsert {
                    name: "7 ta rangli pechat".to_string(),
                    master: ApparatusMasterData::default(),
                })
                .await,
            Err(ApparatusGroupError::InvalidApparatus)
        );
    }
}
