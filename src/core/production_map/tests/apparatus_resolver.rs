use std::sync::Arc;

use crate::core::apparatus_groups::{
    ApparatusGroupError, ApparatusGroupService, ApparatusMasterData, ApparatusUpsert,
    MemoryApparatusGroupStore,
};
use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{ApparatusGroupCanonicalResolver, CanonicalApparatusResolver};

const CANONICAL_ID: &str = "apparatus:test:asset-401";
const DISPLAY_NAME: &str = "Laminatsiya resolver test";

async fn seeded_resolver() -> ApparatusGroupCanonicalResolver {
    let catalog = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));
    catalog
        .upsert_apparatus(ApparatusUpsert {
            id: Some(CANONICAL_ID.to_string()),
            name: DISPLAY_NAME.to_string(),
            master: ApparatusMasterData::default(),
        })
        .await
        .expect("seed canonical apparatus");
    ApparatusGroupCanonicalResolver::new(catalog)
}

#[tokio::test]
async fn resolver_returns_canonical_config_by_exact_id_and_preserves_display_snapshot() {
    let resolver = seeded_resolver().await;
    let id = ApparatusId::new(CANONICAL_ID).expect("canonical id");

    let canonical = resolver
        .resolve(&id)
        .await
        .expect("canonical lookup")
        .expect("canonical configuration");

    assert_eq!(canonical.identity.id, id);
    assert_eq!(canonical.identity.display.display_name, DISPLAY_NAME);
    assert_eq!(
        canonical.aas.submodel_id,
        "urn:mini-rs-erp:submodel:apparatus:test:asset-401"
    );
}

#[tokio::test]
async fn resolver_fails_closed_when_only_legacy_catalog_projection_is_present() {
    let store = Arc::new(MemoryApparatusGroupStore::new());
    store
        .put_apparatus_with_id(
            Some(CANONICAL_ID),
            DISPLAY_NAME,
            &ApparatusMasterData::default(),
        )
        .await
        .expect("seed legacy projection");
    let catalog = ApparatusGroupService::new(store);
    let resolver = ApparatusGroupCanonicalResolver::new(catalog);
    let id = ApparatusId::new(CANONICAL_ID).expect("canonical id");

    assert_eq!(resolver.resolve(&id).await.expect("canonical lookup"), None);
}

#[tokio::test]
async fn canonical_payload_stays_authoritative_after_a_direct_canonical_mutation() {
    let catalog = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));
    catalog
        .upsert_apparatus(ApparatusUpsert {
            id: Some(CANONICAL_ID.to_string()),
            name: DISPLAY_NAME.to_string(),
            master: ApparatusMasterData::default(),
        })
        .await
        .expect("seed canonical apparatus");

    let id = ApparatusId::new(CANONICAL_ID).expect("canonical id");
    let current = catalog
        .canonical_apparatus_by_id(&id)
        .await
        .expect("canonical lookup")
        .expect("canonical configuration");
    catalog
        .mutate_canonical_apparatus(&id, current.versioning.revision, |canonical| {
            canonical.identity.display.display_name = "Canonical rename".to_string();
            Ok(())
        })
        .await
        .expect("direct canonical mutation");

    let resolver = ApparatusGroupCanonicalResolver::new(catalog.clone());
    assert_eq!(
        resolver
            .resolve(&id)
            .await
            .expect("canonical lookup")
            .expect("canonical configuration")
            .identity
            .display
            .display_name,
        "Canonical rename"
    );
    assert_eq!(
        catalog
            .apparatus_catalog("", 10)
            .await
            .expect("catalog")
            .into_iter()
            .find(|entry| entry.id == CANONICAL_ID)
            .expect("catalog entry")
            .name,
        "Canonical rename"
    );
}

#[tokio::test]
async fn catalog_rejects_title_derived_id_instead_of_accepting_display_as_identity() {
    let catalog = ApparatusGroupService::new(Arc::new(MemoryApparatusGroupStore::new()));
    let result = catalog
        .upsert_apparatus(ApparatusUpsert {
            id: Some("apparatus:test:laminatsiya-1".to_string()),
            name: "Laminatsiya 1".to_string(),
            master: ApparatusMasterData::default(),
        })
        .await;

    assert_eq!(result, Err(ApparatusGroupError::InvalidApparatus));
}

#[tokio::test]
async fn resolver_does_not_promote_a_matching_display_title_to_an_unknown_id() {
    let resolver = seeded_resolver().await;
    let title_derived_id = ApparatusId::new("apparatus:test:laminatsiya-1")
        .expect("shape-valid but title-derived test id");

    assert_eq!(
        resolver
            .resolve(&title_derived_id)
            .await
            .expect("canonical lookup"),
        None
    );
}
