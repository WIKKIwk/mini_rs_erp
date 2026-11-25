use mini_rs_erp::core::{
    auth::models::{Principal, PrincipalRole},
    inventory_movements::InventoryActor,
    preparation::{MaterialCreate, PreparationError},
};
use mini_rs_erp::db::{
    postgres::{apply_postgres_migrations_through_version, canonical_apparatus_service},
    postgres_preparation::{
        PostgresPreparationStore,
        paddons::{PaddonCreate, PaddonIssueCreate, PaddonQuery, PaddonRollAttachment, PaddonRollOptions},
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
    let result = tokio::task::LocalSet::new()
        .run_until(async move {
            tokio::task::spawn_local(async move { verify(&checks, options).await }).await
        })
        .await;
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
    apply_postgres_migrations_through_version(pool, "0203")
        .await
        .unwrap();
    verify_paddon_upgrade(pool).await;
    apply_postgres_migrations_through_version(pool, "0206")
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
    let source_code = first["paddon_code"].as_str().unwrap();
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

    let mut unweighed = serde_json::to_value(receipt("Own", item, "unweighed-1")).unwrap();
    unweighed["contents"][0]
        .as_object_mut()
        .unwrap()
        .remove("unit_kg");
    let loading = store
        .receive_raw_material_paddon(&master, serde_json::from_value(unweighed).unwrap(), &[])
        .await
        .unwrap();
    let code = loading["paddon_code"].as_str().unwrap();
    let lookup = || {
        serde_json::from_value(json!({
            "code": format!("raw-material-paddon:{code}"), "status": "available",
        }))
        .unwrap()
    };
    let before = store.raw_material_paddons(&master, lookup()).await.unwrap();
    assert_eq!(before["paddons"][0]["unweighed_count"], 3);
    assert_eq!(before["paddons"][0]["remaining_count"], 3);
    assert_eq!(before["paddons"][0]["remaining_kg"], "0.000000");
    assert_eq!(
        store
            .raw_material_paddons(&supplier, lookup())
            .await
            .unwrap()["paddons"],
        json!([])
    );
    let stock_count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_raw_material_stock")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(stock_count, 6);

    let master_inventory = InventoryActor::new(master.clone(), false, ["Own".into()]);
    let supply_inventory = InventoryActor::new(supplier.clone(), false, ["Supply".into()]);
    let issue =
        |code: &str, request: &str, quantities: &[&str], destination: Value| -> PaddonIssueCreate {
            serde_json::from_value(json!({
                "code": code, "request_id": request, "line_no": 1, "weights": quantities,
                "destination": destination,
            }))
            .unwrap()
        };
    let take_two = || issue(code, "issue-001", &["24.6", "26.1"], Value::Null);
    assert!(matches!(
        store
            .issue_raw_material_paddon(&supply_inventory, take_two())
            .await,
        Err(PreparationError::Forbidden)
    ));
    let (first, retry) = tokio::join!(
        store.issue_raw_material_paddon(&master_inventory, take_two()),
        store.issue_raw_material_paddon(&master_inventory, take_two())
    );
    let first = first.unwrap();
    assert_eq!(first, retry.unwrap());
    assert_eq!(first["labels"][0]["kg"], "24.600000");
    assert_eq!(first["labels"][1]["kg"], "26.100000");
    assert_ne!(first["labels"][0]["barcode"], first["labels"][1]["barcode"]);
    let after = store.raw_material_paddons(&master, lookup()).await.unwrap();
    assert_eq!(after["paddons"][0]["remaining_count"], 1);
    assert_eq!(after["paddons"][0]["unweighed_count"], 1);
    assert_eq!(
        store
            .raw_material_paddon_issues(&master, code)
            .await
            .unwrap(),
        vec![first]
    );
    assert!(
        store
            .raw_material_paddon_issues(&supplier, code)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        store
            .issue_raw_material_paddon(
                &master_inventory,
                issue(code, "issue-002", &["5", "5"], Value::Null)
            )
            .await,
        Err(PreparationError::Insufficient)
    ));

    sqlx::raw_sql(
        "INSERT INTO mini_factory_locations(id, name) VALUES ('paddon-floor', 'Paddon Floor');
         INSERT INTO mini_warehouses(id, name) VALUES ('unassigned', 'Unassigned');",
    )
    .execute(pool)
    .await
    .unwrap();
    let state: String = sqlx::query_scalar(
        "SELECT id FROM mini_inventory_locations WHERE factory_location_id='paddon-floor'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(matches!(
        store
            .issue_raw_material_paddon(
                &master_inventory,
                issue(
                    code,
                    "master-state-denied",
                    &["12"],
                    json!({"kind":"state","id":state})
                )
            )
            .await,
        Err(PreparationError::Forbidden)
    ));
    let supply_code = second["paddon_code"].as_str().unwrap();
    let failed = issue(
        supply_code,
        "supply-bad",
        &["22"],
        json!({"kind":"warehouse","id":"unassigned"}),
    );
    assert!(matches!(
        store
            .issue_raw_material_paddon(&supply_inventory, failed)
            .await,
        Err(PreparationError::Conflict(_))
    ));
    assert!(
        store
            .raw_material_paddon_issues(&supplier, supply_code)
            .await
            .unwrap()
            .is_empty()
    );
    let stated = store
        .issue_raw_material_paddon(
            &supply_inventory,
            issue(
                supply_code,
                "supply-state",
                &["24.7"],
                json!({"kind":"state","id":state}),
            ),
        )
        .await
        .unwrap();
    assert_eq!(stated["destination"], "Paddon Floor");
    assert_eq!(stated["destination_kind"], "state");
    assert_eq!(stated["label_warehouse"], "Supply");
    let barcode = stated["labels"][0]["barcode"].as_str().unwrap();
    let (position, weight, correction): (String, String, String) = sqlx::query_as(
        "SELECT p.physical_location_id, s.qty::text, e.qty_delta::text
         FROM mini_raw_material_stock s
         JOIN mini_inventory_placements p ON p.asset_ref=s.id AND p.asset_kind='raw_material'
         JOIN mini_raw_material_events e ON e.barcode=s.barcode AND e.event_type='adjustment_decrease'
         WHERE s.barcode=$1",
    ).bind(barcode).fetch_one(pool).await.unwrap();
    assert_eq!(position, state);
    assert_eq!(weight, "24.700000");
    assert_eq!(correction.parse::<f64>().unwrap(), -0.3);

    let transferred = store
        .issue_raw_material_paddon(
            &supply_inventory,
            issue(
                supply_code,
                "supply-transfer",
                &["26"],
                json!({"kind":"warehouse","id":"own"}),
            ),
        )
        .await
        .unwrap();
    assert_eq!(transferred["transfer_status"], "requested");
    assert_eq!(transferred["destination_kind"], "warehouse");
    let linked_units: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mini_inventory_transfer_lines WHERE transfer_id=$1",
    )
    .bind(transferred["issue_id"].as_str().unwrap())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(linked_units, 1);
    let supplier_after = store
        .raw_material_paddons(&supplier, filter("available", supply_code))
        .await
        .unwrap();
    assert_eq!(supplier_after["paddons"][0]["remaining_kg"], "25.000000");
    assert_eq!(supplier_after["paddons"][0]["remaining_count"], 1);
    verify_partial_and_transfer(pool, &store, &master_inventory, &supply_inventory, &runtime).await;
    verify_prepared_roll_packing(pool, &store, &master_inventory, code, supply_code, item).await;
    verify_batch_packing(pool, &store, &master, code, source_code, item).await;
    runtime.close().await;
}

async fn verify_prepared_roll_packing(
    pool: &PgPool,
    store: &PostgresPreparationStore,
    inventory: &InventoryActor,
    code: &str,
    foreign_code: &str,
    item: &str,
) {
    use mini_rs_erp::core::gscale::{models::CreateMaterialReceiptDraftInput, ports::MaterialReceiptStorePort};
    use mini_rs_erp::db::postgres_gscale_receipt::PostgresGscaleReceiptStore;

    let master = &inventory.principal;
    let receipts = PostgresGscaleReceiptStore::new(pool.clone());
    let order = "paddon-prep-order";
    let apparatus = "apparatus:default:bosma_7";
    sqlx::query("INSERT INTO mini_production_maps(id,product_code,title,map_json)
                 VALUES($1,'PE','PE','{}')")
        .bind(order)
        .execute(pool)
        .await
        .unwrap();
    let mut names = Vec::new();
    for (barcode, linked) in [("PADDON-PREP-1", true), ("PADDON-PREP-2", false)] {
        let draft = receipts.create_material_receipt_draft(CreateMaterialReceiptDraftInput {
            item_code: item.into(),
            item_name: "PE".into(),
            warehouse: "Own".into(),
            barcode: barcode.into(),
            qty: 12.75,
            width_mm: Some(720.0),
            micron: Some(20.0),
            length_m: Some(400.0),
            actor_role: "tayyorlov_masteri".into(),
            actor_ref: master.ref_.clone(),
            order_assignment: linked.then(|| json!({
                "order_id": order, "apparatus": apparatus, "item_group": "Homashyo",
                "assigned_by_role": "tayyorlov_masteri", "assigned_by_ref": master.ref_,
            })),
            ..Default::default()
        }).await.unwrap();
        receipts.submit_stock_entry_draft(&draft.name).await.unwrap();
        names.push(draft.name);
    }
    let options: PaddonRollOptions = serde_json::from_value(json!({"receipt_names": names})).unwrap();
    let choices = store.production_paddon_options(master, options, false).await.unwrap();
    let choices = choices["paddons"].as_array().unwrap();
    assert!(choices.iter().any(|paddon| paddon["paddon_code"] == code));
    assert!(choices.iter().all(|paddon| paddon["warehouse"] == "Own"));

    let attach = |key, target, receipt_names: &[String]| -> PaddonRollAttachment {
        serde_json::from_value(json!({
            "request_id":key, "code": target, "receipt_names": receipt_names
        })).unwrap()
    };
    assert!(matches!(
        store.attach_prepared_rolls(master, attach("pack-foreign", foreign_code, &names)).await,
        Err(PreparationError::Forbidden)
    ));
    assert!(store.attach_prepared_rolls(master, attach(
        "pack-missing", code, &[names[0].clone(), "missing-receipt".into()]
    )).await.is_err());
    let before: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT id, status, qty::text, source_receipt_id FROM mini_raw_material_stock
         WHERE source_receipt_id=ANY($1::text[]) ORDER BY id",
    ).bind(&names).fetch_all(pool).await.unwrap();
    assert_eq!(before.len(), 2);
    assert!(before.iter().all(|(_, status, _, _)| status == "available"));

    let (saved, retry) = tokio::join!(
        store.attach_prepared_rolls(master, attach("pack-prep-1", code, &names)),
        store.attach_prepared_rolls(master, attach("pack-prep-1", code, &names)),
    );
    let saved = saved.unwrap();
    assert_eq!(saved, retry.unwrap());
    assert!(store.attach_prepared_rolls(master, attach("pack-prep-2", code, &names)).await.is_err());
    let after: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT id, status, qty::text, source_receipt_id FROM mini_raw_material_stock
         WHERE source_receipt_id=ANY($1::text[]) ORDER BY id",
    ).bind(&names).fetch_all(pool).await.unwrap();
    let expected: Vec<_> = before.into_iter().map(|(id, _, qty, receipt)| {
        (id, "on_paddon".to_owned(), qty, receipt)
    }).collect();
    assert_eq!(after, expected);

    let page = store.raw_material_paddons(master, filter("all", code)).await.unwrap();
    let unit = page["paddons"][0]["contents"].as_array().unwrap().iter()
        .find(|unit| unit["prepared_barcode"] == "PADDON-PREP-1").unwrap();
    assert_eq!(unit["issuable_count"], 1);
    let issue = |key, kg| -> PaddonIssueCreate {
        serde_json::from_value(json!({
            "request_id": key, "code":code, "line_no":unit["line_no"], "weights":[kg]
        })).unwrap()
    };
    assert!(matches!(
        store.issue_raw_material_paddon(inventory, issue("unpack-wrong-kg", "13")).await,
        Err(PreparationError::Conflict(_))
    ));
    let issued = store.issue_raw_material_paddon(inventory, issue("unpack-prep", "12.75")).await.unwrap();
    assert_eq!(issued["existing_qr"], true);
    assert_eq!(issued["labels"][0]["barcode"], "PADDON-PREP-1");
    assert_eq!(store.attach_prepared_rolls(master, attach("pack-prep-1", code, &names)).await.unwrap(), saved);
    let restored: (String, String, String, String) = sqlx::query_as(
        "SELECT s.status, s.qty::text, s.source_receipt_id, a.order_id
         FROM mini_raw_material_stock s JOIN mini_raw_material_assignments a ON a.barcode=s.barcode
         WHERE s.barcode='PADDON-PREP-1'",
    ).fetch_one(pool).await.unwrap();
    assert_eq!(restored, ("available".into(), "12.750000".into(), names[0].clone(), order.into()));
    store.attach_prepared_rolls(master, attach("pack-released", code, &names[..1])).await.unwrap();
}

async fn verify_batch_packing(
    pool: &PgPool,
    store: &PostgresPreparationStore,
    master: &Principal,
    code: &str,
    source_code: &str,
    item: &str,
) {
    const BATCH: &str = "batch:pack-history";
    const NEXT_BATCH: &str = "batch:pack-next";
    sqlx::query(
        "INSERT INTO mini_gscale_receipts (name, status, item_code, warehouse, qty, barcode, payload_json)
         SELECT 'batch-pack-'||n, 'submitted', $1, 'Own', 5, 'BATCH-PACK-'||n,
                jsonb_build_object('actor_role','tayyorlov_masteri','actor_ref',$2::text)
         FROM generate_series(1, 107) n",
    )
    .bind(item)
    .bind(&master.ref_)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO mini_raw_material_stock
             (id, status, warehouse, item_code, barcode, qty, source_receipt_id)
         SELECT 'roll-'||name, CASE WHEN barcode='BATCH-PACK-103' THEN 'consumed' ELSE 'available' END,
                CASE WHEN barcode='BATCH-PACK-105' THEN 'Supply' ELSE 'Own' END,
                item_code, barcode, qty, name
         FROM mini_gscale_receipts WHERE name LIKE 'batch-pack-%'",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO mini_rps_batch_history (batch_id, owner_key, owner_role, owner_ref, payload_json)
         SELECT CASE WHEN n <= 105 THEN 'pack-history' ELSE 'pack-next' END,
                'tayyorlov_masteri:'||$1, 'tayyorlov_masteri', $1,
                jsonb_build_object('prints', jsonb_agg(jsonb_build_object('epc', 'BATCH-PACK-'||n)))
         FROM generate_series(1, 106) n
         GROUP BY 1",
    )
    .bind(&master.ref_)
    .execute(pool)
    .await
    .unwrap();
    let options = |ids: &[&str]| {
        serde_json::from_value::<PaddonRollOptions>(json!({"batch_ids": ids})).unwrap()
    };
    let both = [BATCH, NEXT_BATCH];
    let attach = |request_id, names: &[String]| {
        serde_json::from_value::<PaddonRollAttachment>(json!({
            "request_id": request_id, "code": code, "receipt_names": names, "batch_ids": both
        }))
        .unwrap()
    };
    store
        .attach_prepared_rolls(
            master,
            serde_json::from_value(json!({
                "request_id": "pack-one-first", "code": source_code, "receipt_names": ["batch-pack-104"]
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    let outsider = actor(PrincipalRole::TayyorlovMasteri, "another-master");
    assert!(matches!(
        store.production_paddon_options(&outsider, options(&both), false).await,
        Err(PreparationError::Forbidden)
    ));

    assert!(matches!(
        store.production_paddon_options(master, options(&[BATCH, "batch:missing"]), false).await,
        Err(PreparationError::Forbidden)
    ));
    let single = store
        .production_paddon_options(master, serde_json::from_value(json!({"batch_id": BATCH})).unwrap(), false)
        .await
        .unwrap();
    assert_eq!(single["batch_selection"]["batch_id"], BATCH);
    assert_eq!(single["batch_selection"]["total_rolls"], 105);

    let page = store.production_paddon_options(master, options(&both), false).await.unwrap();
    assert!(page["paddons"].as_array().unwrap().iter().any(|p| p["paddon_code"] == code));
    assert!(page["paddons"].as_array().unwrap().iter().all(|p| p["warehouse"] == "Own"));
    let preview = &page["batch_selection"];
    assert_eq!(preview["batch_ids"], json!(both));
    assert_eq!(preview["total_rolls"], 106);
    assert_eq!(
        preview["skipped"],
        json!({"consumed": 1, "outside_warehouse": 1, "packed": 1})
    );
    let already_packed = json!([{
        "barcode": "BATCH-PACK-104",
        "paddon_name": "Oq zaxira",
        "paddon_code": source_code
    }]);
    assert_eq!(preview["packed_rolls"], already_packed);
    let selected: Vec<String> = serde_json::from_value(
        preview["receipt_names_by_warehouse"]["Own"].clone(),
    )
    .unwrap();
    assert_eq!(selected.len(), 103);
    assert!(selected.iter().any(|name| name == "batch-pack-106"));
    assert!(selected.iter().all(|name| name != "batch-pack-107"));

    let mut wrong = selected.clone();
    wrong[0] = "batch-pack-107".into();
    assert!(matches!(
        store.attach_prepared_rolls(master, attach("pack-batch-wrong", &wrong)).await,
        Err(PreparationError::Conflict(_))
    ));
    sqlx::query("UPDATE mini_raw_material_stock SET status='in_use' WHERE barcode='BATCH-PACK-1'")
        .execute(pool)
        .await
        .unwrap();
    assert!(matches!(
        store.attach_prepared_rolls(master, attach("pack-batch-stale", &selected)).await,
        Err(PreparationError::Conflict(_))
    ));
    let preview = store.production_paddon_options(master, options(&both), false).await.unwrap();
    let selected: Vec<String> =
        serde_json::from_value(preview["batch_selection"]["receipt_names_by_warehouse"]["Own"].clone())
            .unwrap();
    assert_eq!(selected.len(), 102);
    let saved = store
        .attach_prepared_rolls(master, attach("pack-batch-confirmed", &selected))
        .await
        .unwrap();
    assert_eq!(saved["receipt_names"].as_array().unwrap().len(), 102);
    assert_eq!(
        store.attach_prepared_rolls(master, attach("pack-batch-confirmed", &selected)).await.unwrap(),
        saved
    );
    let packed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mini_raw_material_paddon_units u
         JOIN mini_raw_material_stock s ON s.id=u.stock_id
         WHERE u.issue_id IS NULL AND s.status='on_paddon' AND s.source_receipt_id=ANY($1)",
    )
    .bind(&selected)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(packed, 102);
    let empty = store.production_paddon_options(master, options(&both), false).await.unwrap();
    assert_eq!(empty["paddons"], json!([]));
    assert_eq!(empty["batch_selection"]["receipt_names_by_warehouse"], json!({}));

    let roll_options = || serde_json::from_value(json!({
        "receipt_names": ["batch-pack-104", "batch-pack-107"]
    })).unwrap();
    assert!(matches!(
        store.production_paddon_options(master, roll_options(), false).await,
        Err(PreparationError::Conflict(_))
    ));
    let page = store.production_paddon_options(master, roll_options(), true).await.unwrap();
    let preview = &page["roll_selection"];
    assert_eq!(preview["total_rolls"], 2);
    assert_eq!(preview["skipped"], json!({"packed": 1}));
    assert_eq!(preview["packed_rolls"], already_packed);
    assert_eq!(preview["receipt_names_by_warehouse"]["Own"], json!(["batch-pack-107"]));
    let attachable = &preview["receipt_names_by_warehouse"]["Own"];
    let saved = store.attach_prepared_rolls(master, serde_json::from_value(json!({
        "request_id": "pack-manual-remaining", "code": code, "receipt_names": attachable
    })).unwrap()).await.unwrap();
    assert_eq!(saved["receipt_names"], *attachable);

    let places: Vec<(String, String)> = sqlx::query_as(
        "SELECT s.barcode, p.code FROM mini_raw_material_paddon_units u
         JOIN mini_raw_material_stock s ON s.id = u.stock_id
         JOIN mini_raw_material_paddons p ON p.id = u.paddon_id
         WHERE u.issue_id IS NULL AND s.barcode IN ('BATCH-PACK-104','BATCH-PACK-107')
         ORDER BY s.barcode",
    ).fetch_all(pool).await.unwrap();
    assert_eq!(places, [
        ("BATCH-PACK-104".into(), source_code.into()),
        ("BATCH-PACK-107".into(), code.into()),
    ]);
    let all_packed = store.production_paddon_options(master, roll_options(), true).await.unwrap();
    assert_eq!(all_packed["paddons"], json!([]));
    assert_eq!(all_packed["roll_selection"]["skipped"], json!({"packed": 2}));
}

async fn verify_partial_and_transfer(
    pool: &PgPool,
    store: &PostgresPreparationStore,
    master: &InventoryActor,
    supplier: &InventoryActor,
    runtime: &PgPool,
) {
    use mini_rs_erp::core::inventory_movements::{
        InventoryAssetKind, InventoryAssetSelector, InventoryMovementError,
        InventoryMovementStorePort, InventoryTransferAction, InventoryTransferActionKind,
        InventoryTransferCreate,
    };
    use mini_rs_erp::core::preparation::{
        ConsumptionCreate, ConsumptionLine, MaterialResponsibilityAssign,
    };
    use mini_rs_erp::db::postgres_inventory_movements::PostgresInventoryMovementStore;

    let material = store
        .create_material(
            &master.principal,
            MaterialCreate {
                request_id: "packed-granula".into(),
                warehouse: "Own".into(),
                name: "TF430".into(),
            },
        )
        .await
        .unwrap();
    let item = material["item_code"].as_str().unwrap();
    let groups = [material["item_group"].as_str().unwrap().to_lowercase()];
    let paddon = store.receive_raw_material_paddon(&supplier.principal, serde_json::from_value(json!({
        "request_id":"mixed-pallet","warehouse":"Supply","contents":[
            {"item_code":item,"lot_number":"PB260139","kind":"bag","count":1,"unit_kg":"25"},
            {"item_code":item,"lot_number":"PB260139","kind":"roll","count":1},
            {"item_code":item,"lot_number":"PB260139","kind":"roll","count":1,"width_mm":"500","micron":"40"}
        ]})).unwrap(),&groups).await.unwrap();
    let code = paddon["paddon_code"].as_str().unwrap();
    let list = || store.raw_material_paddons(&supplier.principal, filter("all", code));
    let before = list().await.unwrap();
    assert_eq!(before["paddons"][0]["contents"][1]["micron"], Value::Null);
    assert_eq!(
        before["paddons"][0]["contents"][2]["width_mm"],
        "500.000000"
    );
    let move_store = PostgresInventoryMovementStore::new(runtime.clone());
    let id: String = sqlx::query_scalar(
        "SELECT stock_id FROM mini_raw_material_paddon_units WHERE paddon_id=$1 AND line_no=1",
    )
    .bind(paddon["paddon_id"].as_str().unwrap())
    .fetch_one(pool)
    .await
    .unwrap();
    let ordinary = InventoryTransferCreate {
        source_warehouse_id: "supply".into(),
        destination_warehouse_id: "own".into(),
        assets: vec![InventoryAssetSelector {
            asset_kind: InventoryAssetKind::RawMaterial,
            asset_ref: id.clone(),
        }],
        note: String::new(),
        idempotency_key: "ordinary-transfer".into(),
    };
    assert!(matches!(
        InventoryMovementStorePort::create_transfer(
            &move_store,
            supplier,
            "ordinary-transfer",
            &ordinary
        )
        .await,
        Err(InventoryMovementError::AssetUnavailable)
    ));
    assert_eq!(list().await.unwrap()["paddons"][0]["remaining_count"], 3);

    let take = |key: &str, line: i32, kg: &str, extra: Value| -> PaddonIssueCreate {
        let mut v = json!({"request_id":key,"code":code,"line_no":line,"weights":[kg]});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_value(v).unwrap()
    };
    let first = || {
        take(
            "ten-to-master",
            1,
            "10",
            json!({"remainder_kg":"15","destination":{"kind":"warehouse","id":"own"}}),
        )
    };
    let (a, b) = tokio::join!(
        store.issue_raw_material_paddon(supplier, first()),
        store.issue_raw_material_paddon(supplier, first())
    );
    let issued = a.unwrap();
    assert_eq!(issued, b.unwrap());
    let label = issued["labels"][0]["barcode"].as_str().unwrap();
    assert_eq!(issued["labels"][0]["kg"], "10.000000");
    let after = list().await.unwrap();
    assert_eq!(after["paddons"][0]["remaining_kg"], "15.000000");
    assert_eq!(after["paddons"][0]["remaining_count"], 3);
    assert_eq!(after["paddons"][0]["contents"][0]["open_unit_no"], 1);
    let event_total: (String, i64) = sqlx::query_as(
        "SELECT sum(qty_delta)::text,count(*) FILTER (WHERE event_type LIKE 'adjustment%')
        FROM mini_raw_material_events WHERE item_code=$1",
    )
    .bind(item)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(event_total.0.parse::<f64>().unwrap(), 25.0);
    assert_eq!(event_total.1, 0);

    let issue_id = issued["issue_id"].as_str().unwrap();
    let track = |action: &str, attempt: &str, previous: Option<&str>| {
        serde_json::from_value(json!({
        "issue_id":issue_id,"barcode":label,"action":action,"attempt":attempt,"previous_attempt":previous
    })).unwrap()
    };
    assert!(matches!(
        store
            .track_paddon_print(&master.principal, track("start", "attempt-one", None))
            .await,
        Err(PreparationError::Forbidden)
    ));
    store
        .track_paddon_print(&supplier.principal, track("start", "attempt-one", None))
        .await
        .unwrap();
    let history = || store.raw_material_paddon_issues(&supplier.principal, code);
    assert_eq!(
        history().await.unwrap()[0]["print_states"][label]["printed"],
        false
    );
    assert!(matches!(
        store
            .track_paddon_print(&supplier.principal, track("start", "other-device", None))
            .await,
        Err(PreparationError::Conflict(_))
    ));
    store
        .track_paddon_print(&supplier.principal, track("confirm", "attempt-one", None))
        .await
        .unwrap();
    assert_eq!(
        store
            .issue_raw_material_paddon(supplier, first())
            .await
            .unwrap()["print_states"][label]["printed"],
        true
    );
    store
        .track_paddon_print(
            &supplier.principal,
            track("start", "attempt-two", Some("attempt-one")),
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .track_paddon_print(&supplier.principal, track("confirm", "attempt-one", None))
            .await,
        Err(PreparationError::Conflict(_))
    ));

    let open = |key: &str| {
        take(
            key,
            1,
            "5",
            json!({"open_unit_no":1,"expected_open_kg":"15","remainder_kg":"10"}),
        )
    };
    let (one, two) = tokio::join!(
        store.issue_raw_material_paddon(supplier, open("open-again-1")),
        store.issue_raw_material_paddon(supplier, open("open-again-2"))
    );
    assert_ne!(one.is_ok(), two.is_ok());
    let remaining = list().await.unwrap();
    assert_eq!(remaining["paddons"][0]["remaining_kg"], "10.000000");
    let last = store
        .issue_raw_material_paddon(
            supplier,
            take(
                "last-open-bag",
                1,
                "10",
                json!({"open_unit_no":1,"expected_open_kg":"10"}),
            ),
        )
        .await
        .unwrap();
    assert_ne!(issued["labels"][0]["barcode"], last["labels"][0]["barcode"]);
    assert_eq!(list().await.unwrap()["paddons"][0]["remaining_count"], 2);
    let loose = store
        .issue_raw_material_paddon(
            supplier,
            take("unknown-split", 2, "3", json!({"remainder_kg":"4.5"})),
        )
        .await
        .unwrap();
    assert_eq!(loose["remainder_kg"], "4.500000");
    assert_eq!(
        list().await.unwrap()["paddons"][0]["remaining_kg"],
        "4.500000"
    );

    sqlx::raw_sql(r#"INSERT INTO mini_calculate_materials(id,lower_name,payload_json)
        VALUES ('paddon-input','paddon','{"id":"paddon-input","name":"Paddon","active":true}');
        INSERT INTO mini_production_maps(id,product_code,title,code,map_json,lifecycle_status)
        VALUES ('paddon-order','P','P','paddon-order','{"order_kg":10}','released');
        INSERT INTO mini_orders(id,code,order_number,product_name) VALUES ('paddon-order','paddon-order','paddon-order','P');
        INSERT INTO mini_order_products(id,order_id,product_name,layers_json)
        VALUES ('paddon-product','paddon-order','P','[{"material_id":"paddon-input","material":"Paddon","micron":"12"}]');"#)
        .execute(pool).await.unwrap();
    store
        .assign_responsibility(MaterialResponsibilityAssign {
            principal_ref: master.principal.ref_.clone(),
            material_id: "paddon-input".into(),
        })
        .await
        .unwrap();
    let consume = || ConsumptionCreate {
        request_id: "use-paddon-stock".into(),
        warehouse: "Own".into(),
        order_id: "paddon-order".into(),
        expected_order_kg: "10".into(),
        lines: vec![ConsumptionLine {
            item_code: item.into(),
            percent: "100".into(),
        }],
    };
    assert!(matches!(
        store.consume(&master.principal, consume()).await,
        Err(PreparationError::Insufficient)
    ));
    for (action, actor, key) in [
        (
            InventoryTransferActionKind::Approve,
            master,
            "approve-paddon",
        ),
        (
            InventoryTransferActionKind::Dispatch,
            supplier,
            "dispatch-paddon",
        ),
        (
            InventoryTransferActionKind::Receive,
            master,
            "receive-paddon",
        ),
    ] {
        InventoryMovementStorePort::transfer_action(
            &move_store,
            actor,
            issue_id,
            action,
            &InventoryTransferAction {
                note: String::new(),
                idempotency_key: key.into(),
            },
        )
        .await
        .unwrap();
    }
    let consumed = store.consume(&master.principal, consume()).await.unwrap();
    assert_eq!(consumed["lines"][0]["kg"], "10.000000");
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM mini_preparation_allocations a
        JOIN mini_preparation_receipts r ON a.receipt_id=r.id JOIN mini_raw_material_stock s ON s.id=r.stock_id
        WHERE s.barcode=$1 AND s.status='consumed' AND r.owner_ref=$2 AND a.kg=10")
        .bind(label).bind(&master.principal.ref_).fetch_one(pool).await.unwrap();
    assert_eq!(count, 1);
}

async fn verify_paddon_upgrade(pool: &PgPool) {
    // Exercise the upgrade with previous-format records and roll back the fixture.
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql(r#"
        INSERT INTO mini_warehouses (id,name) VALUES ('pre-upgrade','Pre-upgrade');
        INSERT INTO mini_raw_material_paddons (id,code,owner_role,owner_ref,warehouse_id,request_id,request_json)
            VALUES ('prior-pallet','XP-000000000000000F','material_taminotchi','previous-user','pre-upgrade','prior-pallet','{}');
        INSERT INTO mini_raw_material_paddon_issues(id,paddon_id,request_id,request_json,response_json) VALUES
            ('prior-issue','prior-pallet','prior-issue','{"destination":{"kind":"state","id":"floor"}}','{"labels":[{"barcode":"OLD-4"}]}');
        INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,status,reserved_order_id,payload_json)
        SELECT 'prior-'||n,'Pre-upgrade','P','P','OLD-'||n,25,CASE n WHEN 2 THEN 'reserved' WHEN 3 THEN 'consumed' ELSE 'available' END,
            CASE n WHEN 2 THEN 'old-order' ELSE '' END,'{"source":"raw_material_paddon"}'::jsonb
            FROM generate_series(1,4) n;
        INSERT INTO mini_raw_material_paddon_units(paddon_id,line_no,unit_no,stock_id,kind,lot_number,unit_kg,issue_id)
        SELECT 'prior-pallet',1,n,'prior-'||n,'bag','L',25,CASE WHEN n=4 THEN 'prior-issue' END FROM generate_series(1,4) n;
    "#).execute(&mut *tx).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/postgres/0204_raw_material_paddon_custody.sql"
    ))
    .execute(&mut *tx)
    .await
    .unwrap();
    let states: Vec<String> = sqlx::query_scalar(
        "SELECT status FROM mini_raw_material_stock WHERE id LIKE 'prior-%' ORDER BY id",
    )
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert_eq!(states, ["on_paddon", "reserved", "consumed", "available"]);
    let old: Value = sqlx::query_scalar(
        "SELECT result FROM mini_raw_material_paddon_issue_history WHERE id='prior-issue'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(old["legacy_print"], true);
    assert_eq!(old["destination_kind"], "state");
    assert_eq!(old["labels"][0]["barcode"], "OLD-4");
    let cancelled:String=sqlx::query_scalar("UPDATE mini_raw_material_stock SET status='available',reserved_order_id='' WHERE id='prior-2' RETURNING status")
        .fetch_one(&mut *tx).await.unwrap();
    assert_eq!(cancelled, "on_paddon");
    tx.rollback().await.unwrap();
}
