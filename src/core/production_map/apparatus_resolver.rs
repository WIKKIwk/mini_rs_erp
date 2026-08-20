use std::sync::Arc;

use async_trait::async_trait;

use crate::core::apparatus_groups::{ApparatusGroupError, ApparatusGroupService};
use crate::core::apparatus_standard::{ApparatusId, CanonicalApparatus};

use super::errors::ProductionMapError;

/// The only runtime lookup contract for apparatus configuration.
///
/// Callers must already hold an [`ApparatusId`]. This interface intentionally
/// has no name/title argument, so display snapshots cannot become identity or
/// configuration fallbacks.
#[async_trait]
pub trait CanonicalApparatusResolver: Send + Sync {
    async fn resolve(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<Arc<CanonicalApparatus>>, ProductionMapError>;
}

#[derive(Clone, Default)]
pub struct UnavailableCanonicalApparatusResolver;

#[async_trait]
impl CanonicalApparatusResolver for UnavailableCanonicalApparatusResolver {
    async fn resolve(
        &self,
        _apparatus_id: &ApparatusId,
    ) -> Result<Option<Arc<CanonicalApparatus>>, ProductionMapError> {
        Ok(None)
    }
}

/// Adapter from the canonical catalog service to the production-map runtime.
/// The catalog service remains the master-data owner; this adapter performs an
/// exact ID lookup and rejects malformed or conflicting canonical data.
#[derive(Clone)]
pub struct ApparatusGroupCanonicalResolver {
    catalog: ApparatusGroupService,
}

impl ApparatusGroupCanonicalResolver {
    pub fn new(catalog: ApparatusGroupService) -> Self {
        Self { catalog }
    }
}

#[async_trait]
impl CanonicalApparatusResolver for ApparatusGroupCanonicalResolver {
    async fn resolve(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<Arc<CanonicalApparatus>>, ProductionMapError> {
        let Some(canonical) = self
            .catalog
            .canonical_apparatus_by_id(apparatus_id)
            .await
            .map_err(map_catalog_error)?
        else {
            return Ok(None);
        };
        Ok(Some(Arc::new(canonical)))
    }
}

fn map_catalog_error(_error: ApparatusGroupError) -> ProductionMapError {
    ProductionMapError::StoreFailed
}
