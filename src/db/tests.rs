mod admin_item;
mod apparatus_identity;
mod calculate_order;
mod customer;
mod gscale_receipt;
mod inventory_movements;
mod mini_order;
mod opening_wip;
mod production_map;
mod training_workspace;
mod warehouse;
mod worker;
mod worker_group;

use std::sync::Arc;

use sqlx::PgPool;

use crate::core::apparatus_standard::ApparatusId;
use crate::core::apparatus_standard::service::CanonicalApparatusService;
use crate::core::apparatus_standard::test_support::{
    TestApparatusSpec, canonical_draft, standard_revisions,
};
use crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository;

async fn seed_canonical_apparatus(pool: &PgPool, spec: TestApparatusSpec<'_>) {
    let apparatus_id = ApparatusId::new(spec.apparatus_id).expect("canonical test apparatus id");
    CanonicalApparatusService::new(Arc::new(PostgresCanonicalApparatusRepository::new(
        pool.clone(),
    )))
    .seed_for_test(apparatus_id, canonical_draft(&spec))
    .await
    .expect("seed canonical test apparatus");
}

async fn seed_standard_canonical_apparatus(pool: &PgPool) {
    let service = CanonicalApparatusService::new(Arc::new(
        PostgresCanonicalApparatusRepository::new(pool.clone()),
    ));
    for revision in standard_revisions() {
        service
            .seed_for_test(revision.apparatus_id.clone(), revision.to_draft())
            .await
            .expect("seed standard canonical test apparatus");
    }
}
