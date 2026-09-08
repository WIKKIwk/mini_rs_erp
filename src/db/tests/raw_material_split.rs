//! Store-native transaction, exact numeric, concurrency and immutability oracles.
use crate::core::{
    auth::models::{Principal, PrincipalRole},
    raw_material_split::*,
};
use crate::db::{
    postgres::apply_foundation_migration,
    postgres_raw_material_split::PostgresRawMaterialSplitStore,
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgConnectOptions};

fn input(barcode: &str, key: &str) -> SplitCreate {
    SplitCreate {
        request_id: key.into(),
        source_barcode: barcode.into(),
        expected_kg: "100".into(),
        expected_revision: "1.000000".into(),
        expected_width_mm: "1000".into(),
        expected_micron: "20".into(),
        waste_kg: "2".into(),
        outputs: vec![
            SplitOutput {
                kg: "39".into(),
                width_mm: "400".into(),
                gross_kg: Some("40".into()),
                bobina_kg: Some("1".into()),
            },
            SplitOutput {
                kg: "59".into(),
                width_mm: "600".into(),
                gross_kg: Some("60".into()),
                bobina_kg: Some("1".into()),
            },
        ],
    }
}
async fn seed(pool: &PgPool, barcode: &str, warehouse: &str, code: &str) {
    sqlx::query("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,width_mm,micron,updated_at)
        VALUES($1,$2,$3,'Test film 1000/20',$1,100,1000,20,to_timestamp(1))").bind(barcode).bind(warehouse).bind(code).execute(pool).await.unwrap();
}
#[tokio::test]
async fn raw_material_split_postgres_atomic_exact_retry_concurrency_scope_and_lineage() {
    // Explicitly supplied disposable database only; never fall back to a live DSN.
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .expect("isolated test PostgreSQL URL required");
    let admin = PgPool::connect(&url).await.unwrap();
    let db = format!("mini_rs_erp_test_raw_split_{:016x}", rand::random::<u64>());
    sqlx::query(&format!(
        "CREATE DATABASE {db} ENCODING 'UTF8' TEMPLATE template0"
    ))
    .execute(&admin)
    .await
    .unwrap();
    let pool = PgPool::connect_with(url.parse::<PgConnectOptions>().unwrap().database(&db))
        .await
        .unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO mini_system_users(id,role,name,phone) VALUES
        ('split-1','homashyo_rezkachi','Cutter','901234591'),('split-2','homashyo_rezkachi','Other','901234592');
        INSERT INTO mini_warehouses(id,name) VALUES ('split-w','Raw W'),('split-other','Other W');
        INSERT INTO mini_warehouse_assignments(assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
            VALUES ('warehouse','Raw W','Raw W','homashyo_rezkachi','split-1');
        INSERT INTO mini_items(code,name,uom,item_group) VALUES('FILM','Test film','kg','All Item Groups'),('FAIL','Test film','kg','All Item Groups');")
        .execute(&pool).await.unwrap();
    let actor = Principal {
        role: PrincipalRole::HomashyoRezkachi,
        ref_: "split-1".into(),
        display_name: "Cutter".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let store = PostgresRawMaterialSplitStore::new(pool.clone());
    seed(&pool, "parent", "Raw W", "FILM").await;
    let mut no_waste = input("parent", "split-zero-waste");
    no_waste.waste_kg = "0".into();
    no_waste.outputs[0].kg = "41".into();
    no_waste.outputs[0].gross_kg = Some("42".into());
    assert!(matches!(
        store.split(&actor, no_waste).await,
        Err(SplitError::Invalid("Atxot 0 dan katta bo‘lishi kerak"))
    ));
    assert_eq!(
        store.source("split-1", "parent").await.unwrap()["kg"],
        "100"
    );
    assert_eq!(
        store.snapshot("split-1").await.unwrap()["history"],
        json!([])
    );
    let mut invalid = input("parent", "split-bad-balance");
    for widths in [["350", "650"], ["500", "600"], ["355", "354.999999"]] {
        let mut bad_widths = input("parent", "split-bad-widths");
        bad_widths.outputs[0].width_mm = widths[0].into();
        bad_widths.outputs[1].width_mm = widths[1].into();
        assert!(matches!(
            store.split(&actor, bad_widths).await,
            Err(SplitError::Invalid(_))
        ));
    }
    assert_eq!(
        store.source("split-1", "parent").await.unwrap()["kg"],
        "100"
    );
    assert_eq!(
        store.snapshot("split-1").await.unwrap()["history"],
        json!([])
    );
    invalid.outputs[0].kg = "39.000001".into();
    assert!(matches!(
        store.split(&actor, invalid).await,
        Err(SplitError::Invalid(_))
    ));
    let mut stale = input("parent", "split-stale-kg");
    stale.expected_kg = "99".into();
    stale.outputs[0].kg = "38".into();
    stale.outputs[0].gross_kg = Some("39".into());
    assert!(matches!(
        store.split(&actor, stale).await,
        Err(SplitError::Conflict(_))
    ));
    let other = Principal {
        ref_: "split-2".into(),
        ..actor.clone()
    };
    assert!(matches!(
        store
            .split(&other, input("parent", "split-wrong-owner"))
            .await,
        Err(SplitError::Forbidden)
    ));
    let legacy = Principal {
        role: PrincipalRole::Aparatchi,
        ..actor.clone()
    };
    assert!(matches!(
        store
            .split(&legacy, input("parent", "split-wrong-role"))
            .await,
        Err(SplitError::Forbidden)
    ));
    assert_eq!(
        store.snapshot("split-2").await.unwrap()["warehouses"],
        json!([])
    );
    let request = input("parent", "split-001");
    let (a, b) = tokio::join!(
        store.split(&actor, request.clone()),
        store.split(&actor, request.clone())
    );
    let result = a.unwrap();
    assert_eq!(result, b.unwrap());
    assert_eq!(result["output_kg"], "98.000000");
    assert_eq!(result["waste_kg"], "2.000000");
    assert_eq!(result["outputs"][0]["kg"], "39.000000");
    assert_eq!(result["outputs"][1]["kg"], "59.000000");
    assert_eq!(result["outputs"][0]["gross_kg"], "40.000000");
    assert_eq!(result["outputs"][0]["bobina_kg"], "1.000000");
    assert_eq!(result["outputs"][0]["item_name"], "Test film 400/20");
    assert_eq!(result["outputs"][1]["item_name"], "Test film 600/20");
    let weights: (String,String,String,String) = sqlx::query_as(
        "SELECT trim_scale(o.kg)::text,trim_scale(o.gross_kg)::text,trim_scale(o.bobina_kg)::text,s.item_name
        FROM mini_raw_material_split_outputs o JOIN mini_raw_material_stock s ON s.id=o.stock_id WHERE o.stock_id=$1")
        .bind(result["outputs"][0]["stock_id"].as_str()).fetch_one(&pool).await.unwrap();
    assert_eq!(
        weights,
        (
            "39".into(),
            "40".into(),
            "1".into(),
            "Test film 400/20".into()
        )
    );
    let parent: (String, String) = sqlx::query_as(
        "SELECT trim_scale(qty)::text,status FROM mini_raw_material_stock WHERE id='parent'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(parent, ("0".into(), "consumed".into()));
    let total: String =
        sqlx::query_scalar("SELECT trim_scale(sum(qty))::text FROM mini_raw_material_stock")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(total, "98");
    let events:(i64,String)=sqlx::query_as("SELECT count(*),trim_scale(sum(qty_delta))::text FROM mini_raw_material_events WHERE source_type='raw_material_split'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(events, (3, "-2".into()));
    let id = result["id"].as_str().unwrap();
    for _ in 0..3 {
        assert_eq!(store.saved("split-1", id).await.unwrap(), result);
    }
    assert!(store.saved("split-2", id).await.is_err());
    assert!(store.source("split-1", "parent").await.is_err());
    assert!(
        store
            .split(&actor, input("parent", "split-again-new-key"))
            .await
            .is_err()
    );
    let mut changed = request.clone();
    changed.outputs[0].width_mm = "399".into();
    assert!(matches!(
        store.split(&actor, changed).await,
        Err(SplitError::Conflict(_))
    ));
    assert!(
        sqlx::query(
            "UPDATE mini_raw_material_stock SET qty=100,status='available' WHERE id='parent'"
        )
        .execute(&pool)
        .await
        .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM mini_raw_material_splits WHERE id=$1")
            .bind(id)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE mini_raw_material_split_outputs SET kg=40 WHERE split_id=$1")
            .bind(id)
            .execute(&pool)
            .await
            .is_err()
    );

    // A saved child is a real usable shared-stock roll, and can be split again.
    let barcode = result["outputs"][0]["barcode"].as_str().unwrap();
    let child = store.source("split-1", barcode).await.unwrap();
    assert_eq!(child["kg"], "39");
    let mut next = input(barcode, "split-child-001");
    next.expected_revision = child["revision"].as_str().unwrap().into();
    next.expected_kg = "39".into();
    next.expected_width_mm = "400".into();
    next.waste_kg = "1".into();
    next.outputs = vec![SplitOutput {
        kg: "38".into(),
        width_mm: "355".into(),
        gross_kg: Some("39".into()),
        bobina_kg: Some("1".into()),
    }];
    store.split(&actor, next).await.unwrap();
    assert_eq!(store.saved("split-1", id).await.unwrap(), result); // historical 39, not current zero

    // Different devices with different keys can consume a parent only once.
    seed(&pool, "race", "Raw W", "FILM").await;
    let (a, b) = tokio::join!(
        store.split(&actor, input("race", "split-race-1")),
        store.split(&actor, input("race", "split-race-2"))
    );
    assert_eq!(a.is_ok() as u8 + b.is_ok() as u8, 1);
    seed(&pool, "metadata", "Raw W", "FILM").await;
    sqlx::query("UPDATE mini_raw_material_stock SET item_name='Changed',updated_at=now() WHERE id='metadata'")
        .execute(&pool).await.unwrap();
    assert!(matches!(
        store
            .split(&actor, input("metadata", "split-stale-metadata"))
            .await,
        Err(SplitError::Conflict(_))
    ));

    seed(&pool, "reserved", "Raw W", "FILM").await;
    sqlx::query("UPDATE mini_raw_material_stock SET status='reserved',reserved_order_id='order-test' WHERE id='reserved'")
        .execute(&pool).await.unwrap();
    assert!(
        store
            .split(&actor, input("reserved", "split-reserved"))
            .await
            .is_err()
    );
    seed(&pool, "transit", "Raw W", "FILM").await;
    sqlx::query("UPDATE mini_raw_material_stock SET payload_json=$1 WHERE id='transit'")
        .bind(json!({"inventory_transfer_id":"transfer-1"}))
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        store
            .split(&actor, input("transit", "split-transit"))
            .await
            .is_err()
    );
    seed(&pool, "outside", "Other W", "FILM").await;
    assert!(
        store
            .split(&actor, input("outside", "split-other-w"))
            .await
            .is_err()
    );
    seed(&pool, "rollback", "Raw W", "FAIL").await;
    sqlx::query("ALTER TABLE mini_raw_material_stock ADD CONSTRAINT test_split_failure CHECK (NOT (item_code='FAIL' AND barcode LIKE '30%'))")
        .execute(&pool).await.unwrap();
    assert!(
        store
            .split(&actor, input("rollback", "split-rollback"))
            .await
            .is_err()
    );
    let rolled_back:Value=sqlx::query_scalar("SELECT jsonb_build_object('kg',trim_scale(qty)::text,'status',status) FROM mini_raw_material_stock WHERE id='rollback'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(rolled_back, json!({"kg":"100","status":"available"}));
    let failed_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mini_raw_material_splits WHERE request_id='split-rollback'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(failed_count, 0);
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
