use mini_rs_erp::core::{
    auth::models::{Principal, PrincipalRole},
    preparation::{MaterialCreate, PreparationError},
};
use mini_rs_erp::db::{
    postgres::{apply_postgres_migrations_through_version, canonical_apparatus_service},
    postgres_preparation::{
        PostgresPreparationStore,
        paddons::{PaddonCreate, PaddonQuery},
    },
};
use serde_json::{Value, json};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

fn actor(role: PrincipalRole, id: &str) -> Principal {
    Principal {
        role,
        ref_: id.into(),
        display_name: id.into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    }
}

fn receipt(warehouse: &str, material: &str, key: &str) -> PaddonCreate {
    serde_json::from_value(json!({
        "request_id": key,
        "warehouse": warehouse,
        "contents": [{
            "item_code": material, "lot_number": "LOT-1",
            "kind": "bag", "count": 3, "unit_kg": "25"
        }]
    }))
    .unwrap()
}

fn filter(status: &str, search: &str) -> PaddonQuery {
    serde_json::from_value(json!({"status": status, "q": search})).unwrap()
}

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated database"]
async fn both_roles_receive_only_their_paddons_and_read_live_stock() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let name = format!("paddon_test_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    let options = url.parse::<PgConnectOptions>().unwrap().database(&name);
    let pool = PgPool::connect_with(options.clone()).await.unwrap();
    let checks = pool.clone();
    let result = tokio::spawn(async move { verify(&checks, options).await }).await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    result.unwrap();
}

async fn verify(pool: &PgPool, options: PgConnectOptions) {
    apply_postgres_migrations_through_version(pool, "0121")
        .await
        .unwrap();
    canonical_apparatus_service(pool.clone())
        .bootstrap_factory_defaults()
        .await
        .unwrap();
    apply_postgres_migrations_through_version(pool, "0201")
        .await
        .unwrap();
    sqlx::raw_sql(
        "INSERT INTO mini_system_users(id,role,name,phone)
             VALUES ('prep-1','tayyorlov_masteri','Master','901234567');
         INSERT INTO mini_warehouses(id,name) VALUES ('own','Own'),('supply','Supply');
         INSERT INTO mini_warehouse_assignments
             (assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
             VALUES ('warehouse','Own','Own','tayyorlov_masteri','prep-1'),
                    ('warehouse','Supply','Supply','material_taminotchi','supply-1');",
    )
    .execute(pool)
    .await
    .unwrap();
    let runtime = PgPoolOptions::new()
        .after_connect(|conn, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE mini_rs_erp").execute(conn).await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await
        .unwrap();
    let store = PostgresPreparationStore::new(runtime.clone());
    let master = actor(PrincipalRole::TayyorlovMasteri, "prep-1");
    let supplier = actor(PrincipalRole::MaterialTaminotchi, "supply-1");
    let material = store
        .create_material(
            &master,
            MaterialCreate {
                request_id: "material-1".into(),
                warehouse: "Own".into(),
                name: "Granula".into(),
            },
        )
        .await
        .unwrap();
    let item = material["item_code"].as_str().unwrap();
    let groups = vec![material["item_group"].as_str().unwrap().to_lowercase()];
    assert!(matches!(
        store
            .receive_raw_material_paddon(
                &supplier,
                receipt("Own", item, "wrong-warehouse"),
                &groups
            )
            .await,
        Err(PreparationError::Forbidden)
    ));
    assert!(matches!(
        store
            .receive_raw_material_paddon(&supplier, receipt("Supply", item, "wrong-group"), &[])
            .await,
        Err(PreparationError::Forbidden)
    ));

    let named_receipt = |name: &str| -> PaddonCreate {
        let mut payload = serde_json::to_value(receipt("Own", item, "master-1")).unwrap();
        payload["name"] = json!(name);
        serde_json::from_value(payload).unwrap()
    };
    let (first, retry) = tokio::join!(
        store.receive_raw_material_paddon(&master, named_receipt("  Oq zaxira  "), &[]),
        store.receive_raw_material_paddon(&master, named_receipt("Oq zaxira"), &[]),
    );
    let first = first.unwrap();
    assert_eq!(first, retry.unwrap());
    let second = store
        .receive_raw_material_paddon(&supplier, receipt("Supply", item, "supply-1"), &groups)
        .await
        .unwrap();
    assert_ne!(first["paddon_id"], second["paddon_id"]);
    let stock_count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_raw_material_stock")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(stock_count, 6);
    let receipt_count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_preparation_receipts")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(receipt_count, 3);
    let master_list = store
        .raw_material_paddons(&master, filter("available", "LOT-1"))
        .await
        .unwrap();
    let master_rows = master_list["paddons"].as_array().unwrap();
    assert_eq!(master_rows.len(), 1);
    assert_eq!(master_rows[0]["paddon_code"], first["paddon_code"]);
    assert_eq!(master_rows[0]["name"], "Oq zaxira");
    assert_eq!(master_rows[0]["contents"][0]["remaining_count"], 3);
    assert_eq!(master_rows[0]["remaining_kg"], "75.000000");
    let by_name = store
        .raw_material_paddons(&master, filter("available", " zAxIrA "))
        .await
        .unwrap();
    assert_eq!(by_name, master_list);
    let supplier_list: Value = store
        .raw_material_paddons(&supplier, filter("all", ""))
        .await
        .unwrap();
    assert_eq!(
        supplier_list["paddons"][0]["paddon_code"],
        second["paddon_code"]
    );
    assert_eq!(supplier_list["paddons"][0]["name"], "");
    assert_eq!(supplier_list["paddons"].as_array().unwrap().len(), 1);

    sqlx::query("UPDATE mini_raw_material_stock SET status = 'consumed' WHERE warehouse = 'Own'")
        .execute(pool)
        .await
        .unwrap();
    let empty = store
        .raw_material_paddons(&master, filter("empty", ""))
        .await
        .unwrap();
    assert_eq!(empty["paddons"][0]["contents"][0]["remaining_count"], 0);
    let active = store
        .raw_material_paddons(&master, filter("available", ""))
        .await
        .unwrap();
    assert_eq!(active["paddons"], json!([]));
    runtime.close().await;
}
