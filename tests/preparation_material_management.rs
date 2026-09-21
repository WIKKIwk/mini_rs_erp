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
fn rename(code: &str, name: &str) -> MaterialRename {
    MaterialRename {
        item_code: code.into(),
        name: name.into(),
    }
}
fn warehouses(code: &str, names: &[&str]) -> MaterialWarehousesUpdate {
    MaterialWarehousesUpdate {
        item_code: code.into(),
        warehouses: names.iter().map(|name| (*name).into()).collect(),
    }
}
async fn material(store: &PostgresPreparationStore, owner: &str, name: &str) -> String {
    store
        .create_material(
            &actor(owner),
            MaterialCreate {
                request_id: format!("test-{owner}-{}", name.replace(' ', "-")),
                warehouse: if owner == "prep-1" { "Own" } else { "Other" }.into(),
                name: name.into(),
            },
        )
        .await
        .unwrap()["item_code"]
        .as_str()
        .unwrap()
        .into()
}

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated database"]
async fn own_material_list_rename_and_safe_delete() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .try_init();
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let db = format!("mini_test_prep_mat_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    let options = url.parse::<PgConnectOptions>().unwrap().database(&db);
    let pool = PgPool::connect_with(options.clone()).await.unwrap();
    let checks = pool.clone();
    let result = tokio::spawn(async move { run_checks(&checks, options).await }).await;
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
    apply_postgres_migrations_through_version(pool, "0128")
        .await
        .unwrap();
    sqlx::raw_sql("INSERT INTO mini_system_users(id,role,name,phone) VALUES
        ('prep-1','tayyorlov_masteri','Master','901234567'),('prep-2','tayyorlov_masteri','Other','901234568');
        INSERT INTO mini_warehouses(id,name) VALUES ('own','Own'),('other','Other'),('second','Second'),('shared','Shared');
        INSERT INTO mini_warehouse_assignments(assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
        VALUES ('warehouse','Own','Own','tayyorlov_masteri','prep-1'),('warehouse','Other','Other','tayyorlov_masteri','prep-2'),
          ('warehouse','Second','Second','tayyorlov_masteri','prep-1'),
          ('warehouse','Shared','Shared','tayyorlov_masteri','prep-1'),
          ('warehouse','Shared','Shared','tayyorlov_masteri','prep-2');")
        .execute(pool).await.unwrap();
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
    assert_eq!(
        store.list_owned_materials("prep-1").await.unwrap()["materials"],
        json!([])
    );
    let first = material(&store, "prep-1", "Kley").await;
    let second = material(&store, "prep-1", "Wrong material").await;
    let foreign = material(&store, "prep-2", "Other material").await;
    let listed = store.list_owned_materials("prep-1").await.unwrap();
    assert_eq!(listed["materials"].as_array().unwrap().len(), 2);
    assert!(
        listed["materials"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["item_code"] != foreign)
    );
    assert_eq!(listed["materials"][0]["warehouses"], json!(["Own"]));
    assert!(matches!(
        store
            .update_owned_material_warehouses("prep-2", warehouses(&second, &["Other"]))
            .await,
        Err(PreparationError::MaterialNotOwned)
    ));
    for destination in ["Other", "Shared", "Missing"] {
        assert!(matches!(
            store
                .update_owned_material_warehouses(
                    "prep-1",
                    warehouses(&second, &["Second", destination])
                )
                .await,
            Err(PreparationError::WarehouseNotExclusive)
        ));
    }
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT warehouse_name FROM mini_preparation_materials WHERE item_code=$1"
        )
        .bind(&second)
        .fetch_one(pool)
        .await
        .unwrap(),
        "Own"
    );
    let changed = store
        .update_owned_material_warehouses("prep-1", warehouses(&second, &[" second ", "Second"]))
        .await
        .unwrap();
    assert_eq!(changed["warehouses"], json!(["Second"]));
    assert_eq!(changed["item_code"], second);
    for destinations in [vec![], vec!["Own", "Second"], vec!["Own"]] {
        let saved = store
            .update_owned_material_warehouses("prep-1", warehouses(&second, &destinations))
            .await
            .unwrap();
        assert_eq!(saved["warehouses"], json!(destinations));
        if destinations.is_empty() {
            let listed = store.list_owned_materials("prep-1").await.unwrap();
            assert_eq!(
                listed["materials"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|m| m["item_code"] == second)
                    .unwrap()["warehouses"],
                json!([])
            );
            assert!(matches!(
                store
                    .receive(
                        &actor("prep-1"),
                        ReceiptCreate {
                            request_id: "unbound-denied".into(),
                            item_code: second.clone(),
                            warehouse: "Own".into(),
                            kg: "1".into()
                        }
                    )
                    .await,
                Err(PreparationError::MaterialNotInWarehouse)
            ));
        }
    }
    assert!(matches!(
        store
            .rename_owned_material("prep-2", rename(&first, "Stolen"))
            .await,
        Err(PreparationError::MaterialNotOwned)
    ));
    assert!(matches!(
        store.delete_owned_material("prep-2", &first).await,
        Err(PreparationError::MaterialNotOwned)
    ));
    assert!(matches!(
        store
            .rename_owned_material("prep-1", rename(&first, "   "))
            .await,
        Err(PreparationError::Invalid(_))
    ));
    assert!(matches!(
        store
            .rename_owned_material("prep-1", rename(&first, &"a".repeat(161)))
            .await,
        Err(PreparationError::Invalid(_))
    ));
    assert!(matches!(
        store
            .rename_owned_material("prep-1", rename(&second, " kLey "))
            .await,
        Err(PreparationError::MaterialNameTaken)
    ));

    store
        .receive(
            &actor("prep-1"),
            ReceiptCreate {
                request_id: "test-receipt-1".into(),
                item_code: first.clone(),
                warehouse: "Own".into(),
                kg: "7.123456".into(),
            },
        )
        .await
        .unwrap();
    sqlx::query("INSERT INTO mini_preparation_formulas(owner_ref,product_code,lines) VALUES ('prep-1','P1',$1)")
        .bind(json!([{"item_code":first,"name":"Kley","percent":"3.5"}])).execute(pool).await.unwrap();
    store
        .rename_owned_material("prep-1", rename(&first, "  New   glue  "))
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT name FROM mini_items WHERE code=$1")
            .bind(&first)
            .fetch_one(pool)
            .await
            .unwrap(),
        "New glue"
    );
    let stock: (String, String) = sqlx::query_as(
        "SELECT item_name,qty::text FROM mini_raw_material_stock WHERE item_code=$1",
    )
    .bind(&first)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(stock, ("New glue".into(), "7.123456".into()));
    assert!(matches!(
        store
            .update_owned_material_warehouses("prep-1", warehouses(&first, &["Second"]))
            .await,
        Err(PreparationError::MaterialWarehouseInUse)
    ));
    assert_eq!(
        store
            .update_owned_material_warehouses("prep-1", warehouses(&first, &["Own", "Second"]))
            .await
            .unwrap()["warehouses"],
        json!(["Own", "Second"])
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT warehouse FROM mini_raw_material_stock WHERE item_code=$1"
        )
        .bind(&first)
        .fetch_one(pool)
        .await
        .unwrap(),
        "Own"
    );
    let lines: Value =
        sqlx::query_scalar("SELECT lines FROM mini_preparation_formulas WHERE product_code='P1'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(lines[0]["name"], "New glue");
    assert_eq!(lines[0]["percent"], "3.5");
    let old: Value = sqlx::query_scalar(
        "SELECT response_json FROM mini_preparation_operations WHERE request_id='test-receipt-1'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(old["name"], "Kley");
    assert!(matches!(
        store.delete_owned_material("prep-1", &first).await,
        Err(PreparationError::MaterialInUse)
    ));

    // A formula alone blocks deletion even before the first receipt.
    sqlx::query("INSERT INTO mini_preparation_formulas(owner_ref,product_code,lines) VALUES ('prep-1','P2',$1)")
        .bind(json!([{"item_code":second,"name":"Wrong material","percent":"1"}])).execute(pool).await.unwrap();
    assert!(matches!(
        store.delete_owned_material("prep-1", &second).await,
        Err(PreparationError::MaterialInUse)
    ));
    sqlx::query("DELETE FROM mini_preparation_formulas WHERE product_code='P2'")
        .execute(pool)
        .await
        .unwrap();
    store
        .delete_owned_material("prep-1", &second)
        .await
        .unwrap();
    for table in [
        "mini_items",
        "mini_preparation_materials",
        "mini_preparation_material_warehouse_scopes",
    ] {
        let col = if table == "mini_items" {
            "code"
        } else {
            "item_code"
        };
        assert_eq!(
            sqlx::query_scalar::<_, i64>(&format!("SELECT count(*) FROM {table} WHERE {col}=$1"))
                .bind(&second)
                .fetch_one(pool)
                .await
                .unwrap(),
            0
        );
    }
    assert_eq!(
        store.list_owned_materials("prep-1").await.unwrap()["materials"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        store
            .receive(
                &actor("prep-1"),
                ReceiptCreate {
                    request_id: "test-deleted-receipt".into(),
                    item_code: second.clone(),
                    warehouse: "Own".into(),
                    kg: "1".into()
                }
            )
            .await,
        Err(PreparationError::MaterialNotInWarehouse)
    ));
    // Creation command retries are immutable and never resurrect a deleted item.
    assert_eq!(material(&store, "prep-1", "Wrong material").await, second);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mini_items WHERE code=$1")
            .bind(&second)
            .fetch_one(pool)
            .await
            .unwrap(),
        0
    );
    let a = material(&store, "prep-1", "Parallel A").await;
    let b = material(&store, "prep-1", "Parallel B").await;
    let (left, right) = tokio::join!(
        store.rename_owned_material("prep-1", rename(&a, "Same name")),
        store.rename_owned_material("prep-1", rename(&b, "Same NAME"))
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    assert!(matches!(
        left.err().or(right.err()),
        Some(PreparationError::MaterialNameTaken)
    ));
    // A receipt racing deletion must never leave a stock row without its item.
    let race = material(&store, "prep-1", "Receipt race").await;
    let receiver = actor("prep-1");
    let (deleted, received) = tokio::join!(
        store.delete_owned_material("prep-1", &race),
        store.receive(
            &receiver,
            ReceiptCreate {
                request_id: "test-receipt-race".into(),
                item_code: race.clone(),
                warehouse: "Own".into(),
                kg: "1".into()
            }
        )
    );
    assert_ne!(deleted.is_ok(), received.is_ok());
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM mini_raw_material_stock s
        LEFT JOIN mini_items i ON i.code=s.item_code WHERE s.item_code=$1 AND i.code IS NULL"
        )
        .bind(&race)
        .fetch_one(pool)
        .await
        .unwrap(),
        0
    );
    // Undoing an accidental material creation also releases an empty child warehouse.
    store
        .create_child_warehouse(
            &actor("prep-1"),
            PreparationWarehouseCreate {
                parent_warehouse: "Own".into(),
                name: "Empty child".into(),
            },
        )
        .await
        .unwrap();
    let accidental = store
        .create_material(
            &actor("prep-1"),
            MaterialCreate {
                request_id: "test-accidental-child".into(),
                warehouse: "Empty child".into(),
                name: "Accidental".into(),
            },
        )
        .await
        .unwrap();
    store
        .delete_owned_material("prep-1", accidental["item_code"].as_str().unwrap())
        .await
        .unwrap();
    store
        .delete_child_warehouse("prep-1", "Empty child")
        .await
        .unwrap();
    // Existing shared/admin links are preserved and cannot be removed by a master.
    let shared = material(&store, "prep-1", "Shared binding").await;
    sqlx::query(
        "INSERT INTO mini_preparation_material_warehouse_scopes
        (item_code,warehouse_id,scope_kind,created_by_role,created_by_ref)
        VALUES($1,'shared','shared','admin','admin-1')",
    )
    .bind(&shared)
    .execute(pool)
    .await
    .unwrap();
    assert!(matches!(
        store
            .update_owned_material_warehouses("prep-1", warehouses(&shared, &["Own"]))
            .await,
        Err(PreparationError::WarehouseNotExclusive)
    ));
    store
        .update_owned_material_warehouses(
            "prep-1",
            warehouses(&shared, &["Own", "Second", "Shared"]),
        )
        .await
        .unwrap();
    assert_eq!(sqlx::query_as::<_, (String,String)>("SELECT scope_kind,created_by_ref FROM mini_preparation_material_warehouse_scopes WHERE item_code=$1 AND warehouse_id='shared' AND active")
        .bind(&shared).fetch_one(pool).await.unwrap(), ("shared".into(), "admin-1".into()));

    // In-flight incoming transfers and pending scale receipts also protect a link.
    let pending = material(&store, "prep-1", "Pending incoming").await;
    sqlx::query("INSERT INTO mini_inventory_transfers
        (id,idempotency_key,source_warehouse_id,source_warehouse,destination_warehouse_id,destination_warehouse,requested_by_role,requested_by_ref)
        VALUES('scope-transfer','scope-transfer','second','Second','own','Own','tayyorlov_masteri','prep-1')")
        .execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO mini_inventory_transfer_lines
        (transfer_id,asset_kind,asset_ref,item_code,qty,uom,source_physical_location_id)
        VALUES('scope-transfer','raw_material','incoming-lot',$1,1,'kg','')",
    )
    .bind(&pending)
    .execute(pool)
    .await
    .unwrap();
    assert!(matches!(
        store
            .update_owned_material_warehouses("prep-1", warehouses(&pending, &["Second"]))
            .await,
        Err(PreparationError::MaterialWarehouseInUse)
    ));
    sqlx::query("UPDATE mini_inventory_transfers SET status='cancelled' WHERE id='scope-transfer'")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO mini_gscale_receipts(name,item_code,warehouse,qty,barcode) VALUES('pending-scope',$1,'Own',1,'pending-scope')")
        .bind(&pending).execute(pool).await.unwrap();
    assert!(matches!(
        store
            .update_owned_material_warehouses("prep-1", warehouses(&pending, &["Second"]))
            .await,
        Err(PreparationError::MaterialWarehouseInUse)
    ));
    sqlx::query("UPDATE mini_gscale_receipts SET status='submitted' WHERE name='pending-scope'")
        .execute(pool)
        .await
        .unwrap();
    store
        .update_owned_material_warehouses("prep-1", warehouses(&pending, &["Second"]))
        .await
        .unwrap();

    // A legacy unscoped material may already have stock; assigning its first
    // explicit warehouse must not hide that stock in another warehouse.
    sqlx::query("DELETE FROM mini_preparation_material_warehouse_scopes WHERE item_code=$1")
        .bind(&first)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE mini_preparation_materials SET warehouse_name=NULL WHERE item_code=$1")
        .bind(&first)
        .execute(pool)
        .await
        .unwrap();
    assert!(matches!(
        store
            .update_owned_material_warehouses("prep-1", warehouses(&first, &["Second"]))
            .await,
        Err(PreparationError::MaterialWarehouseInUse)
    ));
    store
        .update_owned_material_warehouses("prep-1", warehouses(&first, &["Own", "Second"]))
        .await
        .unwrap();
    let scope_race = material(&store, "prep-1", "Scope receipt race").await;
    let receiver = actor("prep-1");
    let (updated, received) = tokio::join!(
        store.update_owned_material_warehouses("prep-1", warehouses(&scope_race, &["Second"])),
        store.receive(
            &receiver,
            ReceiptCreate {
                request_id: "scope-receipt-race".into(),
                item_code: scope_race.clone(),
                warehouse: "Own".into(),
                kg: "1".into()
            }
        )
    );
    assert_ne!(updated.is_ok(), received.is_ok());
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mini_raw_material_stock s
        WHERE s.item_code=$1 AND NOT EXISTS(SELECT 1 FROM mini_preparation_material_warehouse_scopes scope
            JOIN mini_warehouses w ON w.id=scope.warehouse_id
            WHERE scope.item_code=s.item_code AND scope.active AND w.name=s.warehouse)")
        .bind(&scope_race).fetch_one(pool).await.unwrap(), 0);
    runtime.close().await;
}
