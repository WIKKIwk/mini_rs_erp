use std::collections::{BTreeMap, BTreeSet};
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
pub const APPARATUS_COLOR_STATIONS_MIN: u8 = 1;
pub const APPARATUS_COLOR_STATIONS_MAX: u8 = 24;
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
    #[serde(default)]
    pub capability_profiles: Vec<ApparatusCapabilityProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_stations: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusMasterOptions {
    pub families: Vec<String>,
    pub kinds_by_family: BTreeMap<String, Vec<String>>,
    pub capabilities: Vec<String>,
    pub color_stations_min: u8,
    pub color_stations_max: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusCapabilityProfile {
    pub code: String,
    #[serde(default = "default_capability_level")]
    pub level: u16,
    #[serde(default)]
    pub valid_from_unix: Option<i64>,
    #[serde(default)]
    pub valid_to_unix: Option<i64>,
    #[serde(default = "default_capability_enabled")]
    pub enabled: bool,
}

impl ApparatusCapabilityProfile {
    pub fn is_valid_at(&self, at_unix: i64) -> bool {
        self.enabled
            && self
                .valid_from_unix
                .is_none_or(|starts_at| at_unix >= starts_at)
            && self
                .valid_to_unix
                .is_none_or(|ends_at| at_unix < ends_at)
    }
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
    #[serde(default)]
    pub id: Option<String>,
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
    #[error("apparatus family is invalid")]
    InvalidFamily,
    #[error("apparatus kind is invalid")]
    InvalidKind,
    #[error("apparatus capability is invalid")]
    InvalidCapability,
    #[error("apparatus color stations are invalid")]
    InvalidColorStations,
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
    async fn put_apparatus_with_id(
        &self,
        requested_id: Option<&str>,
        name: &str,
        master: &ApparatusMasterData,
    ) -> Result<String, ApparatusGroupError> {
        self.put_apparatus_with_master_data(name, master).await?;
        Ok(requested_id
            .filter(|id| !id.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| custom_apparatus_id(name)))
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
        let requested_id = normalize_requested_apparatus_id(input.id.as_deref())?;
        validate_explicit_apparatus_master_data(&input.master)?;
        let master = normalize_apparatus_master_data(input.master, &name);
        validate_apparatus_master_data(&master)?;
        let id = self
            .store
            .put_apparatus_with_id(requested_id.as_deref(), &name, &master)
            .await?;
        Ok(ApparatusCatalogEntry {
            id,
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
    let mut apparatus = [7_u8, 8, 9]
        .into_iter()
        .map(|count| format!("{count} ta rangli bosma aparat"))
        .collect::<Vec<_>>();
    apparatus.push("Flexo pechat".to_string());
    apparatus
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

pub fn apparatus_master_options() -> ApparatusMasterOptions {
    let families = vec![
        "pechat".to_string(),
        "laminatsiya".to_string(),
        "rezka".to_string(),
        "paket".to_string(),
        "kley".to_string(),
        "other".to_string(),
    ];
    let mut kinds_by_family = BTreeMap::new();
    kinds_by_family.insert(
        "pechat".to_string(),
        vec!["color_pechat".to_string(), "flexo".to_string()],
    );
    kinds_by_family.insert(
        "laminatsiya".to_string(),
        vec![
            "laminatsiya".to_string(),
            "extruder_laminatsiya".to_string(),
        ],
    );
    kinds_by_family.insert("rezka".to_string(), vec!["rezka".to_string()]);
    kinds_by_family.insert("paket".to_string(), vec!["paket".to_string()]);
    kinds_by_family.insert("kley".to_string(), vec!["holodniy_kley".to_string()]);
    kinds_by_family.insert("other".to_string(), vec!["other".to_string()]);

    ApparatusMasterOptions {
        families,
        kinds_by_family,
        capabilities: [
            "print",
            "pechat",
            "flexo",
            "laminate",
            "cut",
            "package",
            "glue",
            "apparatus",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        color_stations_min: APPARATUS_COLOR_STATIONS_MIN,
        color_stations_max: APPARATUS_COLOR_STATIONS_MAX,
    }
}

fn validate_explicit_apparatus_master_data(
    master: &ApparatusMasterData,
) -> Result<(), ApparatusGroupError> {
    let options = apparatus_master_options();
    let family = master.family.trim().to_lowercase();
    if !family.is_empty() && !options.families.iter().any(|item| item == &family) {
        return Err(ApparatusGroupError::InvalidFamily);
    }

    let kind = master.kind.trim().to_lowercase();
    if !kind.is_empty() {
        let kind_is_known = if family.is_empty() {
            options
                .kinds_by_family
                .values()
                .any(|kinds| kinds.iter().any(|item| item == &kind))
        } else {
            options
                .kinds_by_family
                .get(&family)
                .is_some_and(|kinds| kinds.iter().any(|item| item == &kind))
        };
        if !kind_is_known {
            return Err(ApparatusGroupError::InvalidKind);
        }
    }

    for code in master.capabilities.iter().map(String::as_str).chain(
        master
            .capability_profiles
            .iter()
            .map(|profile| profile.code.as_str()),
    ) {
        let code = code.trim().to_lowercase();
        if !code.is_empty() && !options.capabilities.iter().any(|item| item == &code) {
            return Err(ApparatusGroupError::InvalidCapability);
        }
    }

    if let Some(color_stations) = master.color_stations {
        if !(APPARATUS_COLOR_STATIONS_MIN..=APPARATUS_COLOR_STATIONS_MAX).contains(&color_stations)
        {
            return Err(ApparatusGroupError::InvalidColorStations);
        }
    }
    Ok(())
}

fn validate_apparatus_master_data(master: &ApparatusMasterData) -> Result<(), ApparatusGroupError> {
    let options = apparatus_master_options();
    let family = master.family.trim().to_lowercase();
    let Some(kinds) = options.kinds_by_family.get(&family) else {
        return Err(ApparatusGroupError::InvalidFamily);
    };
    let kind = master.kind.trim().to_lowercase();
    if !kinds.iter().any(|item| item == &kind) {
        return Err(ApparatusGroupError::InvalidKind);
    }
    if master.capabilities.is_empty() {
        return Err(ApparatusGroupError::InvalidCapability);
    }
    for code in master.capabilities.iter().map(String::as_str).chain(
        master
            .capability_profiles
            .iter()
            .map(|profile| profile.code.as_str()),
    ) {
        let code = code.trim().to_lowercase();
        if code.is_empty() || !options.capabilities.iter().any(|item| item == &code) {
            return Err(ApparatusGroupError::InvalidCapability);
        }
    }
    if let Some(color_stations) = master.color_stations {
        if kind != "color_pechat"
            || !(APPARATUS_COLOR_STATIONS_MIN..=APPARATUS_COLOR_STATIONS_MAX)
                .contains(&color_stations)
        {
            return Err(ApparatusGroupError::InvalidColorStations);
        }
    }
    Ok(())
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
            capability_profiles: default_capability_profiles(["print", "pechat", "flexo"]),
            color_stations: None,
        };
    }
    if let Some(color_stations) = pechat::pechat_color_count(&normalized) {
        return ApparatusMasterData {
            family: "pechat".to_string(),
            kind: "color_pechat".to_string(),
            capabilities: vec!["print".to_string(), "pechat".to_string()],
            capability_profiles: default_capability_profiles(["print", "pechat"]),
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
    let capabilities = capabilities.into_iter().map(str::to_string).collect::<Vec<_>>();
    ApparatusMasterData {
        family: family.to_string(),
        kind: kind.to_string(),
        capability_profiles: default_capability_profiles(capabilities.iter().map(String::as_str)),
        capabilities,
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
    if master.kind == "flexo" || master.capabilities.iter().any(|item| item == "flexo") {
        master.family = "pechat".to_string();
        master.kind = "flexo".to_string();
        for capability in ["print", "pechat", "flexo"] {
            if !master.capabilities.iter().any(|item| item == capability) {
                master.capabilities.push(capability.to_string());
            }
        }
    }
    master.capability_profiles = normalize_capability_profiles(
        master.capability_profiles,
        &master.capabilities,
    );
    if master.color_stations.is_none() {
        master.color_stations = inferred.color_stations;
    }
    master
}

fn default_capability_level() -> u16 {
    1
}

fn default_capability_enabled() -> bool {
    true
}

fn default_capability_profiles<'a>(
    capabilities: impl IntoIterator<Item = &'a str>,
) -> Vec<ApparatusCapabilityProfile> {
    capabilities
        .into_iter()
        .map(|code| ApparatusCapabilityProfile {
            code: code.to_string(),
            level: 1,
            valid_from_unix: None,
            valid_to_unix: None,
            enabled: true,
        })
        .collect()
}

fn normalize_capability_profiles(
    profiles: Vec<ApparatusCapabilityProfile>,
    capabilities: &[String],
) -> Vec<ApparatusCapabilityProfile> {
    let mut normalized = Vec::new();
    for mut profile in profiles {
        profile.code = profile.code.trim().to_ascii_lowercase();
        if profile.code.is_empty()
            || profile.level == 0
            || profile
                .valid_from_unix
                .zip(profile.valid_to_unix)
                .is_some_and(|(starts_at, ends_at)| ends_at <= starts_at)
            || normalized.iter().any(|item: &ApparatusCapabilityProfile| {
                item.code == profile.code
                    && item.valid_from_unix == profile.valid_from_unix
            })
        {
            continue;
        }
        profile.level = profile.level.clamp(1, 100);
        normalized.push(profile);
    }
    for capability in capabilities {
        let code = capability.trim().to_ascii_lowercase();
        if code.is_empty()
            || normalized
                .iter()
                .any(|profile| profile.code == code && profile.valid_from_unix.is_none())
        {
            continue;
        }
        normalized.push(ApparatusCapabilityProfile {
            code,
            level: 1,
            valid_from_unix: None,
            valid_to_unix: None,
            enabled: true,
        });
    }
    normalized.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then(left.valid_from_unix.cmp(&right.valid_from_unix))
    });
    normalized
}

pub fn custom_apparatus_id(name: &str) -> String {
    format!("apparatus:{}", name.trim().to_lowercase())
}

pub fn apparatus_id_for_name(name: &str) -> String {
    DEFAULT_APPARATUS
        .into_iter()
        .find(|(_, default_name)| default_name.eq_ignore_ascii_case(name.trim()))
        .map(|(id, _)| id.to_string())
        .unwrap_or_else(|| custom_apparatus_id(name))
}

fn normalize_requested_apparatus_id(
    requested_id: Option<&str>,
) -> Result<Option<String>, ApparatusGroupError> {
    let Some(id) = requested_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    if !id.starts_with("apparatus:")
        || id.starts_with("apparatus:default:")
        || id.chars().any(char::is_control)
    {
        return Err(ApparatusGroupError::InvalidApparatus);
    }
    Ok(Some(id.to_string()))
}

fn is_invalid_legacy_apparatus_name(value: &str) -> bool {
    matches!(
        value.trim().to_lowercase().as_str(),
        "7 ta rangli pechat" | "8 ta rangli pechat" | "9 ta rangli pechat"
    )
}

fn group_is_bosma(group: &ApparatusGroup) -> bool {
    pechat::is_pechat_apparatus(&group.name)
        || group.apparatus.iter().any(|item| {
            pechat::is_pechat_apparatus(item)
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
    apparatus_ids: RwLock<BTreeMap<String, String>>,
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
        self.put_apparatus_with_id(None, name, &apparatus_master_data_for_name(name))
            .await
            .map(|_| name.trim().to_string())
    }

    async fn apparatus_catalog(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ApparatusCatalogEntry>, ApparatusGroupError> {
        let names = self.apparatus(query, limit).await?;
        let master_data = self.apparatus_master_data.read().await;
        let apparatus_ids = self.apparatus_ids.read().await;
        Ok(names
            .into_iter()
            .enumerate()
            .map(|(sort_order, name)| ApparatusCatalogEntry {
                id: apparatus_ids
                    .get(&name.to_lowercase())
                    .cloned()
                    .unwrap_or_else(|| custom_apparatus_id(&name)),
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
        self.put_apparatus_with_id(None, name, master)
            .await
            .map(|_| name.trim().to_string())
    }

    async fn put_apparatus_with_id(
        &self,
        requested_id: Option<&str>,
        name: &str,
        master: &ApparatusMasterData,
    ) -> Result<String, ApparatusGroupError> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(ApparatusGroupError::MissingApparatus);
        }
        let key = name.to_lowercase();
        let requested_id = requested_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string);
        let mut apparatus = self.apparatus.write().await;
        let mut apparatus_ids = self.apparatus_ids.write().await;
        let mut master_data = self.apparatus_master_data.write().await;
        let previous_key = requested_id.as_deref().and_then(|id| {
            apparatus_ids
                .iter()
                .find_map(|(key, value)| (value == id).then_some(key.clone()))
        });
        if let Some(previous_key) = previous_key.filter(|previous| previous != &key) {
            apparatus.retain(|item| item.to_lowercase() != previous_key);
            apparatus_ids.remove(&previous_key);
            master_data.remove(&previous_key);
        }
        let stable_id = apparatus_ids
            .get(&key)
            .cloned()
            .or(requested_id)
            .unwrap_or_else(|| custom_apparatus_id(&name));
        if !apparatus
            .iter()
            .any(|item| item.to_lowercase() == key)
        {
            apparatus.push(name.clone());
            apparatus.sort_by_key(|item| item.to_lowercase());
        }
        apparatus_ids.insert(key.clone(), stable_id.clone());
        master_data.insert(key, master.clone());
        Ok(stable_id)
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
                    id: None,
                    name: "7 ta rangli pechat".to_string(),
                    master: ApparatusMasterData::default(),
                })
                .await,
            Err(ApparatusGroupError::InvalidApparatus)
        );
    }

    #[tokio::test]
    async fn flexo_apparatus_group_is_canonicalized_as_bosma() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        let service = ApparatusGroupService::new(store);

        let saved = service
            .upsert_group(ApparatusGroupUpsert {
                name: "Flexo bosma".to_string(),
                apparatus: vec!["Flexo pechat".to_string()],
            })
            .await
            .expect("flexo group");

        assert_eq!(saved.name, "Bosma aparat");
        assert!(saved.apparatus.iter().any(|item| item == "Flexo pechat"));
    }

    #[test]
    fn explicit_flexo_master_data_is_promoted_to_printing_capabilities() {
        let master = normalize_apparatus_master_data(
            ApparatusMasterData {
                family: String::new(),
                kind: "flexo".to_string(),
                capabilities: Vec::new(),
                capability_profiles: vec![ApparatusCapabilityProfile {
                    code: "flexo".to_string(),
                    level: 3,
                    valid_from_unix: None,
                    valid_to_unix: None,
                    enabled: true,
                }],
                color_stations: None,
            },
            "Maxsus liniya 1",
        );

        assert_eq!(master.family, "pechat");
        assert_eq!(master.kind, "flexo");
        assert!(master.capabilities.iter().any(|item| item == "print"));
        assert!(master.capabilities.iter().any(|item| item == "pechat"));
        let flexo = master
            .capability_profiles
            .iter()
            .find(|profile| profile.code == "flexo")
            .expect("flexo capability profile");
        assert_eq!(flexo.level, 3);
        assert!(flexo.is_valid_at(1_700_000_000));
    }

    #[test]
    fn apparatus_master_options_keep_family_kind_and_capability_values_canonical() {
        let options = apparatus_master_options();

        assert_eq!(
            options.kinds_by_family.get("pechat"),
            Some(&vec!["color_pechat".to_string(), "flexo".to_string()])
        );
        assert!(options.capabilities.iter().any(|item| item == "print"));
        assert_eq!(options.color_stations_min, 1);
        assert_eq!(options.color_stations_max, 24);
    }

    #[tokio::test]
    async fn apparatus_upsert_rejects_unknown_master_data() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        let service = ApparatusGroupService::new(store);

        let invalid_family = service
            .upsert_apparatus(ApparatusUpsert {
                id: None,
                name: "Invalid family".to_string(),
                master: ApparatusMasterData {
                    family: "dshjkhgdsjhjksdh".to_string(),
                    kind: "other".to_string(),
                    capabilities: vec!["apparatus".to_string()],
                    capability_profiles: Vec::new(),
                    color_stations: None,
                },
            })
            .await;
        assert_eq!(invalid_family, Err(ApparatusGroupError::InvalidFamily));

        let invalid_capability = service
            .upsert_apparatus(ApparatusUpsert {
                id: None,
                name: "Invalid capability".to_string(),
                master: ApparatusMasterData {
                    family: "other".to_string(),
                    kind: "other".to_string(),
                    capabilities: vec!["hgjhjkd".to_string()],
                    capability_profiles: Vec::new(),
                    color_stations: None,
                },
            })
            .await;
        assert_eq!(
            invalid_capability,
            Err(ApparatusGroupError::InvalidCapability)
        );

        let invalid_color_stations = service
            .upsert_apparatus(ApparatusUpsert {
                id: None,
                name: "Invalid color stations".to_string(),
                master: ApparatusMasterData {
                    family: "pechat".to_string(),
                    kind: "color_pechat".to_string(),
                    capabilities: vec!["print".to_string(), "pechat".to_string()],
                    capability_profiles: Vec::new(),
                    color_stations: Some(25),
                },
            })
            .await;
        assert_eq!(
            invalid_color_stations,
            Err(ApparatusGroupError::InvalidColorStations)
        );
    }
}
