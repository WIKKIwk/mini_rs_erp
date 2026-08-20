use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use data_encoding::HEXLOWER;
use serde::{Deserialize, Serialize};
use thiserror::Error;
#[cfg(test)]
use tokio::sync::RwLock;

use crate::core::apparatus_standard::{
    ApparatusClassification, ApparatusDisplayMetadata, ApparatusFamily, ApparatusId, ApparatusKind,
    CanonicalApparatus, CapabilityCode, CapacityConfiguration, CatalogSource, OperationalPolicies,
    QueuePolicy, ToolingPolicy, TrainingReference, Versioning, aas_package_metadata_for_apparatus,
};

pub const APPARATUS_COLOR_STATIONS_MIN: u8 = 7;
pub const APPARATUS_COLOR_STATIONS_MAX: u8 = 9;
const FACTORY_MAP_OBJECT_ID_MAX_LENGTH: usize = 128;
const DEFAULT_APPARATUS: [(&str, &str); 10] = [
    ("apparatus:default:bosma_7", "7 ta rangli bosma aparat"),
    ("apparatus:default:bosma_8", "8 ta rangli bosma aparat"),
    ("apparatus:default:bosma_9", "9 ta rangli bosma aparat"),
    ("apparatus:default:asset-004", "Extruder laminatsiya"),
    ("apparatus:default:asset-005", "Flexo pechat"),
    ("apparatus:default:holodniy_kley", "Holodniy kley aparat"),
    ("apparatus:default:asset-007", "Laminatsiya 1"),
    ("apparatus:default:asset-008", "Laminatsiya 2"),
    ("apparatus:default:paket", "Paket aparat"),
    ("apparatus:default:asset-010", "Rezka"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApparatusSource {
    Default,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factory_map_object_id: Option<String>,
    #[serde(default)]
    pub training_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooling_policy: Option<ToolingPolicy>,
    /// The canonical runtime capacity profile. A missing value is not a
    /// runtime default; it means the apparatus master is incomplete and must
    /// fail canonical resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<CapacityConfiguration>,
}

impl Default for ApparatusMasterData {
    fn default() -> Self {
        Self {
            family: "other".to_string(),
            kind: "other".to_string(),
            capabilities: vec!["apparatus".to_string()],
            capability_profiles: Vec::new(),
            color_stations: None,
            factory_map_object_id: None,
            training_enabled: false,
            tooling_policy: None,
            capacity: Some(default_capacity_configuration()),
        }
    }
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
            && self.valid_to_unix.is_none_or(|ends_at| at_unix < ends_at)
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
    /// Canonical apparatus IDs. The field name is retained for transport
    /// compatibility; values are never display names.
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
    #[error("apparatus revision conflict")]
    Conflict,
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
    ) -> Result<Vec<ApparatusCatalogEntry>, ApparatusGroupError>;
    /// Load the persisted canonical configuration by immutable ID. This is a
    /// separate contract from the legacy catalog/master projection above;
    /// implementations must not derive it from a display name.
    async fn canonical_apparatus_by_id(
        &self,
        _apparatus_id: &ApparatusId,
    ) -> Result<Option<CanonicalApparatus>, ApparatusGroupError> {
        Ok(None)
    }
    /// Persist the canonical configuration alongside the compatibility catalog
    /// projection. Implementations must make this write atomic with any master
    /// projection update; stores that have not completed the canonical cutover
    /// fail closed instead of silently accepting a non-durable configuration.
    async fn put_canonical_apparatus(
        &self,
        _expected_revision: u64,
        _canonical: &CanonicalApparatus,
    ) -> Result<(), ApparatusGroupError> {
        Err(ApparatusGroupError::StoreFailed)
    }
    /// Persist a catalog projection and its canonical configuration as one
    /// store operation. There is deliberately no sequential fallback: a
    /// durable store must override this with a real transaction so a legacy
    /// master row can never commit without its canonical payload.
    async fn put_apparatus_with_canonical(
        &self,
        _expected_revision: Option<u64>,
        _requested_id: Option<&str>,
        _name: &str,
        _master: &ApparatusMasterData,
        _canonical: &CanonicalApparatus,
    ) -> Result<String, ApparatusGroupError> {
        Err(ApparatusGroupError::StoreFailed)
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
            let groups = default_apparatus_groups();
            self.validate_group_apparatus_ids(&groups).await?;
            return Ok(groups);
        }
        let groups = normalize_groups(groups)?;
        self.validate_group_apparatus_ids(&groups).await?;
        Ok(groups)
    }

    pub async fn upsert_group(
        &self,
        input: ApparatusGroupUpsert,
    ) -> Result<ApparatusGroup, ApparatusGroupError> {
        let group = normalize_group(input)?;
        self.validate_group_apparatus_ids(std::slice::from_ref(&group))
            .await?;
        self.store.put_group(group.clone()).await?;
        Ok(group)
    }

    async fn validate_group_apparatus_ids(
        &self,
        groups: &[ApparatusGroup],
    ) -> Result<(), ApparatusGroupError> {
        for apparatus_id in groups.iter().flat_map(|group| group.apparatus.iter()) {
            let apparatus_id = ApparatusId::new(apparatus_id.clone())
                .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
            if self
                .canonical_apparatus_by_id(&apparatus_id)
                .await?
                .is_none()
            {
                return Err(ApparatusGroupError::InvalidApparatus);
            }
        }
        Ok(())
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
        // Persisted master data is keyed by the canonical ID. Display names
        // are used only for the optional search filter and are never a merge
        // or update key.
        let mut stored_by_id = BTreeMap::<ApparatusId, ApparatusCatalogEntry>::new();
        let mut display_names = BTreeMap::<String, ApparatusId>::new();
        for item in self.store.apparatus_catalog("", 10_000).await? {
            let mut item = item;
            let id = ApparatusId::new(item.id.clone())
                .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
            // A catalog row is only a compatibility projection. It must not
            // become a live read path when its canonical payload is missing.
            // Store migrations are responsible for materializing this payload;
            // an incomplete or legacy-only row fails closed here.
            let Some(canonical) = self.store.canonical_apparatus_by_id(&id).await? else {
                return Err(ApparatusGroupError::InvalidApparatus);
            };
            if canonical.identity.id != id {
                return Err(ApparatusGroupError::InvalidApparatus);
            }
            canonical
                .validate()
                .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
            item.id = id.to_string();
            item.name = canonical.identity.display.display_name.clone();
            item.source = match canonical.provenance.source {
                CatalogSource::Default => ApparatusSource::Default,
                CatalogSource::Custom => ApparatusSource::Custom,
            };
            item.sort_order = canonical.identity.display.catalog_order as usize;
            item.master = apparatus_master_data_from_canonical(&canonical)?;
            let name = item.name.trim().to_string();
            if name.is_empty() || is_invalid_legacy_apparatus_name(&name) {
                return Err(ApparatusGroupError::InvalidApparatus);
            }
            if stored_by_id.contains_key(&id) {
                return Err(ApparatusGroupError::InvalidApparatus);
            }
            let display_key = name.to_lowercase();
            if display_names.insert(display_key, id.clone()).is_some() {
                return Err(ApparatusGroupError::InvalidApparatus);
            }
            stored_by_id.insert(
                id.clone(),
                ApparatusCatalogEntry {
                    id: id.to_string(),
                    name,
                    source: item.source,
                    sort_order: item.sort_order,
                    master: item.master,
                },
            );
        }

        let mut seen_ids = BTreeSet::<ApparatusId>::new();
        let mut result = Vec::new();
        for item in default_apparatus_catalog() {
            let item_id = ApparatusId::new(item.id.clone())
                .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
            if !seen_ids.insert(item_id.clone()) {
                return Err(ApparatusGroupError::InvalidApparatus);
            }
            // A static default definition is not a live apparatus record. It
            // is only an ordering/migration reference; without a persisted
            // canonical payload the default is intentionally not exposed.
            let Some(item) = stored_by_id.remove(&item_id) else {
                continue;
            };
            if !needle.is_empty() && !item.name.to_lowercase().contains(&needle) {
                continue;
            }
            result.push(item);
            if result.len() >= limit {
                return Ok(result);
            }
        }

        for (_, item) in stored_by_id {
            if !needle.is_empty() && !item.name.to_lowercase().contains(&needle) {
                continue;
            }
            let item_id = ApparatusId::new(item.id.clone())
                .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
            if !seen_ids.insert(item_id) {
                return Err(ApparatusGroupError::InvalidApparatus);
            }
            result.push(item);
            if result.len() >= limit {
                break;
            }
        }
        Ok(result)
    }

    pub async fn canonical_apparatus_by_id(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<CanonicalApparatus>, ApparatusGroupError> {
        if let Some(canonical) = self.store.canonical_apparatus_by_id(apparatus_id).await? {
            if canonical.identity.id != *apparatus_id {
                return Err(ApparatusGroupError::InvalidApparatus);
            }
            canonical
                .validate()
                .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
            return Ok(Some(canonical));
        }

        // A missing persisted canonical payload is intentionally not promoted
        // from the legacy catalog/master projection. In particular, an
        // in-process cache must not turn a failed or partial write into live
        // configuration after a restart or another writer's update.
        Ok(None)
    }

    /// Persist a validated canonical configuration through the catalog owner.
    /// Runtime policy/capacity writers use this method so the canonical record
    /// remains the only live source of apparatus configuration.
    pub async fn put_canonical_apparatus(
        &self,
        expected_revision: u64,
        canonical: CanonicalApparatus,
    ) -> Result<CanonicalApparatus, ApparatusGroupError> {
        canonical
            .validate()
            .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
        if canonical.versioning.revision
            != expected_revision
                .checked_add(1)
                .ok_or(ApparatusGroupError::Conflict)?
        {
            return Err(ApparatusGroupError::Conflict);
        }
        self.store
            .put_canonical_apparatus(expected_revision, &canonical)
            .await?;
        Ok(canonical)
    }

    /// Mutate one persisted canonical configuration without exposing the
    /// compatibility master as an alternate write path. The immutable ID is
    /// protected, the revision is advanced exactly once, and the final record
    /// is validated before the store receives it.
    pub async fn mutate_canonical_apparatus<F>(
        &self,
        apparatus_id: &ApparatusId,
        expected_revision: u64,
        mutate: F,
    ) -> Result<CanonicalApparatus, ApparatusGroupError>
    where
        F: FnOnce(&mut CanonicalApparatus) -> Result<(), ApparatusGroupError>,
    {
        let existing = self
            .canonical_apparatus_by_id(apparatus_id)
            .await?
            .ok_or(ApparatusGroupError::MissingApparatus)?;
        if existing.versioning.revision != expected_revision {
            return Err(ApparatusGroupError::Conflict);
        }
        let next_revision = existing
            .versioning
            .revision
            .checked_add(1)
            .ok_or(ApparatusGroupError::InvalidApparatus)?;
        let mut candidate = existing;
        mutate(&mut candidate)?;
        if candidate.identity.id != *apparatus_id {
            return Err(ApparatusGroupError::InvalidApparatus);
        }
        candidate.versioning.revision = next_revision;
        self.put_canonical_apparatus(expected_revision, candidate)
            .await
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
        let default_name = requested_id
            .as_ref()
            .and_then(default_apparatus_name_for_id);
        if requested_id
            .as_ref()
            .is_some_and(|id| id.as_str().starts_with("apparatus:default:"))
            && !default_name.is_some_and(|default_name| default_name.eq_ignore_ascii_case(&name))
        {
            return Err(ApparatusGroupError::InvalidApparatus);
        }
        validate_explicit_apparatus_master_data(&input.master)?;
        let master = if let Some(default_name) = default_name {
            let factory_map_object_id = input.master.factory_map_object_id.clone();
            let training_enabled = input.master.training_enabled;
            let mut canonical = default_apparatus_master_data_for_id(
                requested_id
                    .as_ref()
                    .expect("default id is present")
                    .as_str(),
            );
            canonical.factory_map_object_id = factory_map_object_id;
            canonical.training_enabled = training_enabled;
            normalize_apparatus_master_data(canonical, default_name)
        } else {
            normalize_apparatus_master_data(input.master, &name)
        };
        validate_apparatus_master_data(&master)?;
        let resolved_id = requested_id.clone().unwrap_or_else(|| {
            ApparatusId::new(custom_apparatus_id(&name))
                .expect("custom apparatus id generator must produce canonical IDs")
        });
        let source = if default_name.is_some() {
            ApparatusSource::Default
        } else {
            ApparatusSource::Custom
        };
        // Check display uniqueness before the durable write. The catalog
        // projection rejects ambiguous names, so doing this only after the
        // write would return an error while leaving an invalid row behind.
        for existing in self.apparatus_catalog("", 10_000).await? {
            if existing.id != resolved_id.as_str()
                && existing.name.trim().to_lowercase() == name.to_lowercase()
            {
                return Err(ApparatusGroupError::InvalidApparatus);
            }
        }
        let existing_canonical = if requested_id.is_some() {
            self.canonical_apparatus_by_id(&resolved_id).await?
        } else {
            None
        };
        let expected_revision = existing_canonical
            .as_ref()
            .map(|canonical| canonical.versioning.revision);
        let mut canonical = canonical_apparatus(&resolved_id, &name, &master, source, 0)?;
        if let Some(existing) = existing_canonical {
            // Queue/material policies are canonical-only fields and are not
            // present in the compatibility master DTO. Preserve them across
            // display/master edits instead of silently resetting live policy.
            canonical.policies.queue = existing.policies.queue;
            canonical.policies.material = existing.policies.material;
            canonical.versioning.revision = existing
                .versioning
                .revision
                .checked_add(1)
                .ok_or(ApparatusGroupError::InvalidApparatus)?;
        }
        if let Some(object_id) = master.factory_map_object_id.as_deref() {
            for existing in self.apparatus_catalog("", 10_000).await? {
                let same_apparatus = existing.id.trim() == resolved_id.as_str();
                if !same_apparatus
                    && existing
                        .master
                        .factory_map_object_id
                        .as_deref()
                        .is_some_and(|existing_id| existing_id.trim() == object_id)
                {
                    return Err(ApparatusGroupError::InvalidApparatus);
                }
            }
        }
        let id = self
            .store
            .put_apparatus_with_canonical(
                expected_revision,
                Some(resolved_id.as_str()),
                &name,
                &master,
                &canonical,
            )
            .await?;
        self.apparatus_catalog("", 10_000)
            .await?
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or(ApparatusGroupError::InvalidApparatus)
    }
}

fn normalize_group(input: ApparatusGroupUpsert) -> Result<ApparatusGroup, ApparatusGroupError> {
    let name = canonical_group_name(&input.name);
    if name.is_empty() {
        return Err(ApparatusGroupError::MissingName);
    }
    let mut seen = BTreeSet::<ApparatusId>::new();
    let mut apparatus = input
        .apparatus
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .map(|item| {
            ApparatusId::new(item.clone())
                .map(|id| (id.clone(), id.to_string()))
                .map_err(|_| ApparatusGroupError::InvalidApparatus)
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|(id, _)| seen.insert(id.clone()))
        .map(|(_, id)| id)
        .collect::<Vec<_>>();
    if name == "Bosma aparat" {
        apparatus = default_apparatus_groups()[0].apparatus.clone();
    } else if apparatus.is_empty() {
        return Err(ApparatusGroupError::MissingApparatus);
    }
    Ok(ApparatusGroup { name, apparatus })
}

fn normalize_groups(
    groups: Vec<ApparatusGroup>,
) -> Result<Vec<ApparatusGroup>, ApparatusGroupError> {
    let mut normalized = Vec::<ApparatusGroup>::new();
    let mut names = BTreeSet::new();
    for group in groups.into_iter() {
        let group = normalize_stored_group(group)?;
        let name_key = group.name.to_lowercase();
        if !names.insert(name_key) {
            return Err(ApparatusGroupError::InvalidApparatus);
        }
        normalized.push(group);
    }
    Ok(normalized)
}

fn normalize_stored_group(
    mut group: ApparatusGroup,
) -> Result<ApparatusGroup, ApparatusGroupError> {
    group.name = canonical_group_name(&group.name);
    if group.name.is_empty() {
        return Err(ApparatusGroupError::MissingName);
    }
    let mut seen = BTreeSet::<ApparatusId>::new();
    group.apparatus = group
        .apparatus
        .into_iter()
        .map(|item| {
            let id = ApparatusId::new(item).map_err(|_| ApparatusGroupError::InvalidApparatus)?;
            if !seen.insert(id.clone()) {
                return Err(ApparatusGroupError::InvalidApparatus);
            }
            Ok(id.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if group.name == "Bosma aparat" {
        group.apparatus = default_apparatus_groups()[0].apparatus.clone();
    } else if group.apparatus.is_empty() {
        return Err(ApparatusGroupError::MissingApparatus);
    }
    Ok(group)
}

fn default_apparatus_groups() -> Vec<ApparatusGroup> {
    vec![
        ApparatusGroup {
            name: "Bosma aparat".to_string(),
            apparatus: vec![
                "apparatus:default:bosma_7".to_string(),
                "apparatus:default:bosma_8".to_string(),
                "apparatus:default:bosma_9".to_string(),
                "apparatus:default:asset-005".to_string(),
            ],
        },
        ApparatusGroup {
            name: "Laminatsiya".to_string(),
            apparatus: vec![
                "apparatus:default:asset-007".to_string(),
                "apparatus:default:asset-008".to_string(),
            ],
        },
        ApparatusGroup {
            name: "Rezka".to_string(),
            apparatus: vec!["apparatus:default:asset-010".to_string()],
        },
    ]
}

fn canonical_group_name(value: &str) -> String {
    let name = value.trim();
    match name.to_lowercase().as_str() {
        "pechat" | "bosma" | "bosma aparat" | "bosma apparat" | "flexo bosma" => {
            "Bosma aparat".to_string()
        }
        "laminatsiya" | "laminatsiya apparatlar" | "laminatsiya apparat" => {
            "Laminatsiya".to_string()
        }
        "rezka" | "rezka apparat" => "Rezka".to_string(),
        _ => name.to_string(),
    }
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
            master: default_apparatus_master_data_for_id(id),
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

    if let Some(color_stations) = master.color_stations
        && !(APPARATUS_COLOR_STATIONS_MIN..=APPARATUS_COLOR_STATIONS_MAX).contains(&color_stations)
    {
        return Err(ApparatusGroupError::InvalidColorStations);
    }
    if master
        .factory_map_object_id
        .as_deref()
        .is_some_and(|object_id| {
            let object_id = object_id.trim();
            object_id.chars().count() > FACTORY_MAP_OBJECT_ID_MAX_LENGTH
                || object_id.chars().any(char::is_control)
        })
    {
        return Err(ApparatusGroupError::InvalidApparatus);
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
    if kind == "color_pechat"
        && !master.color_stations.is_some_and(|stations| {
            (APPARATUS_COLOR_STATIONS_MIN..=APPARATUS_COLOR_STATIONS_MAX).contains(&stations)
        })
    {
        return Err(ApparatusGroupError::InvalidColorStations);
    }
    if kind != "color_pechat" && master.color_stations.is_some() {
        return Err(ApparatusGroupError::InvalidColorStations);
    }
    if master.capacity.is_none() {
        return Err(ApparatusGroupError::InvalidApparatus);
    }
    Ok(())
}

fn default_apparatus_master_data_for_id(id: &str) -> ApparatusMasterData {
    match id {
        "apparatus:default:bosma_7" => {
            apparatus_master_data("pechat", "color_pechat", ["print", "pechat"], Some(7))
        }
        "apparatus:default:bosma_8" => {
            apparatus_master_data("pechat", "color_pechat", ["print", "pechat"], Some(8))
        }
        "apparatus:default:bosma_9" => {
            apparatus_master_data("pechat", "color_pechat", ["print", "pechat"], Some(9))
        }
        "apparatus:default:asset-005" => {
            apparatus_master_data("pechat", "flexo", ["print", "pechat", "flexo"], None)
        }
        "apparatus:default:asset-004" => {
            apparatus_master_data("laminatsiya", "extruder_laminatsiya", ["laminate"], None)
        }
        "apparatus:default:holodniy_kley" => {
            apparatus_master_data("kley", "holodniy_kley", ["glue"], None)
        }
        "apparatus:default:asset-007" | "apparatus:default:asset-008" => {
            apparatus_master_data("laminatsiya", "laminatsiya", ["laminate"], None)
        }
        "apparatus:default:paket" => apparatus_master_data("paket", "paket", ["package"], None),
        "apparatus:default:asset-010" => apparatus_master_data("rezka", "rezka", ["cut"], None),
        _ => apparatus_master_data("other", "other", ["apparatus"], None),
    }
}

pub(crate) fn apparatus_master_data_from_canonical(
    canonical: &CanonicalApparatus,
) -> Result<ApparatusMasterData, ApparatusGroupError> {
    let master = ApparatusMasterData {
        family: apparatus_family_name(canonical.classification.family).to_string(),
        kind: apparatus_kind_name(canonical.classification.kind).to_string(),
        capabilities: canonical
            .capabilities
            .iter()
            .map(|code| capability_code_name(*code).to_string())
            .collect(),
        capability_profiles: canonical
            .capability_profiles
            .iter()
            .map(|profile| ApparatusCapabilityProfile {
                code: capability_code_name(profile.code).to_string(),
                level: profile.level,
                valid_from_unix: profile.valid_from_unix,
                valid_to_unix: profile.valid_to_unix,
                enabled: profile.enabled,
            })
            .collect(),
        color_stations: canonical.classification.color_stations,
        factory_map_object_id: canonical
            .placement
            .as_ref()
            .map(|placement| placement.factory_map_object_id.clone()),
        training_enabled: canonical.training.enabled,
        tooling_policy: Some(canonical.policies.tooling),
        capacity: Some(canonical.capacity.clone()),
    };
    validate_apparatus_master_data(&master)?;
    Ok(master)
}

fn apparatus_family_name(family: ApparatusFamily) -> &'static str {
    match family {
        ApparatusFamily::Pechat => "pechat",
        ApparatusFamily::Laminatsiya => "laminatsiya",
        ApparatusFamily::Rezka => "rezka",
        ApparatusFamily::Paket => "paket",
        ApparatusFamily::Kley => "kley",
        ApparatusFamily::Other => "other",
    }
}

fn apparatus_kind_name(kind: ApparatusKind) -> &'static str {
    match kind {
        ApparatusKind::ColorPechat => "color_pechat",
        ApparatusKind::Flexo => "flexo",
        ApparatusKind::Laminatsiya => "laminatsiya",
        ApparatusKind::ExtruderLaminatsiya => "extruder_laminatsiya",
        ApparatusKind::Rezka => "rezka",
        ApparatusKind::Paket => "paket",
        ApparatusKind::HolodniyKley => "holodniy_kley",
        ApparatusKind::Other => "other",
    }
}

fn capability_code_name(code: CapabilityCode) -> &'static str {
    match code {
        CapabilityCode::Print => "print",
        CapabilityCode::Pechat => "pechat",
        CapabilityCode::Flexo => "flexo",
        CapabilityCode::Laminate => "laminate",
        CapabilityCode::Cut => "cut",
        CapabilityCode::Package => "package",
        CapabilityCode::Glue => "glue",
        CapabilityCode::Apparatus => "apparatus",
    }
}

fn tooling_policy_for_master(
    id: &ApparatusId,
    master: &ApparatusMasterData,
) -> Result<ToolingPolicy, ApparatusGroupError> {
    if let Some(policy) = master.tooling_policy {
        return Ok(policy);
    }
    if id.as_str().starts_with("apparatus:default:") {
        return Ok(match id.as_str() {
            "apparatus:default:bosma_7"
            | "apparatus:default:bosma_8"
            | "apparatus:default:bosma_9"
            | "apparatus:default:asset-005" => ToolingPolicy::QolipScanRequired,
            _ => ToolingPolicy::QolipScanNotRequired,
        });
    }
    match (master.family.as_str(), master.kind.as_str()) {
        ("pechat", "color_pechat" | "flexo") => Ok(ToolingPolicy::QolipScanRequired),
        ("laminatsiya", "laminatsiya" | "extruder_laminatsiya")
        | ("rezka", "rezka")
        | ("paket", "paket")
        | ("kley", "holodniy_kley")
        | ("other", "other") => Ok(ToolingPolicy::QolipScanNotRequired),
        _ => Err(ApparatusGroupError::InvalidKind),
    }
}

fn apparatus_master_data(
    family: &str,
    kind: &str,
    capabilities: impl IntoIterator<Item = &'static str>,
    color_stations: Option<u8>,
) -> ApparatusMasterData {
    let capabilities = capabilities
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    ApparatusMasterData {
        family: family.to_string(),
        kind: kind.to_string(),
        capability_profiles: default_capability_profiles(capabilities.iter().map(String::as_str)),
        capabilities,
        color_stations,
        factory_map_object_id: None,
        training_enabled: false,
        tooling_policy: None,
        capacity: Some(default_capacity_configuration()),
    }
}

fn default_capacity_configuration() -> CapacityConfiguration {
    CapacityConfiguration {
        capacity_slots: 1,
        setup_minutes: 0,
        cleanup_minutes: 0,
        efficiency_percent: 100,
        finite_capacity: true,
        working_windows: Vec::new(),
    }
}

fn canonical_apparatus(
    id: &ApparatusId,
    name: &str,
    master: &ApparatusMasterData,
    source: ApparatusSource,
    catalog_order: usize,
) -> Result<CanonicalApparatus, ApparatusGroupError> {
    let family = match master.family.as_str() {
        "pechat" => ApparatusFamily::Pechat,
        "laminatsiya" => ApparatusFamily::Laminatsiya,
        "rezka" => ApparatusFamily::Rezka,
        "paket" => ApparatusFamily::Paket,
        "kley" => ApparatusFamily::Kley,
        "other" => ApparatusFamily::Other,
        _ => return Err(ApparatusGroupError::InvalidFamily),
    };
    let kind = match master.kind.as_str() {
        "color_pechat" => ApparatusKind::ColorPechat,
        "flexo" => ApparatusKind::Flexo,
        "laminatsiya" => ApparatusKind::Laminatsiya,
        "extruder_laminatsiya" => ApparatusKind::ExtruderLaminatsiya,
        "rezka" => ApparatusKind::Rezka,
        "paket" => ApparatusKind::Paket,
        "holodniy_kley" => ApparatusKind::HolodniyKley,
        "other" => ApparatusKind::Other,
        _ => return Err(ApparatusGroupError::InvalidKind),
    };
    let capabilities = master
        .capabilities
        .iter()
        .map(|code| canonical_capability_code(code))
        .collect::<Result<Vec<_>, _>>()?;
    let capability_profiles = master
        .capability_profiles
        .iter()
        .map(|profile| {
            Ok(crate::core::apparatus_standard::CapabilityProfile {
                code: canonical_capability_code(&profile.code)?,
                level: profile.level,
                valid_from_unix: profile.valid_from_unix,
                valid_to_unix: profile.valid_to_unix,
                enabled: profile.enabled,
            })
        })
        .collect::<Result<Vec<_>, ApparatusGroupError>>()?;
    let tooling = tooling_policy_for_master(id, master)?;
    let canonical = CanonicalApparatus {
        identity: crate::core::apparatus_standard::ApparatusIdentity {
            id: id.clone(),
            display: ApparatusDisplayMetadata {
                display_name: name.to_string(),
                description: String::new(),
                catalog_order: catalog_order as u32,
            },
        },
        classification: ApparatusClassification {
            family,
            kind,
            color_stations: master.color_stations,
        },
        capabilities,
        capability_profiles,
        policies: OperationalPolicies {
            queue: QueuePolicy::StrictSequence,
            material: Default::default(),
            tooling,
        },
        capacity: master
            .capacity
            .clone()
            .ok_or(ApparatusGroupError::InvalidApparatus)?,
        placement: master
            .factory_map_object_id
            .as_ref()
            .map(
                |factory_map_object_id| crate::core::apparatus_standard::PlacementReference {
                    factory_map_object_id: factory_map_object_id.clone(),
                },
            ),
        training: TrainingReference {
            enabled: master.training_enabled,
        },
        provenance: crate::core::apparatus_standard::Provenance {
            source: match source {
                ApparatusSource::Default => CatalogSource::Default,
                ApparatusSource::Custom => CatalogSource::Custom,
            },
            source_ref: None,
        },
        versioning: Versioning { revision: 1 },
        aas: aas_package_metadata_for_apparatus(id),
    };
    canonical.validate().map_err(|error| match error {
        crate::core::apparatus_standard::ApparatusValidationError::InvalidColorStations => {
            ApparatusGroupError::InvalidColorStations
        }
        crate::core::apparatus_standard::ApparatusValidationError::ClassificationConflict => {
            ApparatusGroupError::InvalidKind
        }
        crate::core::apparatus_standard::ApparatusValidationError::InvalidCapabilities
        | crate::core::apparatus_standard::ApparatusValidationError::InvalidCapabilityProfile => {
            ApparatusGroupError::InvalidCapability
        }
        _ => ApparatusGroupError::InvalidApparatus,
    })?;
    Ok(canonical)
}

fn canonical_capability_code(
    code: &str,
) -> Result<crate::core::apparatus_standard::CapabilityCode, ApparatusGroupError> {
    match code {
        "print" => Ok(CapabilityCode::Print),
        "pechat" => Ok(CapabilityCode::Pechat),
        "flexo" => Ok(CapabilityCode::Flexo),
        "laminate" => Ok(CapabilityCode::Laminate),
        "cut" => Ok(CapabilityCode::Cut),
        "package" => Ok(CapabilityCode::Package),
        "glue" => Ok(CapabilityCode::Glue),
        "apparatus" => Ok(CapabilityCode::Apparatus),
        _ => Err(ApparatusGroupError::InvalidCapability),
    }
}

pub fn normalize_apparatus_master_data(
    mut master: ApparatusMasterData,
    _display_name: &str,
) -> ApparatusMasterData {
    master.family = master.family.trim().to_lowercase();
    master.kind = master.kind.trim().to_lowercase();
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
    if master.kind == "flexo" || master.capabilities.iter().any(|item| item == "flexo") {
        master.family = "pechat".to_string();
        master.kind = "flexo".to_string();
        for capability in ["print", "pechat", "flexo"] {
            if !master.capabilities.iter().any(|item| item == capability) {
                master.capabilities.push(capability.to_string());
            }
        }
    }
    master.capability_profiles =
        normalize_capability_profiles(master.capability_profiles, &master.capabilities);
    if master.color_stations.is_none() {
        master.color_stations = None;
    }
    master.factory_map_object_id = master
        .factory_map_object_id
        .map(|object_id| object_id.trim().to_string())
        .filter(|object_id| !object_id.is_empty());
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
                item.code == profile.code && item.valid_from_unix == profile.valid_from_unix
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

pub fn custom_apparatus_id(_name: &str) -> String {
    format!(
        "apparatus:custom:{}",
        HEXLOWER.encode(&rand::random::<[u8; 16]>())
    )
}

fn normalize_requested_apparatus_id(
    requested_id: Option<&str>,
) -> Result<Option<ApparatusId>, ApparatusGroupError> {
    let Some(id) = requested_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    ApparatusId::new(id.to_string())
        .map(Some)
        .map_err(|_| ApparatusGroupError::InvalidApparatus)
}

fn default_apparatus_name_for_id(id: &ApparatusId) -> Option<&'static str> {
    DEFAULT_APPARATUS
        .into_iter()
        .find_map(|(default_id, name)| (default_id == id.as_str()).then_some(name))
}

fn is_invalid_legacy_apparatus_name(value: &str) -> bool {
    matches!(
        value.trim().to_lowercase().as_str(),
        "7 ta rangli pechat" | "8 ta rangli pechat" | "9 ta rangli pechat"
    )
}

#[derive(Default)]
#[cfg(test)]
pub struct MemoryApparatusGroupStore {
    groups: RwLock<Vec<ApparatusGroup>>,
    /// Test double keyed by canonical ID. Display names are values only and
    /// therefore cannot silently become an identity or update key.
    apparatus: RwLock<BTreeMap<String, String>>,
    apparatus_master_data: RwLock<BTreeMap<String, ApparatusMasterData>>,
    canonical_apparatus: RwLock<BTreeMap<ApparatusId, CanonicalApparatus>>,
}

#[cfg(test)]
impl MemoryApparatusGroupStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_canonical_defaults() -> Self {
        let store = Self::new();
        {
            let mut apparatus = store.apparatus.try_write().expect("apparatus write lock");
            let mut master_data = store
                .apparatus_master_data
                .try_write()
                .expect("apparatus master write lock");
            let mut canonical_records = store
                .canonical_apparatus
                .try_write()
                .expect("canonical apparatus write lock");
            for (sort_order, (raw_id, name)) in DEFAULT_APPARATUS.into_iter().enumerate() {
                let id = ApparatusId::new(raw_id.to_string()).expect("default apparatus id");
                let master = default_apparatus_master_data_for_id(raw_id);
                let canonical = canonical_apparatus(
                    &id,
                    name,
                    &master,
                    ApparatusSource::Default,
                    sort_order,
                )
                .expect("default canonical apparatus");
                apparatus.insert(id.to_string(), name.to_string());
                master_data.insert(id.to_string(), master);
                canonical_records.insert(id, canonical);
            }
        }
        store
    }

    pub(crate) async fn put_apparatus(&self, name: &str) -> Result<String, ApparatusGroupError> {
        self.put_apparatus_with_id(None, name, &ApparatusMasterData::default())
            .await
    }

    pub(crate) async fn put_apparatus_with_id(
        &self,
        requested_id: Option<&str>,
        name: &str,
        master: &ApparatusMasterData,
    ) -> Result<String, ApparatusGroupError> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(ApparatusGroupError::MissingApparatus);
        }
        let stable_id = requested_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| custom_apparatus_id(&name));
        let stable_apparatus_id = ApparatusId::new(stable_id.clone())
            .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
        let mut apparatus = self.apparatus.write().await;
        let mut master_data = self.apparatus_master_data.write().await;
        apparatus.insert(stable_id.clone(), name);
        master_data.insert(stable_id.clone(), master.clone());
        self.canonical_apparatus
            .write()
            .await
            .remove(&stable_apparatus_id);
        Ok(stable_id)
    }

    async fn seed_canonical_apparatus(
        &self,
        canonical: &CanonicalApparatus,
    ) -> Result<(), ApparatusGroupError> {
        canonical
            .validate()
            .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
        if !self
            .apparatus
            .read()
            .await
            .contains_key(canonical.identity.id.as_str())
        {
            return Err(ApparatusGroupError::MissingApparatus);
        }
        self.canonical_apparatus
            .write()
            .await
            .insert(canonical.identity.id.clone(), canonical.clone());
        Ok(())
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
        let mut result = self
            .apparatus
            .read()
            .await
            .values()
            .filter_map(|item| {
                let name = item.trim();
                if name.is_empty() || (!needle.is_empty() && !name.to_lowercase().contains(&needle))
                {
                    return None;
                }
                Some(name.to_string())
            })
            .collect::<Vec<_>>();
        result.sort_by_key(|name| name.to_lowercase());
        result.truncate(limit);
        Ok(result)
    }

    async fn apparatus_catalog(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ApparatusCatalogEntry>, ApparatusGroupError> {
        let needle = query.trim().to_lowercase();
        let apparatus = self.apparatus.read().await;
        let master_data = self.apparatus_master_data.read().await;
        let canonical_apparatus = self.canonical_apparatus.read().await;
        let mut entries = apparatus
            .iter()
            .filter(|(id, name)| {
                let display_name = canonical_apparatus
                    .values()
                    .find(|canonical| canonical.identity.id.as_str() == id.as_str())
                    .map(|canonical| canonical.identity.display.display_name.as_str())
                    .unwrap_or(name.as_str());
                needle.is_empty() || display_name.to_lowercase().contains(&needle)
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|(id, name)| {
            canonical_apparatus
                .values()
                .find(|canonical| canonical.identity.id.as_str() == id.as_str())
                .map(|canonical| canonical.identity.display.display_name.to_lowercase())
                .unwrap_or_else(|| name.to_lowercase())
        });
        entries
            .into_iter()
            .take(limit)
            .enumerate()
            .map(|(sort_order, (id, name))| {
                ApparatusId::new(id.clone()).map_err(|_| ApparatusGroupError::InvalidApparatus)?;
                let master = master_data
                    .get(id)
                    .cloned()
                    .ok_or(ApparatusGroupError::InvalidApparatus)?;
                Ok(ApparatusCatalogEntry {
                    id: id.clone(),
                    master,
                    name: name.to_string(),
                    source: ApparatusSource::Custom,
                    sort_order,
                })
            })
            .collect::<Result<Vec<_>, ApparatusGroupError>>()
    }

    async fn canonical_apparatus_by_id(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<CanonicalApparatus>, ApparatusGroupError> {
        Ok(self
            .canonical_apparatus
            .read()
            .await
            .get(apparatus_id)
            .cloned())
    }

    async fn put_canonical_apparatus(
        &self,
        expected_revision: u64,
        canonical: &CanonicalApparatus,
    ) -> Result<(), ApparatusGroupError> {
        canonical
            .validate()
            .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
        if canonical.versioning.revision
            != expected_revision
                .checked_add(1)
                .ok_or(ApparatusGroupError::Conflict)?
        {
            return Err(ApparatusGroupError::Conflict);
        }
        let mut canonical_apparatus = self.canonical_apparatus.write().await;
        let Some(existing) = canonical_apparatus.get(&canonical.identity.id) else {
            return Err(ApparatusGroupError::Conflict);
        };
        if existing.versioning.revision != expected_revision {
            return Err(ApparatusGroupError::Conflict);
        }
        canonical_apparatus.insert(canonical.identity.id.clone(), canonical.clone());
        Ok(())
    }

    async fn put_apparatus_with_canonical(
        &self,
        expected_revision: Option<u64>,
        requested_id: Option<&str>,
        name: &str,
        master: &ApparatusMasterData,
        canonical: &CanonicalApparatus,
    ) -> Result<String, ApparatusGroupError> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(ApparatusGroupError::MissingApparatus);
        }
        canonical
            .validate()
            .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
        if let Some(expected_revision) = expected_revision
            && canonical.versioning.revision
                != expected_revision
                    .checked_add(1)
                    .ok_or(ApparatusGroupError::Conflict)?
        {
            return Err(ApparatusGroupError::Conflict);
        }
        let stable_id = requested_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| canonical.identity.id.to_string());
        let stable_apparatus_id = ApparatusId::new(stable_id.clone())
            .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
        if stable_apparatus_id != canonical.identity.id {
            return Err(ApparatusGroupError::InvalidApparatus);
        }

        // The test double keeps all three projections under write locks before
        // publishing the new canonical record. Durable stores must provide the
        // equivalent transaction in their own backend implementation.
        let mut apparatus = self.apparatus.write().await;
        let mut master_data = self.apparatus_master_data.write().await;
        let mut canonical_apparatus = self.canonical_apparatus.write().await;
        match expected_revision {
            Some(expected_revision)
                if canonical_apparatus
                    .get(&stable_apparatus_id)
                    .is_some_and(|current| current.versioning.revision == expected_revision) => {}
            Some(_) => return Err(ApparatusGroupError::Conflict),
            None if apparatus.contains_key(&stable_id) => {
                return Err(ApparatusGroupError::Conflict);
            }
            None => {}
        }
        apparatus.insert(stable_id.clone(), name);
        master_data.insert(stable_id.clone(), master.clone());
        canonical_apparatus.insert(stable_apparatus_id, canonical.clone());
        Ok(stable_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn apparatus_catalog_returns_one_default_each_and_keeps_custom_names() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        let service = ApparatusGroupService::new(store);
        for (id, name) in DEFAULT_APPARATUS {
            service
                .upsert_apparatus(ApparatusUpsert {
                    id: Some(id.to_string()),
                    name: name.to_string(),
                    master: ApparatusMasterData::default(),
                })
                .await
                .expect("seed canonical apparatus");
        }
        service
            .upsert_apparatus(ApparatusUpsert {
                id: None,
                name: "Maxsus aparat".to_string(),
                master: ApparatusMasterData::default(),
            })
            .await
            .expect("seed canonical custom apparatus");

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
        assert!(custom.id.starts_with("apparatus:custom:"));
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
    async fn apparatus_catalog_rejects_custom_name_ambiguous_with_default() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        store
            .put_apparatus_with_id(
                Some("apparatus:custom:duplicate-default-name"),
                "7 ta rangli bosma aparat",
                &default_apparatus_master_data_for_id("apparatus:default:bosma_7"),
            )
            .await
            .expect("seed duplicate display name");
        let service = ApparatusGroupService::new(store);

        assert_eq!(
            service.apparatus_catalog("", 100).await,
            Err(ApparatusGroupError::InvalidApparatus)
        );
    }

    #[tokio::test]
    async fn duplicate_display_name_is_rejected_before_persisting_new_apparatus() {
        let service = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));
        service
            .upsert_apparatus(ApparatusUpsert {
                id: Some("apparatus:custom:existing-name".to_string()),
                name: "Shared display name".to_string(),
                master: ApparatusMasterData::default(),
            })
            .await
            .expect("seed existing apparatus");

        let duplicate_id =
            ApparatusId::new("apparatus:custom:duplicate-name").expect("duplicate candidate id");
        assert_eq!(
            service
                .upsert_apparatus(ApparatusUpsert {
                    id: Some(duplicate_id.to_string()),
                    name: "Shared display name".to_string(),
                    master: ApparatusMasterData::default(),
                })
                .await,
            Err(ApparatusGroupError::InvalidApparatus)
        );
        assert_eq!(
            service
                .canonical_apparatus_by_id(&duplicate_id)
                .await
                .expect("check rejected apparatus"),
            None
        );
    }

    #[test]
    fn all_default_apparatus_have_opaque_ids_capacity_and_aasx_round_trip() {
        let mut submodel_ids = BTreeSet::new();
        for (index, entry) in default_apparatus_catalog().into_iter().enumerate() {
            let id = ApparatusId::new(entry.id.clone()).expect("default apparatus id");
            let canonical =
                canonical_apparatus(&id, &entry.name, &entry.master, entry.source, index)
                    .expect("default apparatus must be canonical");
            assert_eq!(canonical.identity.display.catalog_order, index as u32);
            assert!(entry.master.capacity.is_some());
            canonical.validate().expect("default apparatus validates");
            assert!(submodel_ids.insert(canonical.aas.submodel_id.clone()));
            let package = crate::core::apparatus_standard::aasx::export_aasx(&canonical)
                .expect("default apparatus exports");
            assert_eq!(
                crate::core::apparatus_standard::aasx::import_aasx(&package)
                    .expect("default apparatus imports"),
                canonical
            );
        }
    }

    #[tokio::test]
    async fn flexo_apparatus_group_is_canonicalized_as_bosma() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        for (sort_order, raw_id) in [
            "apparatus:default:bosma_7",
            "apparatus:default:bosma_8",
            "apparatus:default:bosma_9",
            "apparatus:default:asset-005",
        ]
        .into_iter()
        .enumerate()
        {
            let id = ApparatusId::new(raw_id).expect("default apparatus id");
            let name = default_apparatus_name_for_id(&id).expect("default apparatus name");
            let master = default_apparatus_master_data_for_id(id.as_str());
            store
                .put_apparatus_with_id(Some(id.as_str()), name, &master)
                .await
                .expect("seed default catalog projection");
            let canonical =
                canonical_apparatus(&id, name, &master, ApparatusSource::Default, sort_order)
                    .expect("default canonical apparatus");
            store
                .seed_canonical_apparatus(&canonical)
                .await
                .expect("persist default canonical apparatus");
        }
        let service = ApparatusGroupService::new(store);

        let saved = service
            .upsert_group(ApparatusGroupUpsert {
                name: "Flexo bosma".to_string(),
                apparatus: vec!["apparatus:default:asset-005".to_string()],
            })
            .await
            .expect("flexo group");

        assert_eq!(saved.name, "Bosma aparat");
        assert!(
            saved
                .apparatus
                .iter()
                .any(|item| item == "apparatus:default:asset-005")
        );
    }

    #[tokio::test]
    async fn empty_group_store_exposes_canonical_default_groups() {
        let service = ApparatusGroupService::new(Arc::new(
            MemoryApparatusGroupStore::with_canonical_defaults(),
        ));
        assert_eq!(
            service.groups().await.expect("default groups"),
            default_apparatus_groups()
        );
    }

    #[tokio::test]
    async fn empty_group_store_fails_closed_without_canonical_defaults() {
        let service = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));

        assert_eq!(
            service.groups().await,
            Err(ApparatusGroupError::InvalidApparatus)
        );
    }

    #[tokio::test]
    async fn canonical_lookup_does_not_promote_legacy_catalog_projection() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        store
            .put_apparatus_with_id(
                Some("apparatus:default:bosma_7"),
                "7 ta rangli bosma aparat",
                &default_apparatus_master_data_for_id("apparatus:default:bosma_7"),
            )
            .await
            .expect("seed compatibility catalog row");
        let service = ApparatusGroupService::new(store);
        let id = ApparatusId::new("apparatus:default:bosma_7").expect("canonical id");

        assert_eq!(
            service
                .canonical_apparatus_by_id(&id)
                .await
                .expect("canonical lookup"),
            None
        );
    }

    #[tokio::test]
    async fn apparatus_catalog_rejects_legacy_only_projection() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        let id = ApparatusId::new("apparatus:custom:legacy-catalog").expect("canonical id");
        store
            .put_apparatus_with_id(
                Some(id.as_str()),
                "Legacy catalog display",
                &ApparatusMasterData::default(),
            )
            .await
            .expect("seed legacy projection");
        let service = ApparatusGroupService::new(store);

        assert_eq!(
            service.apparatus_catalog("", 50).await,
            Err(ApparatusGroupError::InvalidApparatus)
        );
    }

    #[tokio::test]
    async fn legacy_only_write_cannot_persist_canonical_configuration() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        let id = ApparatusId::new("apparatus:custom:legacy-only").expect("canonical id");
        store
            .put_apparatus_with_id(
                Some(id.as_str()),
                "Legacy-only apparatus",
                &ApparatusMasterData::default(),
            )
            .await
            .expect("seed legacy test projection");
        let service = ApparatusGroupService::new(store.clone());
        let canonical = canonical_apparatus(
            &id,
            "Legacy-only apparatus",
            &ApparatusMasterData::default(),
            ApparatusSource::Custom,
            0,
        )
        .expect("canonical apparatus");

        assert_eq!(service.canonical_apparatus_by_id(&id).await, Ok(None));
        assert_eq!(
            service.put_canonical_apparatus(0, canonical).await,
            Err(ApparatusGroupError::Conflict)
        );
        assert_eq!(service.canonical_apparatus_by_id(&id).await, Ok(None));
    }

    #[tokio::test]
    async fn canonical_catalog_projection_overrides_legacy_master_display() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        let id = ApparatusId::new("apparatus:custom:projection").expect("projection id");
        let legacy_master = ApparatusMasterData::default();
        store
            .put_apparatus_with_id(Some(id.as_str()), "Legacy display", &legacy_master)
            .await
            .expect("seed legacy projection");
        let canonical_master = default_apparatus_master_data_for_id("apparatus:default:bosma_7");
        let canonical = canonical_apparatus(
            &id,
            "Canonical display",
            &canonical_master,
            ApparatusSource::Custom,
            12,
        )
        .expect("canonical apparatus");
        store
            .seed_canonical_apparatus(&canonical)
            .await
            .expect("persist canonical apparatus");
        let service = ApparatusGroupService::new(store);

        let entry = service
            .apparatus_catalog("Canonical display", 10)
            .await
            .expect("catalog projection")
            .pop()
            .expect("canonical entry");

        assert_eq!(entry.id, id.to_string());
        assert_eq!(entry.name, "Canonical display");
        assert_eq!(entry.sort_order, 12);
        assert_eq!(entry.master.family, "pechat");
        assert_eq!(entry.master.kind, "color_pechat");
        assert_eq!(entry.master.color_stations, Some(7));
        assert_eq!(entry.master.capacity, Some(canonical.capacity));
    }

    #[tokio::test]
    async fn canonical_default_projection_is_not_hidden_by_static_catalog() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        let id = ApparatusId::new("apparatus:default:bosma_7").expect("default id");
        let master = default_apparatus_master_data_for_id(id.as_str());
        store
            .put_apparatus_with_id(Some(id.as_str()), "Legacy default display", &master)
            .await
            .expect("seed default compatibility projection");
        let canonical = canonical_apparatus(
            &id,
            "Canonical default display",
            &master,
            ApparatusSource::Default,
            42,
        )
        .expect("canonical default apparatus");
        store
            .seed_canonical_apparatus(&canonical)
            .await
            .expect("persist canonical default apparatus");
        let service = ApparatusGroupService::new(store);

        let entry = service
            .apparatus_catalog("Canonical default display", 10)
            .await
            .expect("catalog projection")
            .into_iter()
            .find(|entry| entry.id == id.as_str())
            .expect("canonical default entry");

        assert_eq!(entry.name, "Canonical default display");
        assert_eq!(entry.source, ApparatusSource::Default);
    }

    #[tokio::test]
    async fn canonical_apparatus_is_persisted_and_read_by_id_after_upsert() {
        let service = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));
        let saved = service
            .upsert_apparatus(ApparatusUpsert {
                id: None,
                name: "Canonical press".to_string(),
                master: ApparatusMasterData::default(),
            })
            .await
            .expect("save apparatus");
        let id = ApparatusId::new(saved.id.clone()).expect("saved canonical id");
        let canonical = service
            .canonical_apparatus_by_id(&id)
            .await
            .expect("canonical lookup")
            .expect("persisted canonical apparatus");

        assert_eq!(canonical.identity.id, id);
        assert_eq!(canonical.identity.display.display_name, "Canonical press");
    }

    #[tokio::test]
    async fn canonical_mutation_advances_revision_without_allowing_identity_change() {
        let service = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));
        let saved = service
            .upsert_apparatus(ApparatusUpsert {
                id: None,
                name: "Mutable apparatus".to_string(),
                master: ApparatusMasterData::default(),
            })
            .await
            .expect("save apparatus");
        let id = ApparatusId::new(saved.id).expect("canonical id");

        let updated = service
            .mutate_canonical_apparatus(&id, 1, |canonical| {
                canonical.training.enabled = true;
                Ok(())
            })
            .await
            .expect("mutate canonical apparatus");
        assert_eq!(updated.identity.id, id);
        assert!(updated.training.enabled);
        assert_eq!(updated.versioning.revision, 2);

        let stale = service
            .mutate_canonical_apparatus(&id, 1, |canonical| {
                canonical.training.enabled = false;
                Ok(())
            })
            .await;
        assert_eq!(stale, Err(ApparatusGroupError::Conflict));

        let identity_change = service
            .mutate_canonical_apparatus(&id, 2, |canonical| {
                canonical.identity.id =
                    ApparatusId::new("apparatus:custom:other").expect("other canonical id");
                Ok(())
            })
            .await;
        assert_eq!(identity_change, Err(ApparatusGroupError::InvalidApparatus));
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
                factory_map_object_id: None,
                training_enabled: false,
                tooling_policy: None,
                capacity: Some(default_capacity_configuration()),
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
        assert_eq!(options.color_stations_min, 7);
        assert_eq!(options.color_stations_max, 9);
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
                    factory_map_object_id: None,
                    training_enabled: false,
                    tooling_policy: None,
                    capacity: Some(default_capacity_configuration()),
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
                    factory_map_object_id: None,
                    training_enabled: false,
                    tooling_policy: None,
                    capacity: Some(default_capacity_configuration()),
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
                    factory_map_object_id: None,
                    training_enabled: false,
                    tooling_policy: None,
                    capacity: Some(default_capacity_configuration()),
                },
            })
            .await;
        assert_eq!(
            invalid_color_stations,
            Err(ApparatusGroupError::InvalidColorStations)
        );
    }

    #[tokio::test]
    async fn default_apparatus_can_persist_factory_map_binding() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        let service = ApparatusGroupService::new(store);

        let saved = service
            .upsert_apparatus(ApparatusUpsert {
                id: Some("apparatus:default:bosma_7".to_string()),
                name: "7 ta rangli bosma aparat".to_string(),
                master: ApparatusMasterData {
                    factory_map_object_id: Some(" node:73 ".to_string()),
                    ..default_apparatus_master_data_for_id("apparatus:default:bosma_7")
                },
            })
            .await
            .expect("save default apparatus map binding");

        assert_eq!(saved.source, ApparatusSource::Default);
        assert_eq!(
            saved.master.factory_map_object_id.as_deref(),
            Some("node:73")
        );

        let catalog = service
            .apparatus_catalog("", 50)
            .await
            .expect("load apparatus catalog");
        let mapped = catalog
            .iter()
            .find(|item| item.id == "apparatus:default:bosma_7")
            .expect("mapped default apparatus");
        assert_eq!(mapped.source, ApparatusSource::Default);
        assert_eq!(
            mapped.master.factory_map_object_id.as_deref(),
            Some("node:73")
        );
    }

    #[tokio::test]
    async fn default_apparatus_can_persist_training_enabled() {
        let service = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));

        let saved = service
            .upsert_apparatus(ApparatusUpsert {
                id: Some("apparatus:default:bosma_7".to_string()),
                name: "7 ta rangli bosma aparat".to_string(),
                master: ApparatusMasterData {
                    training_enabled: true,
                    ..default_apparatus_master_data_for_id("apparatus:default:bosma_7")
                },
            })
            .await
            .expect("save training mode");

        assert!(saved.master.training_enabled);
        let catalog = service
            .apparatus_catalog("", 50)
            .await
            .expect("load apparatus training mode");
        assert!(
            catalog
                .iter()
                .find(|item| item.id == "apparatus:default:bosma_7")
                .expect("training apparatus")
                .master
                .training_enabled
        );
    }

    #[tokio::test]
    async fn default_apparatus_id_must_match_its_canonical_name() {
        let service = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));

        let result = service
            .upsert_apparatus(ApparatusUpsert {
                id: Some("apparatus:default:bosma_7".to_string()),
                name: "Laminatsiya 1".to_string(),
                master: ApparatusMasterData::default(),
            })
            .await;

        assert_eq!(result, Err(ApparatusGroupError::InvalidApparatus));
    }

    #[tokio::test]
    async fn factory_map_object_cannot_be_bound_to_two_apparatus() {
        let service = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));
        service
            .upsert_apparatus(ApparatusUpsert {
                id: None,
                name: "Birinchi liniya".to_string(),
                master: ApparatusMasterData {
                    factory_map_object_id: Some("node:12".to_string()),
                    ..ApparatusMasterData::default()
                },
            })
            .await
            .expect("save first map binding");

        let duplicate = service
            .upsert_apparatus(ApparatusUpsert {
                id: None,
                name: "Ikkinchi liniya".to_string(),
                master: ApparatusMasterData {
                    factory_map_object_id: Some("node:12".to_string()),
                    ..ApparatusMasterData::default()
                },
            })
            .await;

        assert_eq!(duplicate, Err(ApparatusGroupError::InvalidApparatus));
    }

    #[tokio::test]
    async fn custom_apparatus_id_and_group_membership_survive_rename() {
        let store = Arc::new(MemoryApparatusGroupStore::new());
        let service = ApparatusGroupService::new(store.clone());
        let created = service
            .upsert_apparatus(ApparatusUpsert {
                id: None,
                name: "Bobst 1".to_string(),
                master: ApparatusMasterData::default(),
            })
            .await
            .expect("create custom apparatus");
        assert!(created.id.starts_with("apparatus:custom:"));

        service
            .upsert_group(ApparatusGroupUpsert {
                name: "Custom group".to_string(),
                apparatus: vec![created.id.clone()],
            })
            .await
            .expect("save ID-based group");
        let renamed = service
            .upsert_apparatus(ApparatusUpsert {
                id: Some(created.id.clone()),
                name: "Bobst renamed".to_string(),
                master: ApparatusMasterData::default(),
            })
            .await
            .expect("rename custom apparatus");
        assert_eq!(renamed.id, created.id);
        assert_eq!(
            service.groups().await.expect("load group")[0].apparatus,
            vec![created.id]
        );
    }

    #[test]
    fn tooling_policy_serialization_distinguishes_legacy_missing_from_explicit_not_required() {
        let legacy: ApparatusMasterData = serde_json::from_str(
            r#"{"family":"laminatsiya","kind":"laminatsiya","tooling_policy":null}"#,
        )
        .expect("legacy master data");
        let explicit = ApparatusMasterData {
            tooling_policy: Some(ToolingPolicy::QolipScanNotRequired),
            ..legacy.clone()
        };

        assert_eq!(legacy.tooling_policy, None);
        assert_eq!(
            explicit.tooling_policy,
            Some(ToolingPolicy::QolipScanNotRequired)
        );
        assert!(
            !serde_json::to_string(&legacy)
                .expect("serialize legacy")
                .contains("tooling_policy")
        );
        assert!(
            serde_json::to_string(&explicit)
                .expect("serialize explicit")
                .contains("qolip_scan_not_required")
        );
    }

    #[test]
    fn tooling_policy_derives_from_typed_classification_not_title() {
        let mut pechat = default_apparatus_master_data_for_id("apparatus:default:bosma_7");
        pechat.tooling_policy = None;
        let renamed_id = ApparatusId::new("apparatus:custom:renamed").expect("renamed id");
        assert_eq!(
            tooling_policy_for_master(&renamed_id, &pechat),
            Ok(ToolingPolicy::QolipScanRequired)
        );

        let non_pechat = default_apparatus_master_data_for_id("apparatus:default:asset-010");
        let title_like_id = ApparatusId::new("apparatus:custom:qolip-pechat").expect("custom id");
        assert_eq!(
            tooling_policy_for_master(&title_like_id, &non_pechat),
            Ok(ToolingPolicy::QolipScanNotRequired)
        );

        let unknown = ApparatusMasterData {
            family: "unknown".to_string(),
            kind: "unknown".to_string(),
            ..ApparatusMasterData::default()
        };
        let unknown_id = ApparatusId::new("apparatus:custom:unknown").expect("custom id");
        assert_eq!(
            tooling_policy_for_master(&unknown_id, &unknown),
            Err(ApparatusGroupError::InvalidKind)
        );
    }

    #[tokio::test]
    async fn custom_tooling_policy_round_trips_through_upsert_and_catalog() {
        let service = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));
        let master = ApparatusMasterData {
            family: "rezka".to_string(),
            kind: "rezka".to_string(),
            capabilities: vec!["cut".to_string()],
            tooling_policy: Some(ToolingPolicy::QolipScanNotRequired),
            ..ApparatusMasterData::default()
        };

        let saved = service
            .upsert_apparatus(ApparatusUpsert {
                id: None,
                name: "Custom cutter".to_string(),
                master: master.clone(),
            })
            .await
            .expect("save custom apparatus");
        let loaded = service
            .apparatus_catalog("Custom cutter", 1)
            .await
            .expect("load custom apparatus")
            .pop()
            .expect("catalog entry");

        assert_eq!(loaded.id, saved.id);
        assert_eq!(loaded.master.tooling_policy, master.tooling_policy);
    }
}
