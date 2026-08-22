
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactoryLocationApparatus {
    pub id: ApparatusId,
    pub name: String,
    pub source_revision: u64,
    pub equipment_class_id: EquipmentClassId,
    pub physical_asset_id: PhysicalAssetId,
    pub active: bool,
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
    apparatus: CanonicalApparatusService,
}

impl FactoryLocationService {
    pub fn new(
        store: Arc<dyn FactoryLocationStorePort>,
        apparatus: CanonicalApparatusService,
    ) -> Self {
        Self { store, apparatus }
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
        for location in locations {
            for apparatus in &mut location.apparatus {
                let projection = self
                    .apparatus
                    .current_projection(&apparatus.id)
                    .await
                    .map_err(|_| FactoryLocationError::StoreFailed)?
                    .ok_or(FactoryLocationError::InvalidApparatus)?;
                *apparatus = factory_location_apparatus(&projection);
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
        let mut selected = Vec::new();
        for apparatus_id in &requested {
            let projection = self
                .apparatus
                .current_projection(apparatus_id)
                .await
                .map_err(|_| FactoryLocationError::StoreFailed)?
                .ok_or(FactoryLocationError::InvalidApparatus)?;
            if projection.lifecycle.state != LifecycleState::Active {
                return Err(FactoryLocationError::InvalidApparatus);
            }
            selected.push(factory_location_apparatus(&projection));
        }
        selected.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(selected)
    }
}

fn factory_location_apparatus(projection: &RuntimeApparatusProjection) -> FactoryLocationApparatus {
    FactoryLocationApparatus {
        id: projection.apparatus_id.clone(),
        name: projection.display.display_name.clone(),
        source_revision: projection.source_revision,
        equipment_class_id: projection.equipment_class_id.clone(),
        physical_asset_id: projection.physical_asset_id.clone(),
        active: projection.lifecycle.state == LifecycleState::Active,
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
