use std::sync::Arc;

use async_trait::async_trait;

use crate::core::apparatus_standard::{
    ApparatusId, CanonicalApparatusService, RuntimeApparatusConfiguration,
};

use super::errors::ProductionMapError;

#[cfg(test)]
use std::collections::BTreeMap;

/// Required runtime lookup by immutable canonical identity. Implementations
/// return PostgreSQL projections and have no AASX or legacy-catalog path.
#[async_trait]
pub trait CanonicalApparatusResolver: Send + Sync {
    async fn resolve(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<Arc<RuntimeApparatusConfiguration>>, ProductionMapError>;

    async fn list(
        &self,
    ) -> Result<Vec<Arc<RuntimeApparatusConfiguration>>, ProductionMapError>;
}

#[derive(Clone)]
pub struct CanonicalServiceApparatusResolver {
    service: CanonicalApparatusService,
}

impl CanonicalServiceApparatusResolver {
    pub fn new(service: CanonicalApparatusService) -> Self {
        Self { service }
    }
}

#[async_trait]
impl CanonicalApparatusResolver for CanonicalServiceApparatusResolver {
    async fn resolve(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        let configuration = self
            .service
            .current_configuration(apparatus_id)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        if configuration
            .as_deref()
            .is_some_and(|value| !value.has_coherent_source())
        {
            return Err(ProductionMapError::StoreFailed);
        }
        Ok(configuration)
    }

    async fn list(
        &self,
    ) -> Result<Vec<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        self.service
            .list_runtime_configurations()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?
            .into_iter()
            .map(|configuration| {
                if configuration.has_coherent_source() {
                    Ok(Arc::new(configuration))
                } else {
                    Err(ProductionMapError::StoreFailed)
                }
            })
            .collect()
    }
}

#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct TestCanonicalApparatusResolver {
    configurations: Arc<BTreeMap<ApparatusId, Arc<RuntimeApparatusConfiguration>>>,
}

#[cfg(test)]
impl TestCanonicalApparatusResolver {
    pub(crate) fn new(
        configurations: impl IntoIterator<Item = RuntimeApparatusConfiguration>,
    ) -> Self {
        Self {
            configurations: Arc::new(
                configurations
                    .into_iter()
                    .map(|configuration| {
                        (
                            configuration.runtime.apparatus_id.clone(),
                            Arc::new(configuration),
                        )
                    })
                    .collect(),
            ),
        }
    }

    pub(crate) fn standard() -> Self {
        Self::new(crate::core::apparatus_standard::test_support::standard_runtime_configurations())
    }
}

#[cfg(test)]
#[async_trait]
impl CanonicalApparatusResolver for TestCanonicalApparatusResolver {
    async fn resolve(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        Ok(self.configurations.get(apparatus_id).cloned())
    }

    async fn list(
        &self,
    ) -> Result<Vec<Arc<RuntimeApparatusConfiguration>>, ProductionMapError> {
        Ok(self.configurations.values().cloned().collect())
    }
}
