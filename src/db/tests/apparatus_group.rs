use std::sync::Arc;

use crate::core::apparatus_groups::{
    ApparatusGroupService, ApparatusGroupUpsert, ApparatusMasterData,
};
use crate::core::apparatus_standard::{
    ApparatusId, MaterialPolicy, QueuePolicy, RawMaterialStartPolicy,
};
use crate::db::postgres::apply_foundation_migration;
use crate::db::postgres_apparatus_group::PostgresApparatusGroupStore;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_apparatus_groups"]
async fn postgres_apparatus_group_store_round_trips_groups() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_apparatus_groups";
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");
    admin_pool.close().await;

    let test_url = format!("postgres://wikki@127.0.0.1:5432/{db_name}");
    let pool = sqlx::PgPool::connect(&test_url).await.expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migration");
    let store = Arc::new(PostgresApparatusGroupStore::new(pool.clone()));
    let service = ApparatusGroupService::new(store.clone());

    let saved = service
        .upsert_group(ApparatusGroupUpsert {
            name: " pechat ".to_string(),
            apparatus: vec![
                "apparatus:default:bosma_7".to_string(),
                "apparatus:default:bosma_8".to_string(),
                "apparatus:default:bosma_7".to_string(),
            ],
        })
        .await
        .expect("save group");
    assert_eq!(saved.name, "Bosma aparat");
    assert_eq!(
        saved.apparatus,
        vec![
            "apparatus:default:bosma_7".to_string(),
            "apparatus:default:bosma_8".to_string(),
            "apparatus:default:bosma_9".to_string(),
            "apparatus:default:asset-005".to_string(),
        ]
    );

    let reloaded = service.groups().await.expect("load groups");
    assert_eq!(reloaded, vec![saved]);

    let created = service
        .upsert_apparatus(crate::core::apparatus_groups::ApparatusUpsert {
            id: None,
            name: " Bobst 1 ".to_string(),
            master: ApparatusMasterData::default(),
        })
        .await
        .expect("save apparatus");
    assert_eq!(created.name, "Bobst 1");
    assert!(created.id.starts_with("apparatus:custom:"));
    let canonical_payload: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json->'canonical_apparatus'
         FROM mini_apparatus
         WHERE id = $1",
    )
    .bind(&created.id)
    .fetch_one(&pool)
    .await
    .expect("canonical sidecar");
    assert_eq!(canonical_payload["identity"]["id"], created.id);
    assert_eq!(
        service.apparatus("bob", 20).await.expect("list apparatus"),
        vec!["Bobst 1".to_string()]
    );

    store
        .put_apparatus_with_id(
            Some(&created.id),
            "Compatibility rename",
            &ApparatusMasterData::default(),
        )
        .await
        .expect("legacy projection update");
    let preserved_name: String = sqlx::query_scalar(
        "SELECT payload_json #>> '{canonical_apparatus,identity,display,display_name}'
         FROM mini_apparatus
         WHERE id = $1",
    )
    .bind(&created.id)
    .fetch_one(&pool)
    .await
    .expect("preserved canonical sidecar");
    assert_eq!(preserved_name, "Bobst 1");

    let apparatus_id = ApparatusId::new(created.id.clone()).expect("canonical apparatus id");
    let mut canonical = service
        .canonical_apparatus_by_id(&apparatus_id)
        .await
        .expect("canonical lookup before projection sync")
        .expect("canonical apparatus before projection sync");
    canonical.identity.display.display_name = "Bobst canonical".to_string();
    canonical.versioning.revision = 2;
    canonical.capacity.capacity_slots = 3;
    canonical.policies.queue = QueuePolicy::FreePick;
    canonical.policies.material = MaterialPolicy {
        requires_material: true,
        start_policy: RawMaterialStartPolicy::StateAll,
        item_groups: vec!["Kraska".to_string()],
        requirement_groups: Vec::new(),
    };
    service
        .put_canonical_apparatus(1, canonical)
        .await
        .expect("canonical mutation with projection sync");

    let master_fields: (String, String, String) = sqlx::query_as(
        "SELECT name, base_name, kind
         FROM mini_apparatus
         WHERE id = $1",
    )
    .bind(&created.id)
    .fetch_one(&pool)
    .await
    .expect("master projection fields");
    assert_eq!(
        master_fields,
        (
            "Bobst canonical".to_string(),
            "Bobst canonical".to_string(),
            "other".to_string()
        )
    );

    let queue_projection: (String, String, String, serde_json::Value) = sqlx::query_as(
        "SELECT apparatus, canonical_apparatus_id, policy, payload_json
         FROM mini_apparatus_queue_policies
         WHERE canonical_apparatus_id = $1",
    )
    .bind(&created.id)
    .fetch_one(&pool)
    .await
    .expect("queue projection");
    assert_eq!(queue_projection.0, "Bobst canonical");
    assert_eq!(queue_projection.1, created.id);
    assert_eq!(queue_projection.2, "free_pick");
    assert_eq!(queue_projection.3["policy"], "free_pick");

    let capacity_projection: (String, String, i32) = sqlx::query_as(
        "SELECT apparatus, canonical_apparatus_id, capacity_slots
         FROM mini_apparatus_capacity_profiles
         WHERE canonical_apparatus_id = $1",
    )
    .bind(&created.id)
    .fetch_one(&pool)
    .await
    .expect("capacity projection");
    assert_eq!(capacity_projection.0, "Bobst canonical");
    assert_eq!(capacity_projection.1, created.id);
    assert_eq!(capacity_projection.2, 3);

    let material_projection: (String, String, bool, serde_json::Value) = sqlx::query_as(
        "SELECT apparatus, canonical_apparatus_id, requires_material, item_groups
         FROM mini_apparatus_material_rules
         WHERE canonical_apparatus_id = $1",
    )
    .bind(&created.id)
    .fetch_one(&pool)
    .await
    .expect("material projection");
    assert_eq!(material_projection.0, "Bobst canonical");
    assert_eq!(material_projection.1, created.id);
    assert!(material_projection.2);
    assert_eq!(material_projection.3, serde_json::json!(["Kraska"]));

    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("cleanup test db");
    admin_pool.close().await;
}
