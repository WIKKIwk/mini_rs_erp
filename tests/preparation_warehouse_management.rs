use mini_rs_erp::core::{
    auth::models::{Principal, PrincipalRole},
    preparation::*,
};
use mini_rs_erp::db::{
    postgres::{apply_postgres_migrations_through_version, canonical_apparatus_service},
    postgres_preparation::PostgresPreparationStore,
};
use serde_json::{Value, json};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

fn actor(id: &str) -> Principal {
    Principal {
        role: PrincipalRole::TayyorlovMasteri,
        ref_: id.into(),
        display_name: "Master".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    }
}
fn create(parent: &str, name: &str) -> PreparationWarehouseCreate {
    PreparationWarehouseCreate {
        parent_warehouse: parent.into(),
        name: name.into(),
    }
}
fn rename(warehouse: &str, name: &str) -> PreparationWarehouseRename {
    PreparationWarehouseRename {
        warehouse: warehouse.into(),
        name: name.into(),
    }
}

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated database"]
async fn preparation_warehouse_ownership_atomicity_stock_and_rename() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .try_init();
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let db = format!("mini_test_prep_wh_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    let options = url.parse::<PgConnectOptions>().unwrap().database(&db);
    let pool = PgPool::connect_with(options.clone()).await.unwrap();
    // A task boundary lets cleanup run even when an assertion fails.
    let check_pool = pool.clone();
    let result = tokio::spawn(async move { run_checks(&check_pool, options).await }).await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

async fn run_checks(pool: &PgPool, options: PgConnectOptions) {
    apply_postgres_migrations_through_version(pool, "0121")
        .await
        .unwrap();
    canonical_apparatus_service(pool.clone())
        .bootstrap_factory_defaults()
        .await
        .unwrap();
    apply_postgres_migrations_through_version(pool, "0126")
        .await
        .unwrap();
    sqlx::raw_sql("INSERT INTO mini_system_users(id,role,name,phone) VALUES
        ('prep-1','tayyorlov_masteri','Master','901234567'), ('prep-2','tayyorlov_masteri','Other','901234568');
        INSERT INTO mini_warehouses(id,name,parent_warehouse) VALUES
        ('root','Own',''), ('shared','Shared',''), ('other','Other',''),
        ('legacy','Legacy child','Own'), ('legacy-shared','Shared child','Shared');
        INSERT INTO mini_warehouse_assignments(assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
        VALUES ('warehouse','Own','Own','tayyorlov_masteri','prep-1'),
          ('warehouse','Shared','Shared','tayyorlov_masteri','prep-1'),
          ('warehouse','Shared','Shared','material_taminotchi','mt-1'),
          ('warehouse','Other','Other','tayyorlov_masteri','prep-2'),
          ('warehouse','Legacy child','Legacy child','tayyorlov_masteri','prep-1'),
          ('warehouse','Shared child','Shared child','tayyorlov_masteri','prep-1');")
        .execute(pool).await.unwrap();
    apply_postgres_migrations_through_version(pool, "0127")
        .await
        .unwrap();
    // Exercise the actual restricted runtime grants, including trigger/lock privileges.
    let runtime = PgPoolOptions::new()
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE mini_rs_erp")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await
        .unwrap();
    let store = PostgresPreparationStore::new(runtime.clone());
    let master = actor("prep-1");
    let snapshot = store.snapshot("prep-1").await.unwrap();
    assert!(
        snapshot["managed_warehouses"]
            .as_array()
            .unwrap()
            .contains(&json!("Legacy child"))
    );
    assert!(
        !snapshot["managed_warehouses"]
            .as_array()
            .unwrap()
            .contains(&json!("Shared child"))
    );
    for parent in ["Shared", "Other", "Missing"] {
        assert!(matches!(
            store
                .create_child_warehouse(&master, create(parent, "Denied"))
                .await,
            Err(PreparationError::WarehouseNotExclusive)
        ));
    }
    assert!(matches!(
        store.delete_child_warehouse("prep-1", "Own").await,
        Err(PreparationError::WarehouseNotOwned)
    ));
    assert!(matches!(
        store
            .rename_child_warehouse("prep-2", rename("Legacy child", "Stolen"))
            .await,
        Err(PreparationError::WarehouseNotOwned)
    ));
    // Assignment failure rolls the warehouse and its automatic location back too.
    sqlx::raw_sql("CREATE FUNCTION reject_test_assignment() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN IF NEW.warehouse_name='Rollback child' THEN RAISE EXCEPTION 'test assignment failure'; END IF;
        RETURN NEW; END $$;
        CREATE TRIGGER reject_test_assignment BEFORE INSERT ON mini_warehouse_assignments
        FOR EACH ROW EXECUTE FUNCTION reject_test_assignment();").execute(pool).await.unwrap();
    assert!(
        store
            .create_child_warehouse(&master, create("Own", "Rollback child"))
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM mini_warehouses WHERE name='Rollback child'"
        )
        .fetch_one(pool)
        .await
        .unwrap(),
        0
    );
    sqlx::raw_sql("DROP TRIGGER reject_test_assignment ON mini_warehouse_assignments; DROP FUNCTION reject_test_assignment();")
        .execute(pool).await.unwrap();
    let (a, b) = tokio::join!(
        store.create_child_warehouse(&master, create("Own", "Race")),
        store.create_child_warehouse(&master, create("Own", "race"))
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(matches!(
        a.as_ref().err().or(b.as_ref().err()),
        Some(PreparationError::WarehouseNameTaken)
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM mini_warehouse_assignments WHERE lower(warehouse_name)='race'"
        )
        .fetch_one(pool)
        .await
        .unwrap(),
        1
    );
    store
        .delete_child_warehouse("prep-1", "race")
        .await
        .unwrap();
    store
        .create_child_warehouse(&master, create("Own", "Filled"))
        .await
        .unwrap();
    let before: String = sqlx::query_scalar("SELECT id FROM mini_warehouses WHERE name='Filled'")
        .fetch_one(pool)
        .await
        .unwrap();
    let material = store
        .create_material(
            &master,
            MaterialCreate {
                request_id: "material-test-1".into(),
                name: "Kley".into(),
                warehouse: "Filled".into(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        store.delete_child_warehouse("prep-1", "Filled").await,
        Err(PreparationError::WarehouseHasMaterials)
    ));
    let code = material["item_code"].as_str().unwrap().to_string();
    store
        .receive(
            &master,
            ReceiptCreate {
                request_id: "receipt-test-1".into(),
                item_code: code.clone(),
                warehouse: "Filled".into(),
                kg: "13.00003".into(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        store.delete_child_warehouse("prep-1", "Filled").await,
        Err(PreparationError::WarehouseNotEmpty)
    ));
    assert!(matches!(
        store
            .rename_child_warehouse("prep-1", rename("Filled", " own "))
            .await,
        Err(PreparationError::WarehouseNameTaken)
    ));
    assert!(matches!(
        store
            .rename_child_warehouse("prep-1", rename("Filled", "  "))
            .await,
        Err(PreparationError::Invalid(_))
    ));
    store
        .create_child_warehouse(&master, create("Filled", "Nested"))
        .await
        .unwrap();
    store
        .rename_child_warehouse("prep-1", rename("Filled", "Renamed"))
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT id FROM mini_warehouses WHERE name='Renamed'")
            .fetch_one(pool)
            .await
            .unwrap(),
        before
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT parent_warehouse FROM mini_warehouses WHERE name='Nested'"
        )
        .fetch_one(pool)
        .await
        .unwrap(),
        "Renamed"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT warehouse FROM mini_raw_material_stock WHERE item_code=$1"
        )
        .bind(&code)
        .fetch_one(pool)
        .await
        .unwrap(),
        "Renamed"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT warehouse_name FROM mini_preparation_materials WHERE item_code=$1"
        )
        .bind(&code)
        .fetch_one(pool)
        .await
        .unwrap(),
        "Renamed"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT name FROM mini_inventory_locations WHERE warehouse_id=$1"
        )
        .bind(&before)
        .fetch_one(pool)
        .await
        .unwrap(),
        "Renamed"
    );
    let snapshot = store.snapshot("prep-1").await.unwrap();
    let balances = snapshot["materials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["item_code"] == code)
        .unwrap()["balances"]
        .clone();
    assert_eq!(balances[0]["warehouse"], "Renamed");
    assert_eq!(balances[0]["kg"], "13.000030");
    assert_eq!(snapshot["history"][0]["warehouse"], "Renamed");
    assert_eq!(sqlx::query_scalar::<_,Value>("SELECT response_json FROM mini_preparation_operations WHERE request_id='receipt-test-1'").fetch_one(pool).await.unwrap()["warehouse"], "Filled");
    store
        .rename_child_warehouse("prep-1", rename("Renamed", "RENAMED"))
        .await
        .unwrap();
    store
        .create_child_warehouse(&master, create("Own", "Filled"))
        .await
        .unwrap();
    store
        .delete_child_warehouse("prep-1", "Filled")
        .await
        .unwrap();
    assert!(matches!(
        store.delete_child_warehouse("prep-1", "RENAMED").await,
        Err(PreparationError::WarehouseHasChildren)
    ));
    store
        .delete_child_warehouse("prep-1", "Nested")
        .await
        .unwrap();
    // An inventory placement alone must prevent deletion, even without a stock row.
    store
        .create_child_warehouse(&master, create("Own", "Located"))
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO mini_inventory_placements(asset_kind,asset_ref,physical_location_id)
        SELECT 'raw_material','test-located',l.id FROM mini_inventory_locations l
        JOIN mini_warehouses w ON w.id=l.warehouse_id WHERE w.name='Located'",
    )
    .execute(pool)
    .await
    .unwrap();
    assert!(matches!(
        store.delete_child_warehouse("prep-1", "Located").await,
        Err(PreparationError::WarehouseNotEmpty)
    ));
    // Existing receipt workflows still work after the warehouse was renamed.
    let receipt_id: String = sqlx::query_scalar(
        "SELECT id FROM mini_preparation_operations WHERE request_id='receipt-test-1'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    store
        .reverse_receipt(
            &master,
            ReceiptReversalCreate {
                request_id: "reverse-test-1".into(),
                receipt_id,
                reason: "Test reversal after rename".into(),
            },
        )
        .await
        .unwrap();
    // Revocation/shared assignment removes management rights at the server too.
    sqlx::query("INSERT INTO mini_warehouse_assignments(assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
        VALUES ('warehouse','Legacy child','Legacy child','tayyorlov_masteri','prep-2')").execute(pool).await.unwrap();
    assert!(matches!(
        store.delete_child_warehouse("prep-1", "Legacy child").await,
        Err(PreparationError::WarehouseNotOwned)
    ));
    runtime.close().await;
}
