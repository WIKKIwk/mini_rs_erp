
pub const DEFAULT_PET_DENSITY_G_CM3: f64 = 1.400;
pub const DEFAULT_PP_FILM_DENSITY_G_CM3: f64 = 0.905;
pub const DEFAULT_PE_DENSITY_G_CM3: f64 = 0.920;
const RETIRED_BUILTIN_MATERIAL_IDS: &[&str] = &["builtin-pe-qora"];

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CalculateMaterialVariant {
    pub micron: u32,
    #[serde(default)]
    pub coefficient: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_layer_coefficient: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_gsm: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CalculateMaterial {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default = "default_active")]
    pub active: bool,
    #[serde(default)]
    pub density_g_cm3: f64,
    pub variants: Vec<CalculateMaterialVariant>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculateMaterialUpsert {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_active")]
    pub active: bool,
    #[serde(default)]
    pub density_g_cm3: f64,
    #[serde(default)]
    pub variants: Vec<CalculateMaterialVariant>,
}

impl Default for CalculateMaterialUpsert {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            active: true,
            density_g_cm3: 0.0,
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

impl Default for MemoryCalculateMaterialStore {
    fn default() -> Self {
        Self::new()
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

    let density_g_cm3 = input.density_g_cm3;
    if !density_g_cm3.is_finite() || density_g_cm3 < 0.0 {
        return Err(CalculateMaterialError::InvalidInput(
            "zichlik noto'g'ri".to_string(),
        ));
    }

    let mut variants = input.variants;
    if variants.is_empty() && density_g_cm3 <= 0.0 {
        return Err(CalculateMaterialError::InvalidInput(
            "zichlik yoki kamida bitta mikron kerak".to_string(),
        ));
    }
    for variant in &mut variants {
        if variant.micron == 0 {
            return Err(CalculateMaterialError::InvalidInput(
                "mikron musbat bo'lishi kerak".to_string(),
            ));
        }
        if variant
            .actual_gsm
            .is_some_and(|gsm| !gsm.is_finite() || gsm <= 0.0)
        {
            return Err(CalculateMaterialError::InvalidInput(
                "actual GSM musbat son bo'lishi kerak".to_string(),
            ));
        }
        let gsm = effective_variant_gsm(density_g_cm3, variant).ok_or_else(|| {
            CalculateMaterialError::InvalidInput(
                "zichlik yoki har bir mikron uchun actual GSM kerak".to_string(),
            )
        })?;
        if !gsm.is_finite() || gsm <= 0.0 {
            return Err(CalculateMaterialError::InvalidInput(
                "hisoblangan GSM noto'g'ri".to_string(),
            ));
        }
        variant.coefficient = gsm_to_legacy_coefficient(gsm);
        variant.first_layer_coefficient = None;
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

    Ok(CalculateMaterial {
        id: if input.id.trim().is_empty() {
            format!("material-{}", unix_micros())
        } else {
            input.id.trim().to_string()
        },
        name,
        active: input.active,
        density_g_cm3,
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
    if materials
        .iter()
        .any(|current| current.id != incoming.id && normalize_key(&current.name) == incoming_key)
    {
        return Err(CalculateMaterialError::InvalidInput(
            "bu nomdagi material allaqachon mavjud".to_string(),
        ));
    }
    Ok(())
}

pub fn default_calculate_materials() -> Vec<CalculateMaterial> {
    vec![
        builtin(
            "builtin-pet",
            "PET",
            DEFAULT_PET_DENSITY_G_CM3,
            density_variants(
                &[12, 20, 25, 30, 35, 40, 45, 50, 60],
                DEFAULT_PET_DENSITY_G_CM3,
            ),
        ),
        builtin(
            "builtin-opp",
            "OPP",
            DEFAULT_PP_FILM_DENSITY_G_CM3,
            density_variants(
                &[18, 20, 25, 30, 35, 40, 45, 50, 60],
                DEFAULT_PP_FILM_DENSITY_G_CM3,
            ),
        ),
        builtin(
            "builtin-bopp",
            "BOPP",
            DEFAULT_PP_FILM_DENSITY_G_CM3,
            density_variants(
                &[18, 20, 25, 30, 35, 40, 45, 50, 60],
                DEFAULT_PP_FILM_DENSITY_G_CM3,
            ),
        ),
        builtin(
            "builtin-bopp-metal",
            "BOPP metal",
            DEFAULT_PP_FILM_DENSITY_G_CM3,
            density_variants(
                &[18, 20, 25, 30, 35, 40, 45, 50, 60],
                DEFAULT_PP_FILM_DENSITY_G_CM3,
            ),
        ),
        builtin(
            "builtin-mcp",
            "MCP",
            DEFAULT_PP_FILM_DENSITY_G_CM3,
            density_variants(
                &[20, 25, 30, 35, 40, 45, 50, 60],
                DEFAULT_PP_FILM_DENSITY_G_CM3,
            ),
        ),
        builtin(
            "builtin-mcpp",
            "MCPP",
            DEFAULT_PP_FILM_DENSITY_G_CM3,
            density_variants(
                &[20, 25, 30, 35, 40, 45, 50, 60],
                DEFAULT_PP_FILM_DENSITY_G_CM3,
            ),
        ),
        builtin(
            "builtin-cpp",
            "CPP",
            DEFAULT_PP_FILM_DENSITY_G_CM3,
            density_variants(
                &[20, 25, 30, 35, 40, 45, 50, 60],
                DEFAULT_PP_FILM_DENSITY_G_CM3,
            ),
        ),
        builtin(
            "builtin-pe",
            "PE",
            DEFAULT_PE_DENSITY_G_CM3,
            density_variants(
                &[30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90],
                DEFAULT_PE_DENSITY_G_CM3,
            ),
        ),
        builtin(
            "builtin-pe-oq",
            "PE oq",
            DEFAULT_PE_DENSITY_G_CM3,
            density_variants(
                &[30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90],
                DEFAULT_PE_DENSITY_G_CM3,
            ),
        ),
        builtin(
            "builtin-pe-pr",
            "PE PR",
            DEFAULT_PE_DENSITY_G_CM3,
            density_variants(
                &[30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90],
                DEFAULT_PE_DENSITY_G_CM3,
            ),
        ),
        builtin("builtin-jem", "JEM", 0.0, legacy_jem_variants()),
    ]
}

pub fn merge_default_calculate_materials(
    overrides: Vec<CalculateMaterial>,
) -> Vec<CalculateMaterial> {
    let mut materials = default_calculate_materials();
    for override_material in overrides {
        if RETIRED_BUILTIN_MATERIAL_IDS.contains(&override_material.id.as_str()) {
            continue;
        }
        let override_material = upgrade_stored_material(override_material, &materials);
        let override_name = normalize_key(&override_material.name);
        if let Some(current) = materials.iter_mut().find(|current| {
            current.id == override_material.id || normalize_key(&current.name) == override_name
        }) {
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
    density_g_cm3: f64,
    variants: Vec<CalculateMaterialVariant>,
) -> CalculateMaterial {
    CalculateMaterial {
        id: id.to_string(),
        name: name.to_string(),
        active: true,
        density_g_cm3,
        variants,
    }
}

fn legacy_jem_variants() -> Vec<CalculateMaterialVariant> {
    [(25, 1.0), (30, 1.5)]
        .into_iter()
        .map(|(micron, coefficient)| CalculateMaterialVariant {
            micron,
            coefficient,
            first_layer_coefficient: None,
            actual_gsm: Some(legacy_coefficient_to_gsm(coefficient)),
        })
        .collect()
}

fn density_variants(microns: &[u32], density_g_cm3: f64) -> Vec<CalculateMaterialVariant> {
    microns
        .iter()
        .copied()
        .map(|micron| {
            let gsm = f64::from(micron) * density_g_cm3;
            CalculateMaterialVariant {
                micron,
                coefficient: gsm_to_legacy_coefficient(gsm),
                first_layer_coefficient: None,
                actual_gsm: None,
            }
        })
        .collect()
}

fn upgrade_stored_material(
    mut material: CalculateMaterial,
    defaults: &[CalculateMaterial],
) -> CalculateMaterial {
    if material.density_g_cm3 <= 0.0
        && let Some(default) = defaults.iter().find(|default| default.id == material.id)
    {
        material.density_g_cm3 = default.density_g_cm3;
    }
    for variant in &mut material.variants {
        if material.density_g_cm3 <= 0.0
            && variant.actual_gsm.is_none()
            && variant.coefficient.is_finite()
            && variant.coefficient > 0.0
        {
            variant.actual_gsm = Some(legacy_coefficient_to_gsm(variant.coefficient));
        }
        if let Some(gsm) = effective_variant_gsm(material.density_g_cm3, variant) {
            variant.coefficient = gsm_to_legacy_coefficient(gsm);
            variant.first_layer_coefficient = None;
        }
    }
    material
}

pub fn effective_variant_gsm(
    density_g_cm3: f64,
    variant: &CalculateMaterialVariant,
) -> Option<f64> {
    variant.actual_gsm.or_else(|| {
        (density_g_cm3.is_finite() && density_g_cm3 > 0.0)
            .then(|| f64::from(variant.micron) * density_g_cm3)
            .or_else(|| {
                (variant.coefficient.is_finite() && variant.coefficient > 0.0)
                    .then(|| legacy_coefficient_to_gsm(variant.coefficient))
            })
    })
}

pub fn legacy_coefficient_to_gsm(coefficient: f64) -> f64 {
    coefficient * (1_000_000.0 / 60_000.0)
}

pub fn gsm_to_legacy_coefficient(gsm: f64) -> f64 {
    gsm * (60_000.0 / 1_000_000.0)
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
