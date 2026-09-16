use super::*;

#[test]
fn order_edit_legacy_source_explains_missing_and_ambiguous_calculations() {
    let missing = legacy_calculation(vec![]).unwrap_err().to_string();
    assert!(missing.contains("shablon topilmadi"));
    assert!(missing.contains("hozir kiritgan ma’lumotlaringizdagi xato emas"));
    assert!(missing.contains("administrator"));
    let value = serde_json::json!({"id": "original"});
    assert_eq!(legacy_calculation(vec![value.clone()]).unwrap(), value);
    let ambiguous = legacy_calculation(vec![value.clone(), value])
        .unwrap_err()
        .to_string();
    assert!(ambiguous.contains("bir nechta shablon"));
    assert!(ambiguous.contains("administrator"));
    assert_ne!(missing, ambiguous);
}

#[test]
fn order_edit_activity_reasons_identify_every_guarded_history() {
    for table in ACTIVITY_TABLES {
        assert_ne!(
            activity_reason(table),
            activity_reason("unknown"),
            "{table}"
        );
    }
    for (table, reason) in [
        ("mini_order_run_sessions", "ish sessiyasi"),
        ("mini_raw_material_events", "xomashyo"),
        ("mini_opening_wip_intakes", "opening WIP"),
        ("mini_apparatus_schedule_reservations", "vaqti band"),
        ("mini_preparation_operations", "tayyorlov"),
    ] {
        assert!(activity_reason(table).contains(reason), "{table}");
    }
    assert!(activity_reason("mini_raw_material_events").contains("keyin ajratilgan"));
    assert!(activity_reason("mini_opening_wip_intakes").contains("bekor qilingan"));
}
use crate::core::mini_orders::MiniOrderSink;
use crate::core::production_map::ProductionMapStorePort;
use crate::db::{
    postgres::{apply_foundation_migration, postgres_test_database_options},
    postgres_mini_order::PostgresMiniOrderSink,
    postgres_production_map::PostgresProductionMapStore,
};

#[tokio::test]
#[ignore = "creates an isolated PostgreSQL database; requires MINI_ERP_TEST_ADMIN_DATABASE_URL"]
async fn order_edit_postgres_atomic_save_history_and_races() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .expect("explicit local test database URL");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db = format!("order_edit_test_{}_{nonce}", std::process::id());
    let admin = PgPool::connect(&url).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    eprintln!("isolated order-edit test database: {db}");
    let pool = PgPool::connect_with(postgres_test_database_options(&url, &db))
        .await
        .unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();

    use crate::core::apparatus_standard::{
        ApparatusId,
        service::CanonicalApparatusService,
        test_support::{TestApparatusSpec, canonical_draft},
    };
    let spec = TestApparatusSpec::cut("apparatus:test:cut", "Cut");
    CanonicalApparatusService::new(std::sync::Arc::new(
        crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository::new(
            pool.clone(),
        ),
    ))
    .seed_for_test(
        ApparatusId::new(spec.apparatus_id).unwrap(),
        canonical_draft(&spec),
    )
    .await
    .unwrap();
    let maps = PostgresProductionMapStore::new(pool.clone());
    let sink = PostgresMiniOrderSink::new(pool.clone());
    let template = CalculateOrderTemplate {
        name: "Order".into(),
        product: "Product".into(),
        item_code: "item".into(),
        kg: 500.0,
        frame_product_size_mm: 300.0,
        frame_count: 2.0,
        width_mm: 615.0,
        roll_count: Some(6),
        layers: vec![crate::core::formula::LayerInput::new("pet", "12")],
        ..Default::default()
    };
    for number in ["1001", "1002"] {
        let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
            "id": format!("zakaz-{number}"), "product_code": "item", "title": "Product", "code": number,
            "order_number": number, "order_kg": 500, "width_mm": 615, "roll_count": 6,
            "nodes": [{"id":"start", "kind":"start", "title":"Start"},
                {"id":"cut", "kind":"apparatus", "title":"Cut", "apparatus_id":"apparatus:test:cut"},
                {"id":"end", "kind":"end", "title":"End"}],
            "edges": [{"from":"start","to":"cut"},{"from":"cut","to":"end"}],
        })).unwrap();
        maps.put_map(map.clone()).await.unwrap();
        sink.save_order(&map, &template).await.unwrap();
    }
    let head = sink.order_edit_source("zakaz-1001").await;
    assert!(matches!(head, Err(Error::Locked(_))), "{head:?}");
    let source = sink.order_edit_source("zakaz-1002").await.unwrap();
    let timestamp: String = sqlx::query_scalar(
        "SELECT updated_at::text FROM mini_production_maps WHERE id='zakaz-1002'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut edited = source.template.clone();
    edited.kg = 600.0;
    edited.note = "Only this order".into();
    let mut edited_map = source.map.clone();
    edited_map.order_kg = Some(600.0);
    let saved = sink
        .save_order_edit(&source, &edited_map, &edited, &QueueActionActor::default())
        .await
        .unwrap();
    assert_eq!(saved.revision, 1);
    let new_timestamp: String = sqlx::query_scalar(
        "SELECT updated_at::text FROM mini_production_maps WHERE id='zakaz-1002'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        timestamp, new_timestamp,
        "implicit queue position must not change"
    );
    assert!(matches!(
        sink.save_order_edit(&source, &edited_map, &edited, &QueueActionActor::default())
            .await,
        Err(Error::Conflict)
    ));
    // Reconciliation with an old/shared template must retain the edit.
    sink.sync_orders(&[edited_map.clone()], &[source.template.clone()])
        .await
        .unwrap();
    let (kg, note, revision, logs): (String, String, i64, i32) = sqlx::query_as(
        "SELECT o.kg::text, p.note, o.calculation_revision, jsonb_array_length(o.calculation_edit_log)
         FROM mini_orders o JOIN mini_order_products p ON p.order_id=o.id WHERE o.id='zakaz-1002'",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(kg, "600.000000");
    assert_eq!(note, edited.note);
    assert_eq!(revision, 1);
    assert_eq!(logs, 1);
    let counts: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM mini_orders), (SELECT count(*) FROM mini_quick_order_templates)").fetch_one(&pool).await.unwrap();
    assert_eq!(counts, (2, 0), "no new order or quick template");

    // The save-time check must reject a form opened before the order moved to head.
    maps.put_apparatus_sequence(
        "apparatus:test:cut",
        vec!["zakaz-1002".into(), "zakaz-1001".into()],
    )
    .await
    .unwrap();
    let denied = sink
        .save_order_edit(&saved, &edited_map, &edited, &QueueActionActor::default())
        .await;
    assert!(matches!(denied, Err(Error::Locked(_))), "{denied:?}");
    maps.put_apparatus_sequence(
        "apparatus:test:cut",
        vec!["zakaz-1001".into(), "zakaz-1002".into()],
    )
    .await
    .unwrap();

    // Every denied case is rolled back in this isolated database.
    for sql in [
        "INSERT INTO mini_opening_wip_intakes(intake_id,idempotency_key,request_fingerprint,order_id,entry_apparatus,source_operation,current_location,status,resume_apparatus,resume_stage_node_id) VALUES ('wip','wip','wip','zakaz-1002','apparatus:test:cut','cut','factory','cancelled','apparatus:test:cut','cut')",
        "INSERT INTO mini_raw_material_events(event_id,idempotency_key,event_type,warehouse,barcode,item_code,order_id,actor_role,actor_ref,source_type,source_id) VALUES ('raw','raw','order_unreserved','raw','barcode','item','zakaz-1002','admin','admin','order_assignment','test')",
        "INSERT INTO mini_order_run_sessions(session_id,apparatus,canonical_apparatus_id,stage_node_id,order_id,status) VALUES ('old-session','Cut','apparatus:test:cut','cut','zakaz-1002','completed')",
        "INSERT INTO mini_order_control_states(order_id,state) VALUES ('zakaz-1002','active')",
        "INSERT INTO mini_queue_states(apparatus,canonical_apparatus_id,order_id,state) VALUES ('Cut','apparatus:test:cut','zakaz-1002','paused')",
        "UPDATE mini_production_maps SET lifecycle_status='in_progress', lifecycle_version=1 WHERE id='zakaz-1002'",
    ] {
        let mut tx = pool.begin().await.unwrap();
        sqlx::query(sql).execute(&mut *tx).await.unwrap();
        assert!(
            matches!(
                check_eligible(&mut tx, "zakaz-1002").await,
                Err(Error::Locked(_))
            ),
            "{sql}"
        );
        tx.rollback().await.unwrap();
    }

    // A material event occurring after form load is checked again during save.
    sqlx::query("INSERT INTO mini_raw_material_events(event_id,idempotency_key,event_type,warehouse,barcode,item_code,order_id,actor_role,actor_ref,source_type,source_id) VALUES ('race','race','order_reserved','raw','barcode','item','zakaz-1002','admin','admin','order_assignment','test')").execute(&pool).await.unwrap();
    assert!(matches!(
        sink.save_order_edit(&saved, &edited_map, &edited, &QueueActionActor::default())
            .await,
        Err(Error::Locked(_))
    ));
    let revision: i64 =
        sqlx::query_scalar("SELECT calculation_revision FROM mini_orders WHERE id='zakaz-1002'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(revision, 1, "rejected save must not mutate the order");
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
