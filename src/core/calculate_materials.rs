use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CalculateMaterialVariant {
    pub micron: u32,
    pub coefficient: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_layer_coefficient: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CalculateMaterial {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default = "default_active")]
    pub active: bool,
    pub variants: Vec<CalculateMaterialVariant>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CalculateMaterialUpsert {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default = "default_active")]
    pub active: bool,
    #[serde(default)]
    pub variants: Vec<CalculateMaterialVariant>,
}

impl Default for CalculateMaterialUpsert {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            aliases: Vec::new(),
            active: true,
            variants: Vec::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum CalculateMaterialError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("store failed")]
    StoreFailed,
}

#[async_trait]
pub trait CalculateMaterialStorePort: Send + Sync {
    async fn list(&self) -> Result<Vec<CalculateMaterial>, CalculateMaterialError>;
    async fn upsert(
        &self,
        input: CalculateMaterialUpsert,
    ) -> Result<CalculateMaterial, CalculateMaterialError>;
}

#[derive(Clone)]
pub struct MemoryCalculateMaterialStore {
    materials: Arc<RwLock<Vec<CalculateMaterial>>>,
}

impl MemoryCalculateMaterialStore {
    pub fn new() -> Self {
        Self {
            materials: Arc::new(RwLock::new(default_calculate_materials())),
        }
    }
}

#[async_trait]
impl CalculateMaterialStorePort for MemoryCalculateMaterialStore {
    async fn list(&self) -> Result<Vec<CalculateMaterial>, CalculateMaterialError> {
        self.materials
            .read()
            .map(|materials| materials.clone())
            .map_err(|_| CalculateMaterialError::StoreFailed)
    }

    async fn upsert(
        &self,
        input: CalculateMaterialUpsert,
    ) -> Result<CalculateMaterial, CalculateMaterialError> {
        let material = normalize_material(input)?;
        let mut materials = self
            .materials
            .write()
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        ensure_unique_name(&materials, &material)?;
        if let Some(current) = materials
            .iter_mut()
            .find(|current| current.id == material.id)
        {
            *current = material.clone();
        } else {
            materials.push(material.clone());
        }
        materials.sort_by_key(|item| normalize_key(&item.name));
        Ok(material)
    }
}

pub fn normalize_material(
    input: CalculateMaterialUpsert,
) -> Result<CalculateMaterial, CalculateMaterialError> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(CalculateMaterialError::InvalidInput(
            "material nomi kerak".to_string(),
        ));
    }
    if name.chars().count() > 120 {
        return Err(CalculateMaterialError::InvalidInput(
            "material nomi juda uzun".to_string(),
        ));
    }

    let mut variants = input.variants;
    if variants.is_empty() {
        return Err(CalculateMaterialError::InvalidInput(
            "kamida bitta mikron va koeffisent kerak".to_string(),
        ));
    }
    for variant in &variants {
        if variant.micron == 0 {
            return Err(CalculateMaterialError::InvalidInput(
                "mikron musbat bo'lishi kerak".to_string(),
            ));
        }
        if !variant.coefficient.is_finite() || variant.coefficient <= 0.0 {
            return Err(CalculateMaterialError::InvalidInput(
                "koeffisent musbat son bo'lishi kerak".to_string(),
            ));
        }
        if variant
            .first_layer_coefficient
            .is_some_and(|coefficient| !coefficient.is_finite() || coefficient <= 0.0)
        {
            return Err(CalculateMaterialError::InvalidInput(
                "birinchi qavat koeffisenti musbat son bo'lishi kerak".to_string(),
            ));
        }
    }
    variants.sort_by_key(|variant| variant.micron);
    if variants
        .windows(2)
        .any(|window| window[0].micron == window[1].micron)
    {
        return Err(CalculateMaterialError::InvalidInput(
            "mikronlar takrorlanmasligi kerak".to_string(),
        ));
    }

    let mut aliases = Vec::new();
    for alias in input.aliases {
        let alias = alias.trim();
        if alias.is_empty() || normalize_key(alias) == normalize_key(&name) {
            continue;
        }
        if !aliases
            .iter()
            .any(|current: &String| normalize_key(current) == normalize_key(alias))
        {
            aliases.push(alias.to_string());
        }
    }

    Ok(CalculateMaterial {
        id: if input.id.trim().is_empty() {
            format!("material-{}", unix_micros())
        } else {
            input.id.trim().to_string()
        },
        name,
        aliases,
        active: input.active,
        variants,
    })
}

pub fn normalize_key(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

pub fn ensure_unique_name(
    materials: &[CalculateMaterial],
    incoming: &CalculateMaterial,
) -> Result<(), CalculateMaterialError> {
    let incoming_key = normalize_key(&incoming.name);
    if materials.iter().any(|current| {
        current.id != incoming.id && normalize_key(&current.name) == incoming_key
    }) {
        return Err(CalculateMaterialError::InvalidInput(
            "bu nomdagi material allaqachon mavjud".to_string(),
        ));
    }
    Ok(())
}

pub fn default_calculate_materials() -> Vec<CalculateMaterial> {
    vec![
        builtin("builtin-pet", "PET", &["pet"], mcp_cpp_variants()),
        builtin("builtin-opp", "OPP", &["opp", "bopp"], mcp_cpp_variants()),
        builtin(
            "builtin-bopp-metal",
            "BOPP metal",
            &["bopp metall", "boppmetal"],
            mcp_cpp_variants(),
        ),
        builtin("builtin-mcp", "MCP", &["mcp", "mcpp"], mcp_cpp_variants()),
        builtin("builtin-cpp", "CPP", &["cpp"], mcp_cpp_variants()),
        builtin("builtin-pe", "PE", &["pe", "pe oq", "pe pr"], pe_variants()),
        builtin("builtin-jem", "JEM", &["jem"], jem_variants()),
    ]
}

pub fn merge_default_calculate_materials(
    overrides: Vec<CalculateMaterial>,
) -> Vec<CalculateMaterial> {
    let mut materials = default_calculate_materials();
    for override_material in overrides {
        if let Some(current) = materials
            .iter_mut()
            .find(|current| current.id == override_material.id)
        {
            *current = override_material;
        } else {
            materials.push(override_material);
        }
    }
    materials.sort_by_key(|item| normalize_key(&item.name));
    materials
}

fn builtin(
    id: &str,
    name: &str,
    aliases: &[&str],
    variants: Vec<CalculateMaterialVariant>,
) -> CalculateMaterial {
    CalculateMaterial {
        id: id.to_string(),
        name: name.to_string(),
        aliases: aliases.iter().map(|value| (*value).to_string()).collect(),
        active: true,
        variants,
    }
}

fn mcp_cpp_variants() -> Vec<CalculateMaterialVariant> {
    [
        (20, 1.07),
        (25, 1.3),
        (30, 1.6),
        (35, 2.0),
        (40, 2.15),
        (45, 2.7),
        (50, 2.8),
        (60, 3.2),
    ]
    .into_iter()
    .map(|(micron, coefficient)| CalculateMaterialVariant {
        micron,
        coefficient,
        first_layer_coefficient: (micron <= 20).then_some(1.0),
    })
    .collect()
}

fn jem_variants() -> Vec<CalculateMaterialVariant> {
    [(25, 1.0), (30, 1.5)]
        .into_iter()
        .map(|(micron, coefficient)| CalculateMaterialVariant {
            micron,
            coefficient,
            first_layer_coefficient: None,
        })
        .collect()
}

fn pe_variants() -> Vec<CalculateMaterialVariant> {
    [
        (30, 2.0),
        (35, 2.3),
        (40, 2.6),
        (45, 3.0),
        (50, 3.3),
        (55, 3.6),
        (60, 4.0),
        (65, 4.3),
        (70, 4.6),
        (75, 5.0),
        (80, 5.3),
        (85, 5.6),
        (90, 6.0),
    ]
    .into_iter()
    .map(|(micron, coefficient)| CalculateMaterialVariant {
        micron,
        coefficient,
        first_layer_coefficient: None,
    })
    .collect()
}

fn default_active() -> bool {
    true
}

fn unix_micros() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_sorts_material_variants() {
        let material = normalize_material(CalculateMaterialUpsert {
            name: " BOPP metal ".to_string(),
            aliases: vec!["bopp metall".to_string(), "BOPP METALL".to_string()],
            variants: vec![
                CalculateMaterialVariant {
                    micron: 30,
                    coefficient: 1.6,
                    first_layer_coefficient: None,
                },
                CalculateMaterialVariant {
                    micron: 20,
                    coefficient: 1.1,
                    first_layer_coefficient: None,
                },
            ],
            ..CalculateMaterialUpsert::default()
        })
        .expect("valid material");

        assert_eq!(material.name, "BOPP metal");
        assert_eq!(material.variants[0].micron, 20);
        assert_eq!(material.aliases, vec!["bopp metall"]);
    }

    #[tokio::test]
    async fn memory_store_starts_with_legacy_materials() {
        let store = MemoryCalculateMaterialStore::new();
        let materials = store.list().await.expect("materials");
        assert!(materials.iter().any(|material| material.name == "BOPP metal"));
    }
}
