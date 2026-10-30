use super::*;

fn legacy_fixture() -> (
    ProductionMapDefinition,
    CalculateOrderTemplate,
    Vec<serde_json::Value>,
) {
    let map = serde_json::from_value(serde_json::json!({
        "id":"zakaz-0005", "order_number":"0005", "product_code":"magnus",
        "title":"Magnus", "order_kg":100, "width_mm":765, "roll_count":6,
    }))
    .unwrap();
    let template = CalculateOrderTemplate {
        id: "current".into(),
        order_number: "0005".into(),
        source_map_id: "template-zakaz-0005".into(),
        item_code: "magnus".into(),
        product: "Magnus".into(),
        kg: 100.0,
        frame_product_size_mm: 250.0,
        frame_count: 3.0,
        edge_allowance_mm: 15.0,
        width_mm: 765.0,
        roll_count: Some(6),
        waste_percent: 5.0,
        layers: vec![crate::core::formula::LayerInput::new("BOPP metal", "5")],
        ..Default::default()
    };
    let layers = vec![serde_json::to_value(template.effective_layers()).unwrap()];
    (map, template, layers)
}

#[test]
fn order_edit_legacy_source_explains_missing_and_ambiguous_calculations() {
    let (map, template, layers) = legacy_fixture();
    let missing = legacy_calculation(vec![], &map, &layers)
        .unwrap_err()
        .to_string();
    assert!(missing.contains("shablon topilmadi"));
    assert!(missing.contains("hozir kiritgan ma’lumotlaringizdagi xato emas"));
    assert!(missing.contains("administrator"));
    let value = serde_json::to_value(&template).unwrap();
    assert_eq!(
        legacy_calculation(vec![value.clone()], &map, &layers).unwrap(),
        template
    );
    let ambiguous = legacy_calculation(vec![value.clone(), value], &map, &layers)
        .unwrap_err()
        .to_string();
    assert!(ambiguous.contains("bir nechta Calculate shabloni"));
    assert!(ambiguous.contains("administrator"));
    assert_ne!(missing, ambiguous);
}

#[test]
fn order_edit_legacy_reused_source_link_selects_verified_order_not_old_template() {
    let (map, template, layers) = legacy_fixture();
    let mut old = template.clone();
    old.id = "old".into();
    old.order_number.clear();
    old.kg = 0.0;
    old.frame_product_size_mm = 400.0;
    old.width_mm = 1215.0;
    old.roll_count = Some(9);
    old.layers = vec![crate::core::formula::LayerInput::new("BOPP", "12")];
    let old = serde_json::to_value(old).unwrap();
    let current = serde_json::to_value(&template).unwrap();
    for values in [vec![old.clone(), current.clone()], vec![current, old]] {
        assert_eq!(legacy_calculation(values, &map, &layers).unwrap(), template);
    }
    // An equally shaped reusable template still does not outrank an exact order.
    let mut reusable = template.clone();
    reusable.order_number.clear();
    reusable.note = "changed reusable template".into();
    assert_eq!(
        legacy_calculation(
            vec![
                serde_json::to_value(reusable).unwrap(),
                serde_json::to_value(&template).unwrap()
            ],
            &map,
            &layers
        )
        .unwrap(),
        template
    );
}

#[test]
fn order_edit_legacy_rejects_incompatible_identity_dimensions_and_materials() {
    let (map, template, layers) = legacy_fixture();
    let mut variants = Vec::new();
    let mut wrong = template.clone();
    wrong.order_number = "9999".into();
    variants.push(wrong);
    let mut wrong = template.clone();
    wrong.item_code = "other".into();
    variants.push(wrong);
    let mut wrong = template.clone();
    wrong.frame_count = 4.0;
    variants.push(wrong);
    let mut wrong = template.clone();
    wrong.roll_count = Some(9);
    variants.push(wrong);
    let mut wrong = template.clone();
    wrong.kg = 200.0;
    variants.push(wrong);
    let mut wrong = template.clone();
    wrong.layers.clear();
    variants.push(wrong);
    let mut wrong = template.clone();
    wrong.print_val_size_mm = Some(1000.0);
    variants.push(wrong);
    for wrong in variants {
        assert!(matches!(
            legacy_calculation(vec![serde_json::to_value(wrong).unwrap()], &map, &layers),
            Err(Error::Locked(_))
        ));
    }
    let mut mismatched_exact = template.clone();
    mismatched_exact.roll_count = Some(9);
    let mut reusable = template.clone();
    reusable.order_number.clear();
    assert!(
        legacy_calculation(
            vec![
                serde_json::to_value(mismatched_exact).unwrap(),
                serde_json::to_value(reusable).unwrap()
            ],
            &map,
            &layers
        )
        .is_err()
    );
    let mut competing = template.clone();
    competing.waste_percent = 10.0;
    assert!(
        legacy_calculation(
            vec![
                serde_json::to_value(&template).unwrap(),
                serde_json::to_value(competing).unwrap()
            ],
            &map,
            &layers
        )
        .is_err()
    );
    assert!(legacy_calculation(vec![serde_json::to_value(template).unwrap()], &map, &[]).is_err());
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
    assert!(activity_reason("mini_raw_material_events").contains("Oddiy ulash/ajratishdan tashqari"));
    assert!(activity_reason("mini_opening_wip_intakes").contains("bekor qilingan"));
}
use crate::core::mini_orders::MiniOrderSink;
use crate::core::production_map::ProductionMapStorePort;
use crate::db::{
    postgres::{apply_foundation_migration, postgres_test_database_options},
    postgres_mini_order::PostgresMiniOrderSink,
    postgres_production_map::PostgresProductionMapStore,
};

async fn verify_legacy_order_creators(pool: &PgPool, runtime_pool: &PgPool) {
    sqlx::raw_sql(
        "INSERT INTO mini_auth_accounts(principal_ref) VALUES ('admin') ON CONFLICT DO NOTHING;
         INSERT INTO mini_auth_builtin_identities(principal_ref, phone, display_name)
           VALUES ('admin', '+998900000001', 'Legacy admin')
           ON CONFLICT (principal_ref) DO UPDATE SET display_name = excluded.display_name;
         INSERT INTO mini_system_users(id, role, name, phone) VALUES
           ('legacy-brigader', 'brigader', 'Legacy brigader', '+998900000002'),
           ('legacy-sales', 'sales_manager', 'Legacy sales manager', '+998900000003');",
    )
    .execute(pool)
    .await
    .unwrap();
    let sink = PostgresMiniOrderSink::new(runtime_pool.clone());
    for (role, ref_, name) in [
        ("admin", "admin", "Legacy admin"),
        ("brigader", "legacy-brigader", "Legacy brigader"),
        ("sales_manager", "legacy-sales", "Legacy sales manager"),
        ("brigader", "deleted-user", "deleted-user"),
    ] {
        sqlx::query("DELETE FROM mini_order_creation_receipts WHERE map_id='zakaz-1001'")
            .execute(pool)
            .await
            .unwrap();
        // More than one receipt from the same creator remains unambiguous.
        for key in ["original", "retry"] {
            sqlx::query("INSERT INTO mini_order_creation_receipts(owner_key,request_key,fingerprint,map_id,response_json) VALUES ($1,$2,'legacy','zakaz-1001','{}')")
                .bind(format!("{role}:{ref_}")).bind(key).execute(pool).await.unwrap();
        }
        assert_eq!(
            sink.order_creation_metadata("zakaz-1001")
                .await
                .unwrap()
                .created_by,
            Some(QueueActionActor {
                role: role.into(),
                ref_: ref_.into(),
                display_name: name.into()
            })
        );
        assert!(
            sink.order_creation_metadata("zakaz-1002")
                .await
                .unwrap()
                .created_by
                .is_none(),
            "receipts for another order must not be used"
        );
    }
    sqlx::query("INSERT INTO mini_order_creation_receipts(owner_key,request_key,fingerprint,map_id,response_json) VALUES ('admin:admin','conflict','legacy','zakaz-1001','{}')")
        .execute(pool).await.unwrap();
    assert!(
        sink.order_creation_metadata("zakaz-1001")
            .await
            .unwrap()
            .created_by
            .is_none(),
        "conflicting legacy owners must not be guessed"
    );
    sqlx::query("INSERT INTO mini_order_creation_receipts(owner_key,request_key,fingerprint,map_id,response_json) VALUES ('brigader:legacy-brigader','snapshot-priority','legacy','zakaz-2001','{}')")
        .execute(pool).await.unwrap();
    assert_eq!(
        sink.order_creation_metadata("zakaz-2001")
            .await
            .unwrap()
            .created_by,
        Some(QueueActionActor {
            role: "admin".into(),
            ref_: "admin".into(),
            display_name: "Original admin".into()
        }),
        "the creation-time snapshot must take priority over legacy records and renamed accounts"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM mini_production_order_creators WHERE map_id='zakaz-1001'"
        )
        .fetch_one(pool)
        .await
        .unwrap(),
        0,
        "legacy lookup must not write an inferred name into the snapshot"
    );
}

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
    crate::db::postgres::apply_postgres_migrations_through_version(&pool, "0121")
        .await.unwrap();

    use crate::core::apparatus_standard::{
        ApparatusId,
        service::CanonicalApparatusService,
        test_support::{TestApparatusSpec, canonical_draft},
    };
    let apparatus_service = CanonicalApparatusService::new(std::sync::Arc::new(
        crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository::new(
            pool.clone(),
        ),
    ));
    // The topology backfill requires the same factory apparatuses that an
    // existing installation already has before migration 0122 is applied.
    apparatus_service.bootstrap_factory_defaults().await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    for spec in [
        TestApparatusSpec::cut("apparatus:test:cut", "Cut"),
        TestApparatusSpec::laminate("apparatus:test:lam", "Laminate"),
    ] {
        apparatus_service
            .seed_for_test(
                ApparatusId::new(spec.apparatus_id).unwrap(),
                canonical_draft(&spec),
            )
            .await
            .unwrap();
    }
    let maps = PostgresProductionMapStore::new(pool.clone());
    let sink = PostgresMiniOrderSink::new(pool.clone());
    let template = CalculateOrderTemplate {
        name: "Order".into(),
        product: "Product".into(),
        item_code: "item".into(),
        kg: 500.0,
        frame_product_size_mm: 300.0,
        frame_count: 2.0,
        edge_allowance_mm: 15.0,
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
                {"id":"lam", "kind":"apparatus", "title":"Laminate", "apparatus_id":"apparatus:test:lam"},
                {"id":"end", "kind":"end", "title":"End"}],
            "edges": [{"from":"start","to":"cut"},{"from":"cut","to":"lam"},{"from":"lam","to":"end"}],
        })).unwrap();
        maps.put_map(map.clone()).await.unwrap();
        sink.save_order(&map, &template).await.unwrap();
    }
    // A downstream queue head must allow both form loading and saving while
    // the order remains behind another order at its initial physical stage.
    maps.put_apparatus_sequence(
        "apparatus:test:lam",
        vec!["zakaz-1002".into(), "zakaz-1001".into()],
    )
    .await
    .unwrap();
    // Exercise the real runtime ACLs, not the migration/owner connection.
    // Owner-only integration tests previously hid the history-lock failure.
    let runtime_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .after_connect(|connection, _| Box::pin(async move {
            sqlx::query("SET ROLE mini_rs_erp").execute(connection).await?;
            Ok(())
        }))
        .connect_with(postgres_test_database_options(&url, &db))
        .await.unwrap();
    assert_eq!(sqlx::query_scalar::<_, String>("SELECT current_user")
        .fetch_one(&runtime_pool).await.unwrap(), "mini_rs_erp");
    verify_runtime_history_locks(&runtime_pool).await;
    let sink = PostgresMiniOrderSink::new(runtime_pool.clone());
    let head = sink.order_edit_source("zakaz-1001").await;
    assert!(matches!(head, Err(Error::Locked(_))), "{head:?}");
    let source = sink.order_edit_source("zakaz-1002").await.unwrap();
    // Reproduce a reused template-zakaz link without touching live data. The
    // matching calculation is recoverable, but eligibility is still enforced.
    for number in ["1001", "1002"] {
        let mut tx = pool.begin().await.unwrap();
        let id = format!("zakaz-{number}");
        sqlx::query("UPDATE mini_orders SET calculation_json=NULL WHERE id=$1")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .unwrap();
        let mut current = source.template.clone();
        current.order_number = number.into();
        current.source_map_id = format!("template-{id}");
        let mut old = current.clone();
        old.order_number.clear();
        old.frame_product_size_mm = 400.0;
        old.layers = vec![crate::core::formula::LayerInput::new("BOPP", "12")];
        for (key, template) in [("old", old), ("current", current)] {
            sqlx::query("INSERT INTO mini_quick_order_templates(id,owner_key,code,name,item_code,product_name,payload_json,quick_key) VALUES($1,'admin',$1,'Order','item','Product',$2,$1)")
                .bind(key).bind(serde_json::to_value(template).unwrap())
                .execute(&mut *tx).await.unwrap();
        }
        let recovered = load_source(&mut tx, &id).await.unwrap();
        assert_eq!(recovered.template.frame_product_size_mm, 300.0);
        assert_eq!(recovered.template.order_number, number);
        let eligibility = check_eligible(&mut tx, &id).await;
        if number == "1001" {
            assert!(
                eligibility
                    .unwrap_err()
                    .to_string()
                    .contains("navbatida birinchi")
            );
        } else {
            eligibility.unwrap();
        }
        tx.rollback().await.unwrap();
    }
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

    // New ordinary and quick-clone orders must persist their private input in
    // the same transaction as their map. Rejected writes must leave no order.
    use crate::core::mini_orders::NewProductionOrder;
    let make_order = |number: &str, save_quick: bool| {
        let mut map = source.map.clone();
        map.id = format!("zakaz-{number}");
        map.code = number.into();
        map.order_number = number.into();
        let mut template_map = map.clone();
        template_map.id = format!("template-{}", map.id);
        template_map.order_number.clear();
        template_map.code.clear();
        template_map.order_kg = None;
        template_map.base_length = None;
        let mut template = source.template.clone();
        template.id = format!("quick-{number}");
        template.code = format!("Q-{number}");
        template.order_number = number.into();
        template.source_map_id = template_map.id.clone();
        NewProductionOrder {
            creation: None,
            map,
            template_map: save_quick.then_some(template_map),
            quick_template: save_quick.then_some(template.clone()),
            template,
            owner_key: "admin".into(),
            created_by: QueueActionActor {
                role: "admin".into(),
                ref_: "admin".into(),
                display_name: "Original admin".into(),
            },
        }
    };
    let create = make_order("2001", true);
    let quick = sink.create_order_atomic(&create).await.unwrap().unwrap();
    assert_eq!(quick.source_map_id, "template-zakaz-2001");
    let original_creation = sink.order_creation_metadata("zakaz-2001").await.unwrap();
    assert!(original_creation.created_at_unix.is_some());
    assert_eq!(
        original_creation.created_by,
        Some(create.created_by.clone())
    );
    // A later map save or a retried creator write must retain the original snapshot.
    let mut edited_creation_map = create.map.clone();
    edited_creation_map.title = "Renamed order".into();
    let mut tx = pool.begin().await.unwrap();
    crate::db::postgres_production_map::save_edited_map_tx(&mut tx, &edited_creation_map)
        .await
        .unwrap();
    crate::db::postgres_mini_order::save_order_creator_tx(
        &mut tx,
        "zakaz-2001",
        &QueueActionActor {
            role: "brigader".into(),
            ref_: "another-user".into(),
            display_name: "Another name".into(),
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let reread = PostgresMiniOrderSink::new(pool.clone())
        .order_creation_metadata("zakaz-2001")
        .await
        .unwrap();
    assert_eq!(reread.created_by, original_creation.created_by);
    assert_eq!(reread.created_at_unix, original_creation.created_at_unix);
    assert!(sqlx::query("UPDATE mini_production_order_creators SET display_name='Changed' WHERE map_id='zakaz-2001'")
        .execute(&runtime_pool).await.is_err(), "runtime must not rewrite creator snapshots");
    assert!(
        sink.order_creation_metadata("zakaz-1001")
            .await
            .unwrap()
            .created_by
            .is_none(),
        "legacy orders must not inherit a template author or current viewer"
    );
    verify_legacy_order_creators(&pool, &runtime_pool).await;
    let persisted: (String, String, i64) = sqlx::query_as(
        "SELECT o.calculation_json->>'source_map_id', o.calculation_json->>'order_number',
         (SELECT count(*) FROM mini_order_products WHERE order_id=o.id)
         FROM mini_orders o JOIN mini_production_maps m ON m.id=o.id WHERE o.id='zakaz-2001'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted, ("zakaz-2001".into(), "2001".into(), 1));
    assert!(matches!(
        sink.create_order_atomic(&create).await,
        Err(crate::core::production_map::ProductionMapError::DuplicateOrderNumber)
    ));
    // Force failure specifically when the private calculation is written,
    // after maps and quick-template writes have already happened inside the tx.
    sqlx::query("ALTER TABLE mini_orders ADD CONSTRAINT test_snapshot_failure CHECK (id <> 'zakaz-2002' OR calculation_json IS NULL)")
        .execute(&pool).await.unwrap();
    assert!(
        sink.create_order_atomic(&make_order("2002", true))
            .await
            .is_err()
    );
    let partial: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM mini_production_maps WHERE id IN ('zakaz-2002','template-zakaz-2002')),
         (SELECT count(*) FROM mini_orders WHERE id='zakaz-2002'),
         (SELECT count(*) FROM mini_quick_order_templates WHERE id='quick-2002'),
         (SELECT count(*) FROM mini_production_order_creators WHERE map_id='zakaz-2002')",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(partial, (0, 0, 0, 0));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_quick_order_templates")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        sink.create_order_atomic(&make_order("2003", false))
            .await
            .unwrap()
            .is_none()
    );
    let cloned: (bool, i64) = sqlx::query_as(
        "SELECT calculation_json IS NOT NULL, (SELECT count(*) FROM mini_quick_order_templates)
         FROM mini_orders WHERE id='zakaz-2003'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        cloned,
        (true, count),
        "quick clones do not modify reusable templates"
    );
    for (number, role) in [
        ("2101", "admin"),
        ("2102", "brigader"),
        ("2103", "sales_manager"),
    ] {
        let mut order = make_order(number, false);
        order.owner_key = format!("{role}:original-user");
        order.created_by = QueueActionActor {
            role: role.into(),
            ref_: "original-user".into(),
            display_name: format!("Original {role}"),
        };
        sink.create_order_atomic(&order).await.unwrap();
        assert_eq!(
            sink.order_creation_metadata(&order.map.id)
                .await
                .unwrap()
                .created_by,
            Some(order.created_by),
            "{role} creator must persist without a retry receipt or quick template"
        );
    }
    let mut material_order = make_order("3001", false);
    material_order.template.layers.push(crate::core::formula::LayerInput::new("PE", "40"));
    sink.create_order_atomic(&material_order).await.unwrap();
    maps.put_apparatus_sequence("apparatus:test:cut", vec!["zakaz-1001".into(), "zakaz-3001".into()]).await.unwrap();
    verify_material_edit_restrictions(&pool, &runtime_pool, &sink).await;
    verify_preflight_order_edits(&pool, &runtime_pool, &sink).await;
    runtime_pool.close().await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

async fn verify_material_edit_restrictions(
    pool: &PgPool,
    runtime_pool: &PgPool,
    sink: &PostgresMiniOrderSink,
) {
    let maps = PostgresProductionMapStore::new(runtime_pool.clone());
    let opened = sink.order_edit_source("zakaz-3001").await.unwrap();
    assert_eq!(opened.restrictions, OrderEditRestrictions::default());
    for (barcode, material) in [("EDIT-PET", "PET"), ("EDIT-PE", "PE")] {
        sqlx::query("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty) VALUES($1,'raw',$2,$2,$1,100)")
            .bind(barcode).bind(material).execute(pool).await.unwrap();
    }
    let assignment = |barcode: &str, material: &str| {
        serde_json::from_value::<crate::core::production_map::RawMaterialAssignment>(serde_json::json!({
            "order_id":"zakaz-3001", "apparatus_id":"apparatus:test:cut", "apparatus":"Cut",
            "barcode":barcode, "item_code":material, "item_name":material, "item_group":"Rulon",
            "assigned_by_role":"admin", "assigned_by_ref":"admin", "assigned_by_display_name":"Admin",
            "assigned_at":"2026-10-19T00:00:00Z"
        })).unwrap()
    };
    // Use the real assignment writer and its append-only reservation event.
    maps.put_raw_material_assignment(assignment("EDIT-PET", "PET")).await.unwrap();
    let current = sink.order_edit_source("zakaz-3001").await.unwrap();
    assert_eq!(current.restrictions.locked_layer_indices, vec![0]);
    let mut forbidden = opened.template.clone();
    forbidden.layers[0].micron = "35".into();
    assert!(matches!(sink.save_order_edit(&opened, &opened.map, &forbidden, &QueueActionActor::default()).await,
        Err(Error::Invalid(_))), "new attachment must be checked under save locks");
    let mut changed = current.template.clone();
    changed.layers[1].micron = "45".into();
    changed.kg = 600.0;
    let mut map = current.map.clone();
    map.order_kg = Some(600.0);
    let saved = sink.save_order_edit(&current, &map, &changed, &QueueActionActor::default()).await.unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.template.effective_layers()[1].micron, "45");
    assert_eq!(saved.restrictions.locked_layer_indices, vec![0]);
    let stock: (f64, String, String) = sqlx::query_as("SELECT qty::float8,status,reserved_order_id FROM mini_raw_material_stock WHERE barcode='EDIT-PET'")
        .fetch_one(pool).await.unwrap();
    assert_eq!(stock, (100.0, "available".into(), "".into()));
    maps.put_raw_material_assignment(assignment("EDIT-PE", "PE")).await.unwrap();
    changed.layers[1].micron = "50".into();
    assert!(matches!(sink.save_order_edit(&saved, &map, &changed, &QueueActionActor::default()).await,
        Err(Error::Invalid(_))));
    let both = sink.order_edit_source("zakaz-3001").await.unwrap();
    assert_eq!(both.revision, 1, "a rejected save must not mutate the order");
    assert_eq!(both.restrictions.locked_layer_indices, vec![0, 1]);
    for status in ["in_use", "consumed", "deleted", "reserved"] {
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("UPDATE mini_raw_material_stock SET status=$1 WHERE barcode='EDIT-PET'")
            .bind(status).execute(&mut *tx).await.unwrap();
        assert!(matches!(check_eligible(&mut tx, "zakaz-3001").await, Err(Error::Locked(_))), "{status}");
        tx.rollback().await.unwrap();
    }
    // Detaching unused rolls frees their fields; reservation history alone
    // remains permissible. Null/usage history is covered by the denied cases.
    for barcode in ["EDIT-PET", "EDIT-PE"] {
        maps.delete_raw_material_assignment("zakaz-3001", barcode).await.unwrap();
        sqlx::query("INSERT INTO mini_raw_material_events(event_id,idempotency_key,event_type,warehouse,barcode,item_code,order_id,actor_role,actor_ref,source_type,source_id,stock_status_before,stock_status_after) VALUES ($1,$1,'order_unreserved','raw',$2,$3,'zakaz-3001','admin','admin','order_assignment','zakaz-3001','available','available')")
            .bind(format!("unreserve-{barcode}")).bind(barcode).bind(barcode.trim_start_matches("EDIT-"))
            .execute(runtime_pool).await.unwrap();
    }
    let detached = sink.order_edit_source("zakaz-3001").await.unwrap();
    assert_eq!(detached.restrictions, OrderEditRestrictions::default());
}

async fn verify_preflight_order_edits(
    pool: &PgPool,
    runtime_pool: &PgPool,
    sink: &PostgresMiniOrderSink,
) {
    use crate::core::production_map::{PrintPreflightHold, PrintPreflightStatus as Status};
    const PRINT: &str = "apparatus:default:bosma_7";
    let maps = PostgresProductionMapStore::new(runtime_pool.clone());
    for (role_index, role) in ["admin", "brigader", "sales_manager"].iter().enumerate() {
        for (status_index, status) in [Status::Running, Status::Passed].into_iter().enumerate() {
            let number = (4100 + role_index * 2 + status_index).to_string();
            let id = format!("zakaz-{number}");
            let barcode = format!("PREFLIGHT-{number}");
            let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
                "id":id, "code":number, "order_number":number,
                "product_code":"item", "title":"Product", "order_kg":500,
                "width_mm":615, "roll_count":6,
                "nodes":[{"id":"start","kind":"start","title":"Start"},
                    {"id":"print","kind":"apparatus","title":"Print","apparatus_id":PRINT},
                    {"id":"end","kind":"end","title":"End"}],
                "edges":[{"from":"start","to":"print"},{"from":"print","to":"end"}]
            })).unwrap();
            let template = CalculateOrderTemplate {
                id: format!("calculation-{number}"), name: "Order".into(),
                product: "Product".into(), item_code: "item".into(),
                kg: 500.0, frame_product_size_mm: 300.0, frame_count: 2.0,
                edge_allowance_mm: 15.0, width_mm: 615.0, roll_count: Some(6),
                layers: vec![crate::core::formula::LayerInput::new("PET", "12")],
                ..Default::default()
            };
            maps.put_map(map.clone()).await.unwrap();
            sink.save_order(&map, &template).await.unwrap();
            maps.put_apparatus_sequence(PRINT, vec![id.clone()]).await.unwrap();
            assert!(matches!(sink.order_edit_source(&id).await, Err(Error::Locked(_))),
                "ordinary pending queue heads retain their existing policy");
            sqlx::query("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty) VALUES($1,'raw','PET','PET',$1,100)")
                .bind(&barcode).execute(pool).await.unwrap();
            maps.put_raw_material_assignment(serde_json::from_value(serde_json::json!({
                "order_id":id, "apparatus_id":PRINT, "apparatus":"Print",
                "barcode":barcode, "item_code":"PET", "item_name":"PET", "item_group":"Rulon",
                "assigned_by_role":"admin", "assigned_by_ref":"admin", "assigned_by_display_name":"Admin",
                "assigned_at":"2026-10-30T00:00:00Z"
            })).unwrap()).await.unwrap();
            let actor = QueueActionActor {
                role: (*role).into(), ref_: "editor".into(), display_name: "Editor".into(),
            };
            let mut hold = PrintPreflightHold {
                hold_id: format!("preflight-{number}"), idempotency_key: format!("preflight-{number}"),
                order_id: id.clone(), apparatus: PRINT.into(), stage_node_id: "print".into(),
                status: Status::Running, revision: 0, actor: QueueActionActor::default(),
                created_at_unix: 1, updated_at_unix: 1, expires_at_unix: 0, previous_queue_state: None,
            };
            hold.actor = actor.clone();
            maps.put_print_preflight_hold(hold.clone()).await.unwrap();
            if status == Status::Passed {
                hold.status = status;
                hold.revision += 1;
                maps.update_print_preflight_hold(hold.clone()).await.unwrap();
            }
            let source = sink.order_edit_source(&id).await.unwrap();
            assert_eq!(source.restrictions.locked_layer_indices, vec![0]);
            let mut edited = source.template.clone();
            edited.kg = 600.0;
            let mut edited_map = source.map.clone();
            edited_map.order_kg = Some(600.0);
            let saved = sink.save_order_edit(&source, &edited_map, &edited, &actor).await.unwrap();
            assert_eq!(saved.revision, 1, "{role} / {status:?}");
            assert_eq!(maps.print_preflight_hold_by_id(&hold.hold_id).await.unwrap(), Some(hold.clone()),
                "quantity edits must preserve the trial and its matched result");

            // A colour trial never exempts actual work at this or another
            // stage, historical sessions, invalid holds, or unavailable stock.
            for sql in [
                "UPDATE mini_queue_states SET state='in_progress' WHERE order_id=$1",
                "INSERT INTO mini_queue_states(apparatus,canonical_apparatus_id,order_id,state) VALUES ('Cut','apparatus:test:cut',$1,'in_progress')",
                "INSERT INTO mini_order_run_sessions(session_id,apparatus,canonical_apparatus_id,stage_node_id,order_id,status) VALUES ($1,'Print','apparatus:default:bosma_7','print',$1,'completed')",
                "UPDATE mini_print_preflight_holds SET status='cancelled' WHERE order_id=$1",
                "UPDATE mini_raw_material_stock SET status='in_use' WHERE id='PREFLIGHT-' || right($1,4)",
                "UPDATE mini_raw_material_stock SET status='consumed',qty=0 WHERE id='PREFLIGHT-' || right($1,4)",
            ] {
                let mut tx = pool.begin().await.unwrap();
                sqlx::query(sql).bind(&id).execute(&mut *tx).await.unwrap();
                assert!(matches!(check_eligible(&mut tx, &id).await, Err(Error::Locked(_))), "{sql}");
                tx.rollback().await.unwrap();
            }
            let mut invalid_material = saved.template.clone();
            invalid_material.layers[0].micron = "15".into();
            assert!(matches!(sink.save_order_edit(&saved, &saved.map, &invalid_material, &actor).await,
                Err(Error::Invalid(_))), "attached material restrictions still apply");

            // A changed print specification invalidates both old approval and
            // a decision being submitted from an already open worker sheet.
            let mut changed = saved.template.clone();
            changed.roll_count = Some(7);
            let mut changed_map = saved.map.clone();
            changed_map.roll_count = Some(7);
            let changed = sink.save_order_edit(&saved, &changed_map, &changed, &actor).await.unwrap();
            let refreshed = maps.print_preflight_hold_by_id(&hold.hold_id).await.unwrap().unwrap();
            assert_eq!(refreshed.status, Status::Running);
            assert_eq!(refreshed.revision, hold.revision + 1);
            let mut stale_pass = hold.clone();
            stale_pass.status = Status::Passed;
            stale_pass.revision += 1;
            assert!(matches!(maps.update_print_preflight_hold(stale_pass).await,
                Err(crate::core::production_map::ProductionMapError::PrintPreflightNotReady)));
            assert!(matches!(maps.consume_print_preflight_hold(&hold.hold_id, &id, PRINT, &actor).await,
                Err(crate::core::production_map::ProductionMapError::PrintPreflightNotReady)));
            let (quantity, stock_status, reserved): (f64, String, String) = sqlx::query_as(
                "SELECT qty::float8,status,reserved_order_id FROM mini_raw_material_stock WHERE id=$1")
                .bind(&barcode).fetch_one(pool).await.unwrap();
            assert_eq!((quantity, stock_status, reserved), (100.0, "available".into(), "".into()));
            let (state, sequence): (String, serde_json::Value) = sqlx::query_as(
                "SELECT q.state,s.order_ids FROM mini_queue_states q JOIN mini_queue_sequences s USING(canonical_apparatus_id) WHERE q.order_id=$1")
                .bind(&id).fetch_one(pool).await.unwrap();
            assert_eq!(state, "print_preflight");
            assert_eq!(sequence, serde_json::json!([id]));

            // Work or consumption committed after the form opened must also
            // reject saving, with no calculation revision or hold changes.
            if status == Status::Passed {
                sqlx::query("INSERT INTO mini_order_run_sessions(session_id,apparatus,canonical_apparatus_id,stage_node_id,order_id,status) VALUES ($1,'Print','apparatus:default:bosma_7','print',$1,'active')")
                    .bind(&id).execute(pool).await.unwrap();
            } else {
                sqlx::query("UPDATE mini_raw_material_stock SET status='consumed',qty=0 WHERE id=$1")
                    .bind(&barcode).execute(pool).await.unwrap();
            }
            assert!(matches!(sink.order_edit_source(&id).await, Err(Error::Locked(_))));
            assert!(matches!(sink.save_order_edit(&changed, &changed.map, &changed.template, &actor).await,
                Err(Error::Locked(_))));
            let revision: i64 = sqlx::query_scalar("SELECT calculation_revision FROM mini_orders WHERE id=$1")
                .bind(&id).fetch_one(pool).await.unwrap();
            assert_eq!(revision, 2);
            assert_eq!(maps.print_preflight_hold_by_id(&hold.hold_id).await.unwrap(), Some(refreshed.clone()));
            let mut cancelled = refreshed;
            cancelled.status = Status::Cancelled;
            cancelled.revision += 1;
            maps.update_print_preflight_hold(cancelled).await.unwrap();
        }
    }
}

async fn verify_runtime_history_locks(pool: &PgPool) {
    for table in ["mini_raw_material_events", "mini_preparation_operations"] {
        assert!(!sqlx::query_scalar::<_, bool>(
            "SELECT has_table_privilege(current_user, $1, 'UPDATE,DELETE,TRUNCATE')")
            .bind(table).fetch_one(pool).await.unwrap());
        let mut rejected = pool.begin().await.unwrap();
        let error = sqlx::query(&format!("LOCK TABLE public.{table} IN SHARE ROW EXCLUSIVE MODE"))
            .execute(&mut *rejected).await.unwrap_err();
        let explained = Error::from(error);
        assert!(matches!(&explained, Error::Storage { code: "order_edit_database_permission", .. }));
        assert!(explained.to_string().contains("baza ruxsatlari"));
        rejected.rollback().await.unwrap();

        let mut edit = pool.begin().await.unwrap();
        sqlx::query("SELECT public.mini_lock_order_edit_history($1)")
            .bind(table).execute(&mut *edit).await.unwrap();
        // INSERT takes ROW EXCLUSIVE: history cannot appear after eligibility
        // is checked and before the edit commits, even from another process.
        let mut writer = pool.begin().await.unwrap();
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *writer).await.unwrap();
        let error = sqlx::query(&format!("LOCK TABLE public.{table} IN ROW EXCLUSIVE MODE"))
            .execute(&mut *writer).await.unwrap_err();
        assert!(matches!(Error::from(error), Error::Storage { code: "order_edit_database_busy", .. }));
        writer.rollback().await.unwrap();
        edit.rollback().await.unwrap();
        let mut writer = pool.begin().await.unwrap();
        sqlx::query(&format!("LOCK TABLE public.{table} IN ROW EXCLUSIVE MODE NOWAIT"))
            .execute(&mut *writer).await.unwrap();
        writer.rollback().await.unwrap();
    }
    let error = sqlx::query("SELECT public.mini_lock_order_edit_history('mini_orders')")
        .execute(pool).await.unwrap_err();
    assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("22023"));
    let public_execute: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pg_proc p, aclexplode(p.proacl) a
         WHERE p.oid = 'public.mini_lock_order_edit_history(text)'::regprocedure
           AND a.grantee = 0 AND a.privilege_type = 'EXECUTE')")
        .fetch_one(pool).await.unwrap();
    assert!(!public_execute);
}
