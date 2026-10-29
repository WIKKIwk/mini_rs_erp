use super::*;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    canonical_apparatus_service, postgres_test_database_options,
};

struct Fixture {
    admin_url: String,
    database: String,
    admin: PgPool,
    runtime: PgPool,
}

impl Fixture {
    async fn new() -> Self {
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .expect("explicit isolated test database URL");
        let database = format!("qolip_paddons_{:032x}", rand::random::<u128>());
        let server = PgPool::connect(&admin_url).await.unwrap();
        sqlx::query(&format!("CREATE DATABASE \"{database}\""))
            .execute(&server)
            .await
            .unwrap();
        server.close().await;
        let admin = PgPool::connect_with(postgres_test_database_options(&admin_url, &database))
            .await
            .unwrap();
        apply_postgres_migrations_through_version(&admin, "0121")
            .await
            .unwrap();
        canonical_apparatus_service(admin.clone())
            .bootstrap_factory_defaults()
            .await
            .unwrap();
        apply_foundation_migration(&admin).await.unwrap();
        sqlx::query("INSERT INTO mini_item_groups(name,parent_item_group,is_group) VALUES ('Flexo','All Item Groups',false)").execute(&admin).await.unwrap();
        sqlx::query(
            "INSERT INTO mini_items(code,name,item_group) VALUES ('ITEM-R','Label R','Flexo')",
        )
        .execute(&admin)
        .await
        .unwrap();
        sqlx::query("INSERT INTO mini_warehouses(id,name,parent_warehouse) VALUES ('rw','Qolip ombor',''),('rb','Block A','Qolip ombor')")
            .execute(&admin).await.unwrap();
        sqlx::query("INSERT INTO mini_warehouse_assignments(warehouse,warehouse_name,principal_role,principal_ref,assignment_kind)
            VALUES ('Qolip ombor','Qolip ombor','qolipchi','clerk-1','warehouse')")
            .execute(&admin).await.unwrap();
        let set_id: String = sqlx::query_scalar("SELECT mini_qolip_next_set_id()")
            .fetch_one(&admin)
            .await
            .unwrap();
        for code in ["Q-ONE", "Q-TWO", "CATALOG-ONLY"] {
            sqlx::query("INSERT INTO mini_qolip_product_specs
                (item_code,item_name,item_group,qolip_code,size,created_by_role,created_by_ref,created_by_name,payload_json)
                VALUES ('ITEM-R','Label R','Flexo',$1,420,'qolipchi','clerk-1','Qolipchi',
                    jsonb_build_object('warehouse','Qolip ombor','color','BLACK','qolip_set_id',$2::text))")
                .bind(code).bind(&set_id).execute(&admin).await.unwrap();
        }
        // Legacy remains outside the new FK package model: its home receipt remains
        // an authoritative compatibility source during and after peredelka.
        for code in ["Q-ONE", "Q-TWO", "LEGACY"] {
            sqlx::query("INSERT INTO mini_qolip_locations
                (id,block,warehouse,item_code,item_name,qolip_code,size,quantity,row_letter,column_number,location_label,created_by_role,created_by_ref,created_by_name)
                VALUES ($1,'Block A','Qolip ombor','ITEM-R','Label R',$2,420,1,'A',1,'A1','qolipchi','clerk-1','Qolipchi')")
                .bind(format!("home-{code}")).bind(code).execute(&admin).await.unwrap();
        }
        let runtime = PgPool::connect_with(
            postgres_test_database_options(&admin_url, &database).username("mini_rs_erp"),
        )
        .await
        .expect("real unprivileged runtime login");
        Self {
            admin_url,
            database,
            admin,
            runtime,
        }
    }

    async fn cleanup(self) {
        self.runtime.close().await;
        self.admin.close().await;
        let server = PgPool::connect(&self.admin_url).await.unwrap();
        sqlx::query(&format!("DROP DATABASE \"{}\" WITH (FORCE)", self.database))
            .execute(&server)
            .await
            .unwrap();
        server.close().await;
    }
}

fn actor() -> Principal {
    Principal {
        role: PrincipalRole::Qolipchi,
        ref_: "clerk-1".into(),
        display_name: "Qolipchi".into(),
        legal_name: "Qolipchi".into(),
        phone: String::new(),
        avatar_url: String::new(),
    }
}

fn request(
    action: &str,
    request_id: &str,
    code: &str,
    revision: Option<i64>,
    codes: &[&str],
) -> Action {
    let mut i:Action=serde_json::from_value(json!({"action":action,"request_id":request_id,"code":code,"expected_revision":revision,"warehouse":"Qolip ombor","qolip_codes":codes})).unwrap();
    i.normalize(false).unwrap();
    i
}
async fn change(
    f: &Fixture,
    action_name: &str,
    request_id: &str,
    code: &str,
    rev: i64,
    codes: &[&str],
) -> Value {
    action(
        &f.runtime,
        &request(action_name, request_id, code, Some(rev), codes),
        false,
        &actor(),
        Some(&["Qolip ombor".into()]),
    )
    .await
    .unwrap()
}
#[tokio::test]
#[ignore = "requires isolated disposable PostgreSQL admin URL"]
async fn postgres_paddon_archive_roundtrip_retry_inspection_and_snapshots() {
    let f = Fixture::new().await;
    let allowed = vec!["Qolip ombor".to_owned()];
    let create = request("create", "create-1", "", None, &[]);
    let created = action(&f.runtime, &create, false, &actor(), Some(&allowed))
        .await
        .unwrap();
    let code = created["paddon"]["code"].as_str().unwrap();
    assert!(code.starts_with("QP-"));
    assert_eq!(
        action(&f.runtime, &create, false, &actor(), Some(&allowed))
            .await
            .unwrap(),
        created
    );
    change(&f, "add", "add-1", code, 1, &["Q-ONE", "Q-TWO"]).await;
    let stale = action(
        &f.runtime,
        &request("lock", "stale", code, Some(1), &[]),
        false,
        &actor(),
        Some(&allowed),
    )
    .await;
    assert!(matches!(
        stale,
        Err(Error::Conflict("qolip_paddon_changed"))
    ));
    change(&f, "lock", "lock-1", code, 2, &[]).await;
    assert!(matches!(
        action(
            &f.runtime,
            &request("remove", "bad-remove", code, Some(3), &["Q-ONE"]),
            false,
            &actor(),
            Some(&allowed)
        )
        .await,
        Err(Error::Conflict("qolip_paddon_locked"))
    ));
    let first = change(&f, "archive", "archive-1", code, 3, &[]).await;
    assert_eq!(first["paddon"]["archived"], true);
    let mold_view = Query {
        view: "molds".into(),
        limit: Some(1),
        ..Default::default()
    };
    let page = read(&f.runtime, "archive", &mold_view, Some(&allowed))
        .await
        .unwrap();
    let item = &page["entries"][0];
    assert_eq!(item["kind"], "paddon_mold");
    assert_eq!(item["code"], "Q-ONE");
    assert_eq!(item["snapshot"]["qolip_code"], "Q-ONE");
    assert_eq!(item["parent_paddon_code"], code);
    assert_eq!(item["id"], first["paddon"]["archive_entry_id"]);
    assert_eq!(page["has_more"], true);
    let next = read(
        &f.runtime,
        "archive",
        &Query {
            offset: 1,
            ..mold_view.clone()
        },
        Some(&allowed),
    )
    .await
    .unwrap();
    assert_eq!(next["entries"][0]["code"], "Q-TWO");
    assert_eq!(next["has_more"], false);
    let searched = read(
        &f.runtime,
        "archive",
        &Query {
            q: "Q-TWO".into(),
            ..mold_view.clone()
        },
        Some(&allowed),
    )
    .await
    .unwrap();
    assert_eq!(searched["entries"].as_array().unwrap().len(), 1);
    assert_eq!(searched["entries"][0]["code"], "Q-TWO");
    let inspection = change(
        &f,
        "inspect",
        "inspect-1",
        code,
        4,
        &["Q-ONE", "UNEXPECTED"],
    )
    .await;
    assert_eq!(inspection["inspection"]["missing"], json!(["q-two"]));
    assert_eq!(
        inspection["inspection"]["unexpected"],
        json!(["unexpected"])
    );
    let restored = change(&f, "restore", "restore-1", code, 5, &[]).await;
    assert_eq!(restored["paddon"]["locked"], true);
    assert_eq!(restored["paddon"]["archived"], false);
    change(&f, "unlock", "unlock-1", code, 6, &[]).await;
    change(&f, "remove", "remove-1", code, 7, &["Q-TWO"]).await;
    change(&f, "lock", "lock-2", code, 8, &[]).await;
    change(&f, "archive", "archive-2", code, 9, &[]).await;
    let archives = read(
        &f.runtime,
        "archive",
        &Query {
            status: "all".into(),
            ..Default::default()
        },
        Some(&allowed),
    )
    .await
    .unwrap();
    let entries = archives["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["snapshot"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(entries[1]["snapshot"]["items"].as_array().unwrap().len(), 2);
    assert!(!entries[1]["restored_at"].is_null());
    assert_eq!(
        action(
            &f.runtime,
            &request("archive", "archive-1", code, Some(3), &[]),
            false,
            &actor(),
            Some(&allowed)
        )
        .await
        .unwrap(),
        first
    );
    let latest = read(
        &f.runtime,
        "detail",
        &Query {
            code: code.into(),
            ..Default::default()
        },
        Some(&allowed),
    )
    .await
    .unwrap();
    assert_eq!(latest["paddon"]["revision"], 10);
    assert!(matches!(
        action(
            &f.runtime,
            &create,
            false,
            &actor(),
            Some(&["Other".into()])
        )
        .await,
        Err(Error::Forbidden)
    ));
    // Snapshot history and physical catalog identities are retained.
    let stock: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_qolip_locations")
        .fetch_one(&f.admin)
        .await
        .unwrap();
    assert_eq!(stock, 3);
    let mileage: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_qolip_mileage_cycles")
        .fetch_one(&f.admin)
        .await
        .unwrap();
    assert_eq!(mileage, 0);
    f.cleanup().await;
}
#[tokio::test]
#[ignore = "requires isolated disposable PostgreSQL admin URL"]
async fn postgres_standalone_archive_is_atomic_and_reversible() {
    let f = Fixture::new().await;
    let allowed = vec!["Qolip ombor".to_owned()];
    let mut input: Action = serde_json::from_value(
        json!({"action":"archive","request_id":"archive-molds","qolip_codes":["Q-ONE","Q-TWO"]}),
    )
    .unwrap();
    input.normalize(true).unwrap();
    let archived = action(&f.runtime, &input, true, &actor(), Some(&allowed))
        .await
        .unwrap();
    assert_eq!(archived["entries"].as_array().unwrap().len(), 2);
    assert_eq!(
        action(&f.runtime, &input, true, &actor(), Some(&allowed))
            .await
            .unwrap(),
        archived
    );
    let mut conflicting = input.clone();
    conflicting.note = "changed".into();
    assert!(matches!(
        action(&f.runtime, &conflicting, true, &actor(), Some(&allowed)).await,
        Err(Error::Conflict("qolip_storage_request_conflict"))
    ));
    let mut mixed = input.clone();
    mixed.request_id = "mixed".into();
    mixed.qolip_codes = vec!["catalog-only".into(), "q-one".into()];
    assert!(
        action(&f.runtime, &mixed, true, &actor(), Some(&allowed))
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_qolip_archive_entries")
        .fetch_one(&f.admin)
        .await
        .unwrap();
    assert_eq!(count, 2);
    let ids: Vec<String> = archived["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap().to_owned())
        .collect();
    let mut restore: Action = serde_json::from_value(
        json!({"action":"restore","request_id":"restore-molds","entry_ids":ids}),
    )
    .unwrap();
    restore.normalize(true).unwrap();
    let returned = action(&f.runtime, &restore, true, &actor(), Some(&allowed))
        .await
        .unwrap();
    assert!(
        returned["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| !v["restored_at"].is_null())
    );
    let mut again = input.clone();
    again.request_id = "archive-again".into();
    action(&f.runtime, &again, true, &actor(), Some(&allowed))
        .await
        .unwrap();
    let mut stale = restore.clone();
    stale.request_id = "stale-restore".into();
    assert!(matches!(
        action(&f.runtime, &stale, true, &actor(), Some(&allowed)).await,
        Err(Error::Conflict("qolip_archive_changed"))
    ));
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mini_qolip_archive_entries WHERE restored_at IS NULL",
    )
    .fetch_one(&f.admin)
    .await
    .unwrap();
    assert_eq!(count, 2);
    f.cleanup().await;
}

#[tokio::test]
#[ignore = "requires isolated disposable PostgreSQL admin URL"]
async fn postgres_concurrent_paddon_membership_and_idempotent_creation() {
    let f = Fixture::new().await;
    let allowed = vec!["Qolip ombor".to_owned()];
    let principal = actor();
    let input = request("create", "concurrent-create", "", None, &[]);
    let (one, two) = tokio::join!(
        action(&f.runtime, &input, false, &principal, Some(&allowed)),
        action(&f.runtime, &input, false, &principal, Some(&allowed))
    );
    let first = one.unwrap();
    assert_eq!(first, two.unwrap());
    let first_code = first["paddon"]["code"].as_str().unwrap();
    let other = action(
        &f.runtime,
        &request("create", "other-create", "", None, &[]),
        false,
        &principal,
        Some(&allowed),
    )
    .await
    .unwrap();
    let other_code = other["paddon"]["code"].as_str().unwrap();
    sqlx::query("UPDATE mini_qolip_locations SET quantity=2 WHERE qolip_code='Q-ONE'")
        .execute(&f.admin)
        .await
        .unwrap();
    let left = request("add", "left-add", first_code, Some(1), &["Q-ONE"]);
    let right = request("add", "right-add", other_code, Some(1), &["Q-ONE"]);
    let (left_result, right_result) = tokio::join!(
        action(&f.runtime, &left, false, &principal, Some(&allowed)),
        action(&f.runtime, &right, false, &principal, Some(&allowed))
    );
    assert_ne!(left_result.is_ok(), right_result.is_ok());
    let success = left_result.ok().or_else(|| right_result.ok()).unwrap();
    assert_eq!(success["items"][0]["quantity"], 2);
    let memberships: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mini_qolip_paddon_members WHERE removed_at IS NULL",
    )
    .fetch_one(&f.admin)
    .await
    .unwrap();
    assert_eq!(memberships, 1);
    let hidden = read(
        &f.runtime,
        "paddons",
        &Query::default(),
        Some(&["Other warehouse".into()]),
    )
    .await
    .unwrap();
    assert!(hidden["paddons"].as_array().unwrap().is_empty());
    assert!(matches!(
        read(
            &f.runtime,
            "detail",
            &Query {
                code: first_code.into(),
                ..Default::default()
            },
            Some(&["Other warehouse".into()])
        )
        .await,
        Err(Error::Forbidden)
    ));
    f.cleanup().await;
}

#[tokio::test]
#[ignore = "requires isolated disposable PostgreSQL admin URL"]
async fn postgres_archive_filters_use_tashkent_day_before_pagination() {
    let f = Fixture::new().await;
    let allowed = vec!["Qolip ombor".to_owned()];
    let mut input:Action=serde_json::from_value(json!({"action":"archive","request_id":"filter-setup","qolip_codes":["Q-ONE","Q-TWO","CATALOG-ONLY"]})).unwrap();
    input.normalize(true).unwrap();
    action(&f.runtime, &input, true, &actor(), Some(&allowed))
        .await
        .unwrap();
    for (code, at) in [
        ("Q-ONE", "2026-10-27T19:00:00Z"),
        ("Q-TWO", "2026-10-28T18:59:59Z"),
        ("CATALOG-ONLY", "2026-10-28T19:00:00Z"),
    ] {
        sqlx::query(
            "UPDATE mini_qolip_archive_entries SET archived_at=$2::text::timestamptz WHERE code=$1",
        )
        .bind(code)
        .bind(at)
        .execute(&f.admin)
        .await
        .unwrap();
    }
    let query = Query {
        from: Some("2026-10-28".into()),
        to: Some("2026-10-28".into()),
        warehouse: "qolip ombor".into(),
        limit: Some(1),
        ..Default::default()
    };
    let page = read(&f.runtime, "archive", &query, Some(&allowed))
        .await
        .unwrap();
    assert_eq!(page["entries"][0]["code"], "Q-TWO");
    assert_eq!(page["has_more"], true);
    let next = read(
        &f.runtime,
        "archive",
        &Query {
            offset: 1,
            ..query.clone()
        },
        Some(&allowed),
    )
    .await
    .unwrap();
    assert_eq!(next["entries"][0]["code"], "Q-ONE");
    assert_eq!(next["has_more"], false);
    assert!(matches!(
        read(
            &f.runtime,
            "archive",
            &Query {
                warehouse: "Other warehouse".into(),
                ..query.clone()
            },
            Some(&allowed)
        )
        .await,
        Err(Error::Forbidden)
    ));
    let next_day = read(
        &f.runtime,
        "archive",
        &Query {
            from: Some("2026-10-29".into()),
            to: Some("2026-10-29".into()),
            ..query.clone()
        },
        Some(&allowed),
    )
    .await
    .unwrap();
    assert_eq!(next_day["entries"][0]["code"], "CATALOG-ONLY");
    let set_id: String = sqlx::query_scalar(
        "SELECT qolip_set_id FROM mini_qolip_product_specs WHERE qolip_code='Q-ONE'",
    )
    .fetch_one(&f.admin)
    .await
    .unwrap();
    let search = read(
        &f.runtime,
        "molds",
        &Query {
            q: format!("  {set_id}  "),
            ..Default::default()
        },
        Some(&allowed),
    )
    .await
    .unwrap();
    assert_eq!(search["molds"].as_array().unwrap().len(), 3);
    let exact = read(
        &f.runtime,
        "molds",
        &Query {
            code: " q-One ".into(),
            ..Default::default()
        },
        Some(&allowed),
    )
    .await
    .unwrap();
    assert_eq!(exact["molds"].as_array().unwrap().len(), 1);
    assert_eq!(exact["molds"][0]["qolip_code"], "Q-ONE");
    f.cleanup().await;
}
