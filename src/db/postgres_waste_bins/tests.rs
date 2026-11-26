use super::*;
use sqlx::postgres::PgPoolOptions;

const REZKA: &str = "apparatus:test:rezka";
const BOSMA: &str = "apparatus:test:bosma";
const LEGACY_STATION: &str = "apparatus:test:legacy";

fn actor(role: &str, name: &str) -> QueueActionActor {
    QueueActionActor {
        role: role.into(),
        ref_: name.into(),
        display_name: name.into(),
    }
}

async fn production_write(pool: &PgPool, entry: WasteEntry<'_>) -> Result<(), WasteBinError> {
    let mut tx = pool.begin().await?;
    record_production_waste(&mut tx, &entry).await?;
    tx.commit().await?;
    Ok(())
}

#[tokio::test]
async fn waste_bin_lifecycle() {
    let Ok(url) = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL") else {
        return;
    };
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("waste_bin_test_{}", new_id());
    sqlx::raw_sql(
        "DO $$ BEGIN
            IF to_regrole('mini_rs_erp_owner') IS NULL THEN CREATE ROLE mini_rs_erp_owner NOLOGIN; END IF;
            IF to_regrole('mini_rs_erp') IS NULL THEN CREATE ROLE mini_rs_erp NOLOGIN; END IF;
        END $$;",
    )
    .execute(&admin)
    .await
    .unwrap();
    let mut init = admin.acquire().await.unwrap();
    sqlx::raw_sql(&format!(
        "CREATE SCHEMA {schema} AUTHORIZATION mini_rs_erp_owner;
         SET search_path TO {schema};
         GRANT USAGE ON SCHEMA {schema} TO mini_rs_erp;
         CREATE TABLE mini_apparatus(id text PRIMARY KEY, name text NOT NULL);
         CREATE TABLE mini_warehouses(id text PRIMARY KEY, name text NOT NULL, is_group boolean NOT NULL DEFAULT false);
         CREATE TABLE mini_warehouse_assignments(warehouse_name text, assignment_kind text, principal_role text, principal_ref text);
         INSERT INTO mini_apparatus VALUES ('{REZKA}', 'Rezka'), ('{BOSMA}', 'Bosma'), ('{LEGACY_STATION}', 'Eski');
         GRANT SELECT ON mini_apparatus TO mini_rs_erp;
         GRANT SELECT, UPDATE ON mini_warehouses, mini_warehouse_assignments TO mini_rs_erp;",
    ))
    .execute(&mut *init)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/postgres/0208_waste_bins.sql"
    ))
    .execute(&mut *init)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/postgres/0210_waste_warehouses.sql"
    ))
    .execute(&mut *init)
    .await
    .unwrap();
    sqlx::raw_sql(&format!(
        "INSERT INTO mini_warehouses(id, name, is_waste) VALUES ('main', 'Chiqindi 1', true);
         INSERT INTO mini_warehouse_assignments
         VALUES ('Chiqindi 1', 'warehouse', 'chiqindi_qabulchi', 'receiver');
         INSERT INTO mini_waste_bins (code,tare_units,tolerance_units,registered_by_ref)
         VALUES ('OLD-EMPTY',0,0,'receiver'), ('OLD-ACTIVE',0,0,'receiver');
         INSERT INTO mini_waste_bin_cycles (id,bin_code,apparatus_id,opened_by_ref,opened_by_display_name)
         VALUES ('legacy-cycle','OLD-ACTIVE','{LEGACY_STATION}','Vali','Vali');",
    ))
    .execute(&mut *init)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/postgres/0211_waste_bin_access.sql"
    ))
    .execute(&mut *init)
    .await
    .unwrap();
    drop(init);

    let pool = PgPoolOptions::new()
        .after_connect({
            let set = format!("SET search_path TO {schema}");
            move |db, _| {
                let set = set.clone();
                Box::pin(async move {
                    sqlx::query(&set).execute(&mut *db).await?;
                    sqlx::query("SET ROLE mini_rs_erp").execute(db).await?;
                    Ok(())
                })
            }
        })
        .connect(&url)
        .await
        .unwrap();
    let store = PostgresWasteBinStore::new(pool.clone());
    let admin_actor = actor("admin", "admin");
    let ali = actor("aparatchi", "Ali");
    let vali = actor("aparatchi", "Vali");
    let receiver = actor(RECEIVER_ROLE, "receiver");
    let access = WasteBinAccessInput {
        name: "Quruq chiqindi".into(),
        apparatus_ids: vec![REZKA.into(), BOSMA.into()],
        operators: [&ali, &vali]
            .into_iter()
            .map(|actor| WasteBinOperator {
                role: actor.role.clone(),
                r#ref: actor.ref_.clone(),
            })
            .collect(),
    };

    assert_eq!(
        store.detail("OLD-EMPTY").await.unwrap()["configured"],
        false
    );
    assert_eq!(
        store.assign("OLD-EMPTY", REZKA, &ali).await,
        Err(WasteBinError::Forbidden)
    );
    let renamed = store
        .configure("OLD-EMPTY", &access, &receiver)
        .await
        .unwrap();
    assert_eq!(renamed["name"], "Quruq chiqindi");
    let operator_view = store
        .detail_for_operator("OLD-EMPTY", None, REZKA, &ali)
        .await
        .unwrap();
    assert_eq!(operator_view["name"], "Quruq chiqindi");
    assert!(operator_view.get("operators").is_none());
    assert!(
        store
            .active_for_operator(LEGACY_STATION, &ali)
            .await
            .unwrap()["bin"]
            .is_null()
    );
    assert_eq!(
        store
            .active_for_operator(LEGACY_STATION, &vali)
            .await
            .unwrap()["bin"]["name"],
        "OLD-ACTIVE"
    );
    assert_eq!(
        store
            .submit("OLD-ACTIVE", "legacy-cycle", None, &vali)
            .await
            .unwrap()["status"],
        "sealed"
    );

    assert_eq!(
        store
            .register("ACCORD-WASTE:a1", 5.0, Some(0.5), &access, &receiver)
            .await
            .unwrap()["qr_payload"],
        "ACCORD-WASTE:A1"
    );
    store
        .register("b2", 3.0, None, &access, &receiver)
        .await
        .unwrap();
    let pending = WasteEntry {
        request_id: "report-before-bin",
        apparatus_id: REZKA,
        order_id: Some("order"),
        session_id: Some("session"),
        expected_cycle_id: None,
        kg: 8.0,
        reason: "Atxot",
        actor: &vali,
        reconcile_manual: true,
    };
    production_write(&pool, pending).await.unwrap();
    let status = store.active(REZKA).await.unwrap();
    assert!(status["bin"].is_null());
    let pending_id = status["pending"][0]["id"].as_str().unwrap().to_string();

    let initial = store.assign("a1", REZKA, &ali).await.unwrap();
    assert_eq!(initial["cycle"]["apparatus_name"], "Rezka");
    let first = initial["cycle"]["id"].as_str().unwrap();
    assert_eq!(
        store.assign("b2", REZKA, &vali).await,
        Err(WasteBinError::Conflict)
    );
    assert_eq!(
        store.assign("a1", BOSMA, &vali).await,
        Err(WasteBinError::Conflict)
    );
    let manual = WasteEntry {
        request_id: "manual",
        kg: 12.0,
        actor: &ali,
        ..pending
    };
    store.add("a1", first, &manual).await.unwrap();
    assert_eq!(
        store.add("a1", first, &manual).await.unwrap()["cycle"]["expected_kg"],
        12.0
    );
    assert_eq!(
        store
            .reconcile("a1", first, std::slice::from_ref(&pending_id), &ali)
            .await,
        Err(WasteBinError::NotFound)
    );
    let combined = store
        .reconcile("a1", first, &[pending_id], &vali)
        .await
        .unwrap();
    assert_eq!(combined["cycle"]["expected_kg"], 20.0);
    assert_eq!(combined["cycle"]["entries"].as_array().unwrap().len(), 2);

    let swapped = store.submit("a1", first, Some("b2"), &ali).await.unwrap();
    assert_eq!(swapped["status"], "sealed");
    let second = swapped["replacement"]["cycle"]["id"].as_str().unwrap();
    assert_eq!(
        store.active(REZKA).await.unwrap()["bin"]["cycle"]["id"],
        second
    );
    assert_eq!(
        store.submit("a1", first, Some("b2"), &ali).await.unwrap(),
        swapped
    );
    assert_eq!(
        store
            .add(
                "a1",
                first,
                &WasteEntry {
                    request_id: "too-late",
                    ..manual
                }
            )
            .await,
        Err(WasteBinError::Conflict)
    );

    let closing = WasteEntry {
        request_id: "first-closing",
        expected_cycle_id: Some(second),
        kg: 10.0,
        ..manual
    };
    production_write(&pool, closing).await.unwrap();
    assert_eq!(
        store.active(REZKA).await.unwrap()["bin"]["cycle"]["expected_kg"],
        0.0
    );
    let closing = WasteEntry {
        request_id: "second-closing",
        kg: 4.0,
        ..closing
    };
    production_write(&pool, closing).await.unwrap();
    production_write(&pool, closing).await.unwrap();
    production_write(&pool, pending).await.unwrap();
    assert_eq!(
        store.active(REZKA).await.unwrap()["bin"]["cycle"]["expected_kg"],
        2.0
    );

    assert_eq!(
        store
            .receive("a1", first, "main", 23.0, "gscale", "", &ali)
            .await,
        Err(WasteBinError::Forbidden)
    );
    assert_eq!(
        store.reset("a1", first, &receiver).await,
        Err(WasteBinError::Conflict)
    );
    assert_eq!(
        store
            .receive("a1", first, "main", 23.0, "gscale", "", &receiver)
            .await,
        Err(WasteBinError::DiscrepancyNoteRequired)
    );
    let accepted = store
        .receive(
            "a1",
            first,
            "main",
            23.0,
            "gscale",
            "Qayta tortildi",
            &receiver,
        )
        .await
        .unwrap();
    let receipt = &accepted["cycle"]["receipt"];
    assert_eq!(receipt["net_kg"], 18.0);
    assert_eq!(receipt["difference_kg"], -2.0);
    assert_eq!(receipt["outside_tolerance"], true);
    assert_eq!(
        store
            .receive(
                "a1",
                first,
                "main",
                23.0,
                "gscale",
                "Qayta tortildi",
                &receiver
            )
            .await
            .unwrap(),
        accepted
    );
    assert_eq!(
        store
            .receive("a1", first, "main", 24.0, "manual", "Yangilash", &receiver)
            .await,
        Err(WasteBinError::Conflict)
    );
    for prohibited in [&ali, &admin_actor] {
        assert_eq!(
            store.reset("a1", first, prohibited).await,
            Err(WasteBinError::Forbidden)
        );
    }
    let reset = store.reset("a1", first, &receiver).await.unwrap();
    assert_eq!(reset["status"], "empty");
    assert_eq!(reset["cycle"]["receipt"], *receipt);
    assert_eq!(
        store.history("a1").await.unwrap()["cycles"][0]["receipt"],
        *receipt
    );
    let reopened = store.assign("a1", BOSMA, &vali).await.unwrap();
    assert_ne!(reopened["cycle"]["id"], first);
    assert_eq!(
        store.detail_cycle("a1", first).await.unwrap()["cycle"]["receipt"],
        *receipt
    );
    assert!(
        sqlx::query("UPDATE mini_waste_bin_receipts SET note='overwrite'")
            .execute(&pool)
            .await
            .is_err()
    );
    assert_eq!(
        store.list(Some("open"), None).await.unwrap()["bins"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
}
