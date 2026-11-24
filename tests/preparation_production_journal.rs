use std::sync::Arc;

use mini_rs_erp::core::auth::models::{Principal, PrincipalRole};
use mini_rs_erp::core::preparation::{
    PreparationError, PreparationNoteViewerRole, PreparationOutputNoteUpdate,
    PreparationProductionNoteUpdate,
};
use mini_rs_erp::core::rps_batch::models::{RpsBatchPrintEntry, RpsBatchSession};
use mini_rs_erp::core::rps_batch::ports::RpsBatchStorePort;
use mini_rs_erp::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    canonical_apparatus_service,
};
use mini_rs_erp::db::postgres_preparation::PostgresPreparationStore;
use mini_rs_erp::db::postgres_rps_batch::PostgresRpsBatchStore;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgConnectOptions};

#[tokio::test]
#[ignore = "requires maintenance PostgreSQL access; creates an isolated database"]
async fn journal_groups_confirmed_rolls_by_order_preserves_recipe_and_scope_and_pages() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".into());
    let admin = PgPool::connect(&url).await.unwrap();
    let database = format!(
        "mini_test_production_journal_{:016x}",
        rand::random::<u64>()
    );
    sqlx::query(&format!("CREATE DATABASE {database}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(url.parse::<PgConnectOptions>().unwrap().database(&database))
        .await
        .unwrap();
    let checks = pool.clone();
    let result = tokio::task::LocalSet::new()
        .run_until(async move {
            tokio::task::spawn_local(async move { run_checks(checks).await }).await
        })
        .await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

async fn run_checks(pool: PgPool) {
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    canonical_apparatus_service(pool.clone())
        .bootstrap_factory_defaults()
        .await
        .unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT to_regclass('public.idx_mini_gscale_receipts_preparation_owner') IS NOT NULL"
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    );
    sqlx::raw_sql("INSERT INTO mini_orders(id,code,order_number,customer_name,product_name) VALUES
        ('order-a','0317','0317','Silver choy','Tea packaging'),('order-b','0318','0318','Customer B','Other packaging');")
        .execute(&pool).await.unwrap();
    let recipe = json!({"schema_version":1,"variant_id":"variant-lg","variant_name":"LG","material_name":"PE PR",
        "formulas":[{"name":"A","share_percent":"100","lines":[{"item_code":"S1","name":"Seriya 1","percent":"88"},{"item_code":"S2","name":"Seriya 2","percent":"12"}]}]});
    let store = Arc::new(PostgresRpsBatchStore::new(pool.clone()));
    let mut batch = RpsBatchSession {
        id: "batch-a".into(),
        owner_key: "tayyorlov_masteri:master-1".into(),
        owner_role: "tayyorlov_masteri".into(),
        owner_ref: "master-1".into(),
        item_code: "PE PR".into(),
        item_name: "PE PR".into(),
        warehouse: "Own W".into(),
        active: false,
        order_assignment: Some(json!({"order_id":"order-a","preparation_formula":recipe})),
        created_at: "2026-10-28T05:00:00Z".into(),
        updated_at: "2026-10-28T05:10:00Z".into(),
        prints: vec![print("EPC1"), print("EPC1"), print("EPC2")],
        ..Default::default()
    };
    store.complete(&batch).await.unwrap(); // persists both current and history
    let duplicated = PostgresPreparationStore::new(pool.clone())
        .production_journal_orders("master-1", "", "all", 0, 50)
        .await
        .unwrap();
    assert_eq!(
        duplicated["orders"][0]["batch_count"], 1,
        "the same completed session in current and history counts once"
    );
    batch.id = "batch-live".into();
    batch.active = true;
    batch.prints.clear();
    batch.updated_at = "2026-10-29T05:00:00Z".into();
    store.put(&batch).await.unwrap();
    for (epc, order, qty, status, owner) in [
        ("EPC1", "order-a", 99, "submitted", "master-1"),
        ("EPC2", "order-a", 20, "submitted", "master-1"),
        ("EPC3", "order-a", 5, "submitted", "master-1"),
        ("DRAFT", "order-a", 200, "draft", "master-1"),
        ("OTHER", "order-a", 300, "submitted", "master-2"),
        ("EPC-B", "order-b", 7, "submitted", "master-1"),
    ] {
        let payload = json!({"actor_role":"tayyorlov_masteri","actor_ref":owner,"item_name":"PE PR",
            "order_assignment":{"order_id":order,"preparation_formula":if epc=="EPC1" {recipe.clone()} else {Value::Null}}});
        sqlx::query("INSERT INTO mini_gscale_receipts(name,status,item_code,warehouse,qty,barcode,payload_json,length_m,width_mm,micron,created_at,submitted_at)
            VALUES($1,$2,'PE PR','Own W',$3,$4,$5,125,495,50,'2026-10-28T05:05:00Z','2026-10-28T05:05:00Z')")
            .bind(format!("receipt-{epc}")).bind(status).bind(qty as f64).bind(epc).bind(payload).execute(&pool).await.unwrap();
    }
    // Original production event remains authoritative after a receipt correction.
    sqlx::query("INSERT INTO mini_raw_material_events(event_id,idempotency_key,event_type,warehouse,barcode,item_code,item_name,
        qty_delta,order_id,actor_role,actor_ref,source_type,source_id,payload_json,occurred_at)
        VALUES('journal-event','journal-event','receipt_posted','Original W','EPC1','PE PR','Original PE PR',10,
            'order-a','tayyorlov_masteri','master-1','gscale_receipt','receipt-EPC1',$1,'2026-10-28T05:05:00Z')")
        .bind(json!({"length_m":100,"width_mm":490,"micron":40,"preparation_formula":recipe})).execute(&pool).await.unwrap();
    let journal = PostgresPreparationStore::new(pool.clone());
    let list = journal
        .production_journal_orders("master-1", "", "all", 0, 1)
        .await
        .unwrap();
    assert_eq!(list["orders"][0]["order_id"], "order-a");
    assert_eq!(list["orders"][0]["roll_count"], 3);
    assert_eq!(list["orders"][0]["batch_count"], 3); // completed, active-empty, legacy receipt
    assert_eq!(list["orders"][0]["active_batch_count"], 1);
    assert_eq!(number(&list["orders"][0]["total_net_kg"]), 35.0);
    assert_eq!(number(&list["orders"][0]["total_length_m"]), 350.0);
    assert_eq!(list["has_more"], true);
    assert_eq!(list["next_offset"], 1);
    let next = journal
        .production_journal_orders("master-1", "", "all", 1, 1)
        .await
        .unwrap();
    assert_eq!(next["orders"][0]["order_id"], "order-b");
    assert_eq!(next["has_more"], false);
    for search in ["0317", "Tea", "Silver", "PE PR", "LG"] {
        assert_eq!(
            journal
                .production_journal_orders("master-1", search, "all", 0, 50)
                .await
                .unwrap()["orders"][0]["order_id"],
            "order-a"
        );
    }
    assert_eq!(
        journal
            .production_journal_orders("master-1", "", "active", 0, 50)
            .await
            .unwrap()["orders"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        journal
            .production_journal_orders("master-1", "", "completed", 0, 50)
            .await
            .unwrap()["orders"][0]["order_id"],
        "order-b"
    );
    let detail = journal
        .production_journal_order("master-1", "order-a", 0, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail["batches"][0]["batch_id"], "batch:batch-live");
    assert_eq!(detail["batches"][0]["roll_count"], 0);
    assert_eq!(detail["has_more"], true);
    let rolls = journal
        .production_journal_batch("master-1", "batch:batch-a", 0, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rolls["batch"]["roll_count"], 2);
    assert_eq!(rolls["has_more"], true);
    let all = journal
        .production_journal_batch("master-1", "batch:batch-a", 0, 50)
        .await
        .unwrap()
        .unwrap();
    let original = all["rolls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|roll| roll["epc"] == "EPC1")
        .unwrap();
    assert_eq!(number(&original["net_kg"]), 10.0);
    assert_eq!(number(&original["width_mm"]), 490.0);
    assert_eq!(original["preparation_formula"], recipe);
    assert!(
        journal
            .production_journal_batch("master-2", "batch:batch-a", 0, 50)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        journal
            .production_journal_order("master-3", "order-a", 0, 50)
            .await
            .unwrap()
            .is_none()
    );
    let legacy = journal
        .production_journal_batch("master-1", "receipt:EPC3", 0, 50)
        .await
        .unwrap()
        .unwrap();
    assert!(legacy["batch"]["preparation_formula"].is_null());
    assert_eq!(legacy["rolls"][0]["epc"], "EPC3");
    // No current warehouse or formula assignments exist in this fixture: historical ownership suffices.
    assert_eq!(
        journal
            .production_journal_orders("master-2", "", "all", 0, 50)
            .await
            .unwrap()["orders"][0]["roll_count"],
        1
    );
    check_production_notes(&pool, &journal, &recipe).await;
}

async fn check_production_notes(pool: &PgPool, journal: &PostgresPreparationStore, recipe: &Value) {
    let actor = |owner: &str| Principal {
        role: PrincipalRole::TayyorlovMasteri,
        ref_: owner.into(),
        display_name: "Master".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let update = |order: &str, text: &str, revision| PreparationProductionNoteUpdate {
        order_id: order.into(),
        text: text.into(),
        expected_revision: revision,
    };
    assert_eq!(
        journal
            .production_journal_order("master-1", "order-a", 0, 30)
            .await
            .unwrap()
            .unwrap()["order"]["preparation_note"]["revision"],
        0
    );
    assert!(
        journal
            .update_production_order_note(&actor("master-3"), &update("order-a", "denied", 0))
            .await
            .unwrap()
            .is_none()
    );
    let saved = journal
        .update_production_order_note(
            &actor("master-1"),
            &update("order-a", "  Sovuq joyda saqlansin\nO‘zbekcha 🫖  ", 0),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved["text"], "Sovuq joyda saqlansin\nO‘zbekcha 🫖");
    assert_eq!(saved["revision"], 1);
    assert_eq!(saved["updated_by_ref"], "master-1");
    assert!(saved["updated_at"].is_string());
    // A new store and a second producer see the same persisted order-level note.
    let fresh = PostgresPreparationStore::new(pool.clone());
    let detail = fresh
        .production_journal_order("master-2", "order-a", 0, 30)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail["order"]["preparation_note"], saved);
    assert!(
        detail["batches"]
            .as_array()
            .unwrap()
            .iter()
            .all(|batch| batch["preparation_note"] == saved)
    );
    let batch = fresh
        .production_journal_batch("master-1", "batch:batch-a", 0, 50)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(batch["batch"]["preparation_note"], saved);
    assert!(
        batch["rolls"]
            .as_array()
            .unwrap()
            .iter()
            .all(|roll| roll["preparation_note"] == saved)
    );
    assert_eq!(batch["rolls"][0]["preparation_formula"], *recipe);
    let legacy = fresh
        .production_journal_batch("master-1", "receipt:EPC3", 0, 50)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(legacy["rolls"][0]["preparation_note"], saved);
    assert_eq!(
        fresh
            .production_journal_order("master-1", "order-b", 0, 30)
            .await
            .unwrap()
            .unwrap()["order"]["preparation_note"]["text"],
        ""
    );

    // The immutable production event wins over a later receipt/order correction.
    sqlx::raw_sql("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,uom,status,source_receipt_id)
        VALUES ('note-stock','Own W','PE PR','PE PR','EPC1',10,'kg','available','receipt-EPC1');
        UPDATE mini_gscale_receipts SET payload_json=jsonb_set(payload_json,'{order_assignment,order_id}','\"order-b\"') WHERE name='receipt-EPC1';
        INSERT INTO mini_gscale_receipts(name,status,item_code,warehouse,qty,barcode,payload_json)
        VALUES('receipt-FUTURE','submitted','PE PR','Own W',1,'FUTURE',
            '{\"actor_role\":\"tayyorlov_masteri\",\"actor_ref\":\"master-1\",\"order_assignment\":{\"order_id\":\"order-a\"}}');")
        .execute(pool).await.unwrap();
    let notes = fresh
        .production_notes_for_receipts(&["EPC1".into(), "MISSING".into()])
        .await
        .unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes["EPC1"], saved);
    assert_eq!(
        fresh
            .production_journal_batch("master-1", "receipt:FUTURE", 0, 50)
            .await
            .unwrap()
            .unwrap()["rolls"][0]["preparation_note"],
        saved
    );

    let input = update("order-a", "Concurrent", 1);
    let a = actor("master-1");
    let b = actor("master-2");
    let (left, right) = tokio::join!(
        journal.update_production_order_note(&a, &input),
        fresh.update_production_order_note(&b, &input)
    );
    assert!(matches!(
        (&left, &right),
        (Ok(_), Err(PreparationError::Conflict(_))) | (Err(PreparationError::Conflict(_)), Ok(_))
    ));
    let cleared = fresh
        .update_production_order_note(&a, &update("order-a", "   ", 2))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cleared["text"], "");
    assert_eq!(cleared["revision"], 3);
    let long = "🫖".repeat(2000);
    assert!(
        fresh
            .update_production_order_note(&a, &update("order-a", &long, 3))
            .await
            .is_ok()
    );
    assert!(matches!(
        fresh
            .update_production_order_note(&a, &update("order-a", &"a".repeat(2001), 4))
            .await,
        Err(PreparationError::Invalid(_))
    ));
    let cleared = fresh
        .update_production_order_note(&a, &update("order-a", "", 4))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fresh
            .production_notes_for_receipts(&["epc1".into()])
            .await
            .unwrap()["EPC1"],
        cleared
    );

    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(pool)
        .await
        .unwrap();
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").unwrap();
    let runtime = sqlx::postgres::PgPoolOptions::new()
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE mini_rs_erp")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(url.parse::<PgConnectOptions>().unwrap().database(&database))
        .await
        .unwrap();
    let restricted = PostgresPreparationStore::new(runtime.clone());
    let granted = restricted
        .update_production_order_note(&a, &update("order-a", "Runtime note", 5))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        restricted
            .production_notes_for_receipts(&["EPC1".into()])
            .await
            .unwrap()["EPC1"],
        granted
    );
    check_output_workspace(pool, &restricted, &a, &b).await;
    runtime.close().await;
    let reset = mini_rs_erp::db::postgres_order_reset::PostgresOrderResetStore::new(pool.clone())
        .reset_all_orders()
        .await
        .unwrap();
    assert_eq!(reset.preparation_order_notes_deleted, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM mini_preparation_production_order_notes"
        )
        .fetch_one(pool)
        .await
        .unwrap(),
        0
    );
}

async fn check_output_workspace(pool: &PgPool, journal: &PostgresPreparationStore, owner: &Principal, other: &Principal) {
    let batches = journal.production_journal_batches(&owner.ref_, "EPC2", "completed", 0, 1).await.unwrap();
    assert_eq!(batches["batches"][0]["batch_id"], "batch:batch-a");
    assert_eq!(batches["batches"][0]["order_code"], "0317");
    assert_eq!(batches["has_more"], false);
    let page = journal.production_journal_rolls(&owner.ref_, "0317", "all", 0, 1).await.unwrap();
    assert_eq!(page["has_more"], true);
    let next = journal.production_journal_rolls(&owner.ref_, "0317", "all", 1, 1).await.unwrap();
    assert_ne!(page["rolls"][0]["receipt_name"], next["rolls"][0]["receipt_name"]);

    let batch = |id, text, revision| serde_json::from_value::<PreparationOutputNoteUpdate>(json!({
        "scope":"batch","batch_id":id,"text":text,"expected_revision":revision
    })).unwrap();
    let selected = |rolls, text| serde_json::from_value::<PreparationOutputNoteUpdate>(json!({
        "scope":"rolls","rolls":rolls,"text":text
    })).unwrap();
    let roll = |id, revision| json!({"receipt_name":id,"expected_revision":revision});
    assert!(journal.update_output_notes(other, &batch("batch:batch-a", "denied", 0)).await.unwrap().is_none());
    assert!(journal.update_output_notes(owner, &batch("receipt:EPC3", "denied", 0)).await.is_err());
    let all = journal.update_output_notes(owner, &batch("batch:batch-a", " Har bir rulonga ", 0)).await.unwrap().unwrap();
    assert_eq!(all["batch:batch-a"]["text"], "Har bir rulonga");
    let two = journal.update_output_notes(owner, &selected(vec![roll("receipt-EPC1", 0), roll("receipt-EPC2", 0)], "Faqat tanlanganlar"))
        .await.unwrap().unwrap();
    assert_eq!(two.len(), 2);
    assert!(journal.update_output_notes(owner, &selected(vec![roll("receipt-EPC3", 0), roll("receipt-OTHER", 0)], "Ruxsat yo‘q"))
        .await.unwrap().is_none());
    assert!(journal.update_output_notes(owner, &selected(vec![roll("receipt-DRAFT", 0)], "Tasdiqlanmagan"))
        .await.unwrap().is_none());
    assert!(matches!(journal.update_output_notes(owner, &selected(vec![roll("receipt-EPC3", 0), roll("receipt-EPC2", 0)], "Eski versiya")).await,
        Err(PreparationError::Conflict(_))));

    let standalone = journal.production_journal_batch(&owner.ref_, "receipt:EPC3", 0, 50).await.unwrap().unwrap();
    assert!(standalone["rolls"][0]["roll_note"].is_null());
    assert!(standalone["rolls"][0]["batch_note"].is_null());

    sqlx::query("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,barcode,qty,status,source_receipt_id)
        VALUES ('raw:epc2','Own W','PE PR','epc2',20,'available','receipt-EPC2')")
        .execute(pool).await.unwrap();
    let barcodes = ["epc1", "epc2", "cut-a", "cut-b", "cut-a1"].map(String::from);
    let scan = || journal.output_notes_for_inventory(
        &barcodes, PreparationNoteViewerRole::HomashyoRezkachi, "raw-cutter",
    );
    let originals = scan().await.unwrap();
    assert_eq!(originals.len(), 2);
    assert_eq!(originals["EPC2"].0.as_ref(), Some(&all["batch:batch-a"]));
    assert_eq!(originals["EPC2"].1.as_ref(), Some(&two["receipt-EPC2"]));

    // EPC2 was cut into A and B, then A was cut again. All retain EPC2's receipt.
    sqlx::raw_sql("UPDATE mini_raw_material_stock SET status='consumed',qty=0 WHERE id='raw:epc2';
        INSERT INTO mini_raw_material_stock(id,warehouse,item_code,barcode,qty,status,source_receipt_id,payload_json)
        VALUES ('raw:a','Own W','PE PR','CUT-A',0,'consumed','receipt-EPC2','{\"parent_stock_id\":\"raw:epc2\"}'),
               ('raw:b','Own W','PE PR','CUT-B',9,'available','receipt-EPC2','{\"parent_stock_id\":\"raw:epc2\"}'),
               ('raw:a1','Own W','PE PR','CUT-A1',8,'available','receipt-EPC2','{\"parent_stock_id\":\"raw:a\"}');")
        .execute(pool).await.unwrap();
    let inventory = scan().await.unwrap();
    assert_eq!(inventory.keys().map(String::as_str).collect::<Vec<_>>(), ["EPC1"]);
    assert_eq!(inventory["EPC1"].0.as_ref().unwrap(), &all["batch:batch-a"]);
    assert_eq!(inventory["EPC1"].1.as_ref().unwrap(), &two["receipt-EPC1"]);

    let detail = journal.production_journal_batch(&owner.ref_, "batch:batch-a", 0, 50).await.unwrap().unwrap();
    assert_eq!(detail["batch"]["batch_note"], all["batch:batch-a"]);
    for row in detail["rolls"].as_array().unwrap() {
        assert_eq!(row["batch_note"], all["batch:batch-a"]);
        assert_eq!(row["roll_note"], two[row["receipt_name"].as_str().unwrap()]);
    }
    let prep = PreparationNoteViewerRole::TayyorlovMasteri;
    assert!(journal.output_notes_for_inventory(&["EPC3".into()], prep, &owner.ref_).await.unwrap().is_empty());
    assert_eq!(journal.output_notes_for_barcode(" epc1 ", prep, &other.ref_).await.unwrap(), inventory["EPC1"]);
    assert_eq!(journal.output_notes_for_barcode("EPC", prep, &owner.ref_).await.unwrap(), (None, None));
    check_note_audiences(journal, owner, other).await;

    let store = PostgresRpsBatchStore::new(pool.clone());
    let mut unassigned = RpsBatchSession {
        id: "unassigned".into(), owner_key: format!("tayyorlov_masteri:{}", owner.ref_),
        owner_role: "tayyorlov_masteri".into(), owner_ref: owner.ref_.clone(),
        item_code: "PE".into(), item_name: "Buyurtmasiz PE".into(), warehouse: "Own W".into(),
        active: true, created_at: "2026-10-30T05:00:00Z".into(), updated_at: "2026-10-30T05:00:00Z".into(),
        ..Default::default()
    };
    store.put(&unassigned).await.unwrap();
    let later = journal.update_output_notes(owner, &batch("batch:unassigned", "Keyingi rulonlarga ham", 0)).await.unwrap().unwrap();
    unassigned.prints.push(print("NO-ORDER"));
    store.complete(&RpsBatchSession { active: false, ..unassigned }).await.unwrap();
    sqlx::raw_sql("INSERT INTO mini_gscale_receipts(name,status,item_code,warehouse,qty,barcode,payload_json)
        VALUES('receipt-NO-ORDER','submitted','PE','Own W',1,'NO-ORDER',
          '{\"actor_role\":\"tayyorlov_masteri\",\"actor_ref\":\"master-1\",\"item_name\":\"Buyurtmasiz PE\"}');")
        .execute(pool).await.unwrap();
    let batch = journal.production_journal_batch(&owner.ref_, "batch:unassigned", 0, 50).await.unwrap().unwrap();
    assert_eq!(batch["batch"]["order_id"], "");
    assert_eq!(batch["rolls"][0]["batch_note"], later["batch:unassigned"]);
    let plain = journal.production_journal_rolls(&owner.ref_, "NO-ORDER", "completed", 0, 50).await.unwrap();
    assert_eq!(plain["rolls"][0]["epc"], "NO-ORDER");
    sqlx::query("INSERT INTO mini_raw_material_stock
        (id,warehouse,item_code,item_name,barcode,qty,uom,status,source_receipt_id)
        VALUES ('raw:no-order','Own W','PE','Buyurtmasiz PE','NO-ORDER',1,'kg','available','receipt-NO-ORDER')")
        .execute(pool).await.unwrap();
    let (batch_note, roll_note) = journal.output_notes_for_barcode("no-order", prep, &other.ref_).await.unwrap();
    assert_eq!(batch_note, Some(later["batch:unassigned"].clone()));
    assert!(roll_note.is_none());
}

async fn check_note_audiences(journal: &PostgresPreparationStore, owner: &Principal, other: &Principal) {
    use PreparationNoteViewerRole as Role;

    let update = |payload| serde_json::from_value::<PreparationOutputNoteUpdate>(payload).unwrap();
    let batch = journal.update_output_notes(owner, &update(json!({
        "scope":"batch", "batch_id":"batch:batch-a", "expected_revision":1,
        "audience":{"mode":"roles", "roles":["rezka_master"]}
    }))).await.unwrap().unwrap();
    assert_eq!(batch["batch:batch-a"]["text"], "Har bir rulonga");
    let roll = |id, revision| json!({"receipt_name":id,"expected_revision":revision});
    journal.update_output_notes(owner, &update(json!({
        "scope":"rolls", "rolls":[roll("receipt-EPC2", 1)], "text":"Ikkinchi rulon"
    }))).await.unwrap().unwrap();
    let rolls = journal.update_output_notes(owner, &update(json!({
        "scope":"rolls", "rolls":[roll("receipt-EPC1", 1), roll("receipt-EPC2", 2)],
        "audience":{"mode":"roles", "roles":["homashyo_rezkachi", "rezka_worker"]}
    }))).await.unwrap().unwrap();
    assert_eq!(rolls["receipt-EPC1"]["text"], "Faqat tanlanganlar");
    assert_eq!(rolls["receipt-EPC2"]["text"], "Ikkinchi rulon");
    let batch_note = Some(batch["batch:batch-a"].clone());
    let roll_note = Some(rolls["receipt-EPC1"].clone());
    for (role, viewer_ref, expected) in [
        (Role::TayyorlovMasteri, owner.ref_.as_str(), (batch_note.clone(), roll_note.clone())),
        (Role::TayyorlovMasteri, other.ref_.as_str(), (None, None)),
        (Role::RezkaMaster, "rezka-master", (batch_note, None)),
        (Role::HomashyoRezkachi, "raw-cutter", (None, roll_note.clone())),
        (Role::RezkaWorker, "cut-worker", (None, roll_note)),
        (Role::Aparatchi, "print-worker", (None, None)),
        (Role::Admin, owner.ref_.as_str(), (None, None)),
    ] {
        assert_eq!(journal.output_notes_for_barcode("epc1", role, viewer_ref).await.unwrap(), expected);
    }
    assert!(journal.output_notes_for_inventory(&["epc1".into()], Role::TayyorlovMasteri, &other.ref_).await.unwrap().is_empty());

    let edited = journal.update_output_notes(owner, &update(json!({
        "scope":"rolls", "rolls":[roll("receipt-EPC1", 2)], "text":"Yangi matn"
    }))).await.unwrap().unwrap();
    assert_eq!(edited["receipt-EPC1"]["audience"], rolls["receipt-EPC1"]["audience"]);
    assert_eq!(journal.output_notes_for_barcode("epc1", Role::Admin, "admin").await.unwrap(), (None, None));

    let all = journal.update_output_notes(owner, &update(json!({
        "scope":"batch", "batch_id":"batch:batch-a", "expected_revision":2,
        "audience":{"mode":"all"}
    }))).await.unwrap().unwrap();
    let private = journal.update_output_notes(owner, &update(json!({
        "scope":"rolls", "rolls":[roll("receipt-EPC1", 3)], "audience":{"mode":"roles", "roles":[]}
    }))).await.unwrap().unwrap();
    let (batch_note, roll_note) = journal.output_notes_for_barcode("epc1", Role::HomashyoRezkachi, "raw-cutter").await.unwrap();
    assert_eq!(batch_note, Some(all["batch:batch-a"].clone()));
    assert!(roll_note.is_none());
    assert_eq!(journal.output_notes_for_barcode("epc1", Role::TayyorlovMasteri, &owner.ref_).await.unwrap().1,
        Some(private["receipt-EPC1"].clone()));
}

fn print(epc: &str) -> RpsBatchPrintEntry {
    RpsBatchPrintEntry {
        epc: epc.into(),
        gross_qty: if epc == "EPC2" { 22.0 } else { 12.0 },
        net_qty: if epc == "EPC2" { 20.0 } else { 10.0 },
        printed_at: "2026-10-28T05:05:00Z".into(),
        ..Default::default()
    }
}
fn number(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}
