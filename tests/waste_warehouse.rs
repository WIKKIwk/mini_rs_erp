use std::sync::Arc;

use mini_rs_erp::{
    core::{
        auth::models::PrincipalRole,
        production_map::{CanonicalServiceApparatusResolver, QueueActionActor},
        warehouses::{
            WarehouseAssignmentUpsert, WarehouseDeleteRequest, WarehouseError, WarehouseService,
            WarehouseUpsert,
        },
    },
    db::{
        postgres,
        postgres_warehouse::PostgresWarehouseStore,
        postgres_waste_bins::{
            PostgresWasteBinStore, WasteBinAccessInput, WasteBinError, WasteBinOperator, WasteEntry,
        },
    },
};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

fn actor(role: &str, name: &str) -> QueueActionActor {
    QueueActionActor {
        role: role.into(),
        ref_: name.into(),
        display_name: name.into(),
    }
}

fn bin_access(
    name: &str,
    apparatuses: &[&str],
    actors: &[&QueueActionActor],
) -> WasteBinAccessInput {
    WasteBinAccessInput {
        name: name.into(),
        apparatus_ids: apparatuses.iter().map(|id| (*id).into()).collect(),
        operators: actors
            .iter()
            .map(|actor| WasteBinOperator {
                role: actor.role.clone(),
                r#ref: actor.ref_.clone(),
            })
            .collect(),
    }
}

fn assignment(warehouse: &str, role: PrincipalRole, id: &str) -> WarehouseAssignmentUpsert {
    WarehouseAssignmentUpsert {
        assignment_kind: "warehouse".into(),
        warehouse: warehouse.into(),
        warehouse_name: None,
        apparatus_id: None,
        principal_role: role,
        principal_ref: id.into(),
        display_name: id.into(),
    }
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL admin URL in MINI_ERP_TEST_ADMIN_DATABASE_URL"]
async fn waste_warehouse_receives_actual_kg_once_and_preserves_legacy_history() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").expect("disposable PostgreSQL");
    let base = url.parse::<PgConnectOptions>().unwrap();
    let admin = PgPool::connect_with(base.clone()).await.unwrap();
    let db_name = format!("waste_warehouse_test_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {db_name}"))
        .execute(&admin)
        .await
        .unwrap();
    let options = base.database(&db_name);
    let setup = PgPool::connect_with(options.clone()).await.unwrap();
    postgres::apply_postgres_migrations_through_version(&setup, "0121")
        .await
        .unwrap();
    let apparatus = postgres::canonical_apparatus_service(setup.clone());
    apparatus.bootstrap_factory_defaults().await.unwrap();
    postgres::apply_postgres_migrations_through_version(&setup, "0210")
        .await
        .unwrap();
    sqlx::raw_sql(
        "INSERT INTO mini_waste_bins(code,tare_units,tolerance_units,registered_by_ref)
         VALUES ('OLD-EMPTY',4000000,500000,'previous-receiver'),
                ('OLD-ACTIVE',4000000,500000,'previous-receiver');
         INSERT INTO mini_waste_bin_cycles
             (id,bin_code,apparatus_id,opened_by_ref,opened_by_display_name)
         VALUES ('old-waste-cycle','OLD-ACTIVE','apparatus:default:asset-012',
                 'old-operator','Old operator');",
    )
    .execute(&setup)
    .await
    .unwrap();
    postgres::apply_foundation_migration(&setup).await.unwrap();

    // Run domain calls under the same grants as the application.
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
    let warehouses = WarehouseService::new(
        Arc::new(PostgresWarehouseStore::new(runtime.clone())),
        Arc::new(CanonicalServiceApparatusResolver::new(apparatus)),
    );
    for (name, is_waste, is_group) in [
        ("Chiqindi A", true, false),
        ("Chiqindi B", true, false),
        ("Oddiy", false, false),
        ("Guruh", false, true),
    ] {
        let saved = warehouses
            .upsert_warehouse(WarehouseUpsert {
                warehouse: name.into(),
                is_waste,
                is_group,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(saved.is_waste, is_waste);
    }
    assert_eq!(
        warehouses
            .upsert_warehouse(WarehouseUpsert {
                warehouse: "Oddiy".into(),
                is_waste: true,
                ..Default::default()
            })
            .await,
        Err(WarehouseError::TypeMismatch),
    );
    let receiver = actor("chiqindi_qabulchi", "receiver-a");
    let other = actor("chiqindi_qabulchi", "receiver-b");
    let worker = actor("aparatchi", "operator");
    for (name, id) in [("Chiqindi A", &receiver.ref_), ("Chiqindi B", &other.ref_)] {
        warehouses
            .assign_warehouse(assignment(name, PrincipalRole::ChiqindiQabulchi, id))
            .await
            .unwrap();
    }
    assert_eq!(
        warehouses
            .assign_warehouse(assignment(
                "Oddiy",
                PrincipalRole::ChiqindiQabulchi,
                &receiver.ref_
            ))
            .await,
        Err(WarehouseError::TypeMismatch)
    );
    assert_eq!(
        warehouses
            .assign_warehouse(assignment(
                "Chiqindi A",
                PrincipalRole::Werka,
                "warehouse-worker"
            ))
            .await,
        Err(WarehouseError::TypeMismatch)
    );
    let bins = PostgresWasteBinStore::new(runtime.clone());
    let visible = bins.warehouses(Some(&receiver.ref_)).await.unwrap();
    let visible = visible["warehouses"].as_array().unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0]["warehouse"], "Chiqindi A");
    let destination = visible[0]["id"].as_str().unwrap();
    let another_destination = bins.warehouses(Some(&other.ref_)).await.unwrap();
    let another_destination = another_destination["warehouses"][0]["id"].as_str().unwrap();
    let ordinary: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM mini_warehouses WHERE name IN ('Oddiy','Guruh') ORDER BY name",
    )
    .fetch_all(&setup)
    .await
    .unwrap();

    let station = "apparatus:default:asset-010";
    bins.register(
        "TEST-QUTI",
        5.0,
        Some(0.5),
        &bin_access("Ombor sinovi qutisi", &[station], &[&worker]),
        &receiver,
    )
    .await
    .unwrap();
    let active = bins.assign("TEST-QUTI", station, &worker).await.unwrap();
    let cycle = active["cycle"]["id"].as_str().unwrap();
    bins.add(
        "TEST-QUTI",
        cycle,
        &WasteEntry {
            request_id: "waste-warehouse-deposit",
            apparatus_id: station,
            order_id: None,
            session_id: None,
            expected_cycle_id: Some(cycle),
            kg: 20.0,
            reason: "Chet chiqindisi",
            actor: &worker,
            reconcile_manual: false,
        },
    )
    .await
    .unwrap();
    bins.submit("TEST-QUTI", cycle, None, &worker)
        .await
        .unwrap();

    // Wrong/missing destinations cannot leave behind an accepted measurement.
    assert_eq!(
        bins.receive(
            "TEST-QUTI",
            cycle,
            "",
            23.0,
            "manual",
            "Qayta tortildi",
            &receiver
        )
        .await,
        Err(WasteBinError::WarehouseRequired),
    );
    for forbidden in ordinary
        .iter()
        .map(String::as_str)
        .chain([another_destination])
    {
        assert_eq!(
            bins.receive(
                "TEST-QUTI",
                cycle,
                forbidden,
                23.0,
                "manual",
                "Qayta tortildi",
                &receiver
            )
            .await,
            Err(WasteBinError::WarehouseNotAssigned),
        );
    }
    assert!(bins.detail("TEST-QUTI").await.unwrap()["cycle"]["receipt"].is_null());
    for _ in 0..2 {
        let received = bins
            .receive(
                "TEST-QUTI",
                cycle,
                destination,
                23.0,
                "manual",
                "Qayta tortildi",
                &receiver,
            )
            .await
            .unwrap();
        let receipt = &received["cycle"]["receipt"];
        assert_eq!(receipt["warehouse"], "Chiqindi A");
        assert_eq!(receipt["warehouse_id"], destination);
        assert_eq!(receipt["net_kg"].as_f64(), Some(18.0));
        assert_eq!(receipt["difference_kg"].as_f64(), Some(-2.0));
    }
    assert_eq!(
        bins.reset("TEST-QUTI", cycle, &other).await,
        Err(WasteBinError::WarehouseNotAssigned)
    );
    bins.reset("TEST-QUTI", cycle, &receiver).await.unwrap();
    let history = bins.history("TEST-QUTI").await.unwrap();
    assert_eq!(history["cycles"][0]["receipt"]["warehouse"], "Chiqindi A");
    let totals = warehouses
        .warehouse_summaries("Chiqindi A", 10)
        .await
        .unwrap();
    assert!(totals[0].is_waste);
    assert_eq!(totals[0].waste_receipt_count, 1);
    assert_eq!(totals[0].waste_on_hand_kg, 18.0);
    assert_eq!(
        warehouses
            .delete_warehouse(WarehouseDeleteRequest {
                warehouse: "Chiqindi A".into(),
                delete_products: true,
            })
            .await,
        Err(WarehouseError::HasWasteReceipts),
    );

    let active = bins.assign("TEST-QUTI", station, &worker).await.unwrap();
    let cycle = active["cycle"]["id"].as_str().unwrap();
    bins.submit("TEST-QUTI", cycle, None, &worker)
        .await
        .unwrap();
    // This is the previous schema's accepted-but-unlocated receipt.
    sqlx::query(
        "INSERT INTO mini_waste_bin_receipts
         (cycle_id,gross_units,tare_units,expected_units,tolerance_units,measurement_source,
          note,received_by_ref,received_by_display_name)
         VALUES ($1,8000000,5000000,0,500000,'manual','Old receipt','old-receiver','Old receiver')",
    )
    .bind(cycle)
    .execute(&setup)
    .await
    .unwrap();
    assert!(bins.detail("TEST-QUTI").await.unwrap()["cycle"]["receipt"]["warehouse"].is_null());
    assert_eq!(
        bins.reset("TEST-QUTI", cycle, &receiver).await,
        Err(WasteBinError::WarehouseRequired)
    );
    for _ in 0..2 {
        let located = bins
            .record_legacy_destination("TEST-QUTI", cycle, destination, &receiver)
            .await
            .unwrap();
        assert_eq!(
            located["cycle"]["receipt"]["received_by_ref"],
            "old-receiver"
        );
        assert_eq!(located["cycle"]["receipt"]["warehouse"], "Chiqindi A");
    }
    bins.reset("TEST-QUTI", cycle, &receiver).await.unwrap();
    let stock = bins.warehouses(Some(&receiver.ref_)).await.unwrap();
    assert_eq!(stock["warehouses"][0]["on_hand_kg"].as_f64(), Some(21.0));
    assert_eq!(stock["warehouses"][0]["receipt_count"].as_i64(), Some(2));

    verify_legacy_bin_access(&bins, &receiver).await;
    verify_bin_race_and_scope(&bins, &receiver).await;
    runtime.close().await;
    setup.close().await;
    sqlx::query(&format!("DROP DATABASE {db_name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

async fn verify_legacy_bin_access(bins: &PostgresWasteBinStore, receiver: &QueueActionActor) {
    const APPARATUS: &str = "apparatus:default:asset-012";
    let owner = actor("aparatchi", "old-operator");
    let other = actor("aparatchi", "another-operator");
    let empty = bins.detail("OLD-EMPTY").await.unwrap();
    assert_eq!(empty["name"], "OLD-EMPTY");
    assert_eq!(
        bins.assign("OLD-EMPTY", APPARATUS, &owner).await,
        Err(WasteBinError::Forbidden),
    );
    assert!(bins.active_for_operator(APPARATUS, &other).await.unwrap()["bin"].is_null());
    assert_eq!(
        bins.active_for_operator(APPARATUS, &owner).await.unwrap()["bin"]["code"],
        "OLD-ACTIVE",
    );
    bins.submit("OLD-ACTIVE", "old-waste-cycle", None, &owner)
        .await
        .unwrap();
    let access = bin_access("Eski bo‘sh quti", &[APPARATUS], &[&owner]);
    let configured = bins
        .configure("OLD-EMPTY", &access, receiver)
        .await
        .unwrap();
    assert_eq!(configured["qr_payload"], empty["qr_payload"]);
    bins.assign("OLD-EMPTY", APPARATUS, &owner).await.unwrap();
}

async fn verify_bin_race_and_scope(bins: &PostgresWasteBinStore, receiver: &QueueActionActor) {
    const FIRST: &str = "apparatus:default:asset-010";
    const SECOND: &str = "apparatus:default:asset-011";
    let ali = actor("aparatchi", "operator-ali");
    let vali = actor("aparatchi", "operator-vali");
    let outsider = actor("aparatchi", "operator-other");
    let access = bin_access("Umumiy plyonka", &[FIRST, SECOND], &[&ali, &vali]);
    let bin = bins
        .register("SHARED", 4.0, None, &access, receiver)
        .await
        .unwrap();
    assert_eq!(bin["name"], "Umumiy plyonka");
    let access = bin_access("Plyonka qutisi", &[FIRST, SECOND], &[&ali, &vali]);
    assert_eq!(
        bins.configure("SHARED", &access, &ali).await,
        Err(WasteBinError::Forbidden),
    );
    let edited = bins.configure("SHARED", &access, receiver).await.unwrap();
    assert_eq!(edited["name"], "Plyonka qutisi");
    assert_eq!(edited["qr_payload"], bin["qr_payload"]);

    bins.register(
        "PRIVATE",
        4.0,
        None,
        &bin_access("Boshqa guruh", &[FIRST, SECOND], &[&outsider]),
        receiver,
    )
    .await
    .unwrap();
    bins.register(
        "FIRST-ONLY",
        4.0,
        None,
        &bin_access("Faqat birinchi apparat", &[FIRST], &[&ali]),
        receiver,
    )
    .await
    .unwrap();
    for (operator, apparatus, expected) in [
        (&ali, FIRST, &["FIRST-ONLY", "SHARED"][..]),
        (&ali, SECOND, &["SHARED"][..]),
        (&vali, SECOND, &["SHARED"][..]),
    ] {
        let list = bins
            .list_for_operator(Some("empty"), apparatus, operator)
            .await
            .unwrap();
        let mut codes: Vec<_> = list["bins"]
            .as_array()
            .unwrap()
            .iter()
            .map(|bin| {
                assert!(bin.get("operators").is_none());
                bin["code"].as_str().unwrap()
            })
            .collect();
        codes.sort_unstable();
        assert_eq!(codes.as_slice(), expected);
    }
    assert_eq!(
        bins.assign("FIRST-ONLY", SECOND, &ali).await,
        Err(WasteBinError::Forbidden),
    );
    assert_eq!(
        bins.assign("ACCORD-WASTE:SHARED", FIRST, &outsider).await,
        Err(WasteBinError::Forbidden),
    );
    assert!(
        bins.detail_for_operator("SHARED", None, FIRST, &outsider)
            .await
            .is_err()
    );

    let (first, second) = tokio::join!(
        bins.assign("SHARED", FIRST, &ali),
        bins.assign("SHARED", SECOND, &vali),
    );
    let (held, apparatus, holder, peer) = match (first, second) {
        (Ok(bin), Err(WasteBinError::Conflict)) => (bin, FIRST, &ali, &vali),
        (Err(WasteBinError::Conflict), Ok(bin)) => (bin, SECOND, &vali, &ali),
        unexpected => panic!("expected exactly one reservation: {unexpected:?}"),
    };
    let cycle = held["cycle"]["id"].as_str().unwrap();
    assert_eq!(
        bins.assign("SHARED", apparatus, holder).await.unwrap()["cycle"]["id"],
        cycle
    );
    assert_eq!(
        bins.assign("SHARED", apparatus, peer).await,
        Err(WasteBinError::Conflict),
    );
    assert_eq!(
        bins.submit("SHARED", cycle, Some("PRIVATE"), holder).await,
        Err(WasteBinError::Forbidden),
    );
    assert_eq!(
        bins.active_for_operator(apparatus, holder).await.unwrap()["bin"]["cycle"]["id"],
        cycle
    );
    assert!(
        bins.active_for_operator(apparatus, &outsider)
            .await
            .unwrap()["bin"]
            .is_null()
    );
    bins.register("NEXT-SHARED", 4.0, None, &access, receiver)
        .await
        .unwrap();
    let replaced = bins
        .submit("SHARED", cycle, Some("NEXT-SHARED"), holder)
        .await
        .unwrap();
    assert_eq!(replaced["replacement"]["code"], "NEXT-SHARED");
    assert_eq!(
        bins.submit("SHARED", cycle, Some("NEXT-SHARED"), peer)
            .await,
        Err(WasteBinError::Conflict),
    );
}
