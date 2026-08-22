use std::sync::atomic::{AtomicU64, Ordering};

use sqlx::postgres::PgConnectOptions;
use sqlx::{PgPool, Row};

use crate::core::apparatus_standard::isa95::tests::revision_with;
use crate::core::apparatus_standard::{
    ApparatusId, CanonicalApparatusDraft, CanonicalApparatusService, CanonicalCommandMetadata,
};
use crate::db::postgres::{apply_foundation_migration, apply_postgres_migrations_through};

use super::super::PostgresCanonicalApparatusRepository;

static DATABASE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub(super) struct TestDatabase {
    pub admin_url: String,
    pub name: String,
    pub pool: PgPool,
}

impl TestDatabase {
    pub async fn create(label: &str) -> Self {
        Self::create_through(label, 72).await
    }

    pub async fn create_through(label: &str, migration_count: usize) -> Self {
        let sequence = DATABASE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "mini_rs_erp_test_canonical_{}_{}_{}",
            label,
            std::process::id(),
            sequence
        );
        assert!(
            name.len() < 64,
            "PostgreSQL database name must fit NAMEDATALEN"
        );
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
        let admin_pool = PgPool::connect(&admin_url)
            .await
            .expect("connect to PostgreSQL test admin database");
        sqlx::query(&format!(r#"DROP DATABASE IF EXISTS "{name}" WITH (FORCE)"#))
            .execute(&admin_pool)
            .await
            .expect("drop stale canonical test database");
        sqlx::query(&format!(r#"CREATE DATABASE "{name}""#))
            .execute(&admin_pool)
            .await
            .expect("create canonical test database");
        admin_pool.close().await;

        let options = admin_url
            .parse::<PgConnectOptions>()
            .expect("valid PostgreSQL admin URL")
            .database(&name);
        let pool = PgPool::connect_with(options)
            .await
            .expect("connect to canonical test database");
        apply_postgres_migrations_through(&pool, migration_count)
            .await
            .expect("apply requested PostgreSQL migration prefix");
        Self {
            admin_url,
            name,
            pool,
        }
    }

    pub async fn migrate_current(&self) {
        apply_foundation_migration(&self.pool)
            .await
            .expect("apply current PostgreSQL migration registry");
    }

    pub async fn migrate_through(&self, migration_count: usize) {
        apply_postgres_migrations_through(&self.pool, migration_count)
            .await
            .expect("apply PostgreSQL migration prefix");
    }

    pub fn service(&self) -> CanonicalApparatusService {
        CanonicalApparatusService::new(std::sync::Arc::new(
            PostgresCanonicalApparatusRepository::new(self.pool.clone()),
        ))
    }

    pub async fn close(self) {
        self.pool.close().await;
        let admin_pool = PgPool::connect(&self.admin_url)
            .await
            .expect("connect for canonical test cleanup");
        sqlx::query(&format!(
            r#"DROP DATABASE IF EXISTS "{}" WITH (FORCE)"#,
            self.name
        ))
        .execute(&admin_pool)
        .await
        .expect("drop canonical test database");
        admin_pool.close().await;
    }
}

pub(super) fn draft(physical_asset_id: &str, display_name: &str) -> CanonicalApparatusDraft {
    let mut draft = revision_with(
        "apparatus:test:draft-fixture",
        physical_asset_id,
        display_name,
    )
    .to_draft();
    draft
        .placement
        .as_mut()
        .expect("fixture has map placement")
        .factory_map_object_id = format!("factory-map-object:{physical_asset_id}");
    draft
}

pub(super) fn metadata(command: impl Into<String>) -> CanonicalCommandMetadata {
    CanonicalCommandMetadata::new("user:test-admin", command)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ApparatusDbState {
    pub identities: i64,
    pub revisions: i64,
    pub heads: i64,
    pub runtime: i64,
    pub queue: i64,
    pub material: i64,
    pub capacity: i64,
    pub outbox: i64,
    pub head_revision: Option<i64>,
    pub runtime_revision: Option<i64>,
    pub drift: i64,
}

pub(super) async fn apparatus_state(pool: &PgPool, apparatus_id: &ApparatusId) -> ApparatusDbState {
    let row = sqlx::query(
        "SELECT
             (SELECT COUNT(*) FROM mini_canonical_apparatus_identities
               WHERE apparatus_id = $1) AS identities,
             (SELECT COUNT(*) FROM mini_canonical_apparatus_revisions
               WHERE apparatus_id = $1) AS revisions,
             (SELECT COUNT(*) FROM mini_canonical_apparatus_heads
               WHERE apparatus_id = $1) AS heads,
             (SELECT COUNT(*) FROM mini_apparatus
               WHERE id = $1 AND source_revision IS NOT NULL) AS runtime,
             (SELECT COUNT(*) FROM mini_apparatus_queue_policies
               WHERE canonical_apparatus_id = $1 AND source_revision IS NOT NULL) AS queue,
             (SELECT COUNT(*) FROM mini_apparatus_material_rules
               WHERE canonical_apparatus_id = $1 AND source_revision IS NOT NULL) AS material,
             (SELECT COUNT(*) FROM mini_apparatus_capacity_profiles
               WHERE canonical_apparatus_id = $1 AND source_revision IS NOT NULL) AS capacity,
             (SELECT COUNT(*) FROM mini_canonical_apparatus_change_outbox
               WHERE apparatus_id = $1) AS outbox,
             (SELECT current_revision FROM mini_canonical_apparatus_heads
               WHERE apparatus_id = $1) AS head_revision,
             (SELECT source_revision FROM mini_apparatus
               WHERE id = $1) AS runtime_revision,
             (SELECT COUNT(*) FROM mini_canonical_apparatus_projection_drift
               WHERE apparatus_id = $1) AS drift",
    )
    .bind(apparatus_id.as_str())
    .fetch_one(pool)
    .await
    .expect("read canonical apparatus database state");
    ApparatusDbState {
        identities: row.get("identities"),
        revisions: row.get("revisions"),
        heads: row.get("heads"),
        runtime: row.get("runtime"),
        queue: row.get("queue"),
        material: row.get("material"),
        capacity: row.get("capacity"),
        outbox: row.get("outbox"),
        head_revision: row.get("head_revision"),
        runtime_revision: row.get("runtime_revision"),
        drift: row.get("drift"),
    }
}
