use std::sync::Arc;

use crate::core::apparatus_standard::isa95::tests::revision_with;
use crate::core::apparatus_standard::{
    ApparatusId, CanonicalApparatusService, CanonicalCommandMetadata,
};
use crate::core::production_map::{
    CanonicalApparatusResolver, CanonicalServiceApparatusResolver, MemoryProductionMapStore,
    ProductionMapError, ProductionMapService,
};

#[tokio::test]
async fn resolver_reads_exact_runtime_projection_from_canonical_service() {
    let service = CanonicalApparatusService::memory();
    let committed = service
        .create(
            revision_with(
                "apparatus:test:resolver-template",
                "physical-asset:resolver-001",
                "Resolver display snapshot",
            )
            .to_draft(),
            CanonicalCommandMetadata::new("user:test", "command:resolver-create"),
        )
        .await
        .expect("create canonical fixture");
    let resolver = CanonicalServiceApparatusResolver::new(service);

    let configuration = resolver
        .resolve(&committed.revision.apparatus_id)
        .await
        .expect("canonical lookup")
        .expect("runtime configuration");

    assert!(configuration.has_coherent_source());
    assert_eq!(
        configuration.runtime.apparatus_id,
        committed.revision.apparatus_id
    );
    assert_eq!(
        configuration.runtime.display.display_name,
        "Resolver display snapshot"
    );
    assert_eq!(
        configuration.runtime.source_aasx_sha256,
        committed.aasx_sha256
    );
}

#[tokio::test]
async fn resolver_never_promotes_display_text_to_identity() {
    let resolver = CanonicalServiceApparatusResolver::new(CanonicalApparatusService::memory());
    let unknown = ApparatusId::new("apparatus:test:unknown").unwrap();

    assert!(resolver.resolve(&unknown).await.unwrap().is_none());
}

#[tokio::test]
async fn production_map_fails_closed_when_required_projection_is_missing() {
    let service = ProductionMapService::new(
        Arc::new(MemoryProductionMapStore::new()),
        Arc::new(CanonicalServiceApparatusResolver::new(
            CanonicalApparatusService::memory(),
        )),
    );
    let unknown = ApparatusId::new("apparatus:test:missing").unwrap();

    assert_eq!(
        service.resolve_canonical_apparatus(&unknown).await,
        Err(ProductionMapError::StoreFailed)
    );
}
