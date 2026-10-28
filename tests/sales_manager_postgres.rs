use mini_rs_erp::core::auth::models::PrincipalRole;
use mini_rs_erp::core::system_users::{SystemUser, SystemUserError, SystemUserStorePort};
use mini_rs_erp::db::postgres_system_user::PostgresSystemUserStore;
use sqlx::{PgPool, postgres::PgConnectOptions};

#[tokio::test]
#[ignore = "requires an explicitly configured disposable PostgreSQL administrator URL"]
async fn sales_manager_migration_preserves_existing_roles_and_native_identity_constraints() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .expect("disposable PostgreSQL administrator URL required");
    let admin = PgPool::connect(&url).await.unwrap();
    let db = format!("mini_rs_erp_test_sales_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {db} TEMPLATE template0"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(url.parse::<PgConnectOptions>().unwrap().database(&db))
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../migrations/postgres/0004_system_users.sql"))
        .execute(&pool)
        .await
        .unwrap();
    assert!(sqlx::query("INSERT INTO mini_system_users(id,role,name,phone) VALUES('before','sales_manager','Sales','901112292')")
        .execute(&pool).await.is_err());
    sqlx::raw_sql(include_str!(
        "../migrations/postgres/0172_sales_manager.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let store = PostgresSystemUserStore::new(pool.clone());
    for (index, role) in [
        PrincipalRole::Qolipchi,
        PrincipalRole::Boyoqchi,
        PrincipalRole::TayyorlovMasteri,
        PrincipalRole::HomashyoRezkachi,
        PrincipalRole::SalesManager,
    ]
    .into_iter()
    .enumerate()
    {
        let user = SystemUser {
            id: format!("sales-test-{index}"),
            role,
            name: format!("Role {index}"),
            phone: format!("+9989011122{index:02}"),
        };
        assert_eq!(store.upsert_user(user.clone()).await.unwrap(), user);
        assert_eq!(store.users(&role, "", 10).await.unwrap(), vec![user]);
    }
    let duplicate = store
        .upsert_user(SystemUser {
            id: "duplicate".into(),
            role: PrincipalRole::SalesManager,
            name: "Duplicate".into(),
            phone: "998901112204".into(),
        })
        .await;
    assert_eq!(duplicate, Err(SystemUserError::DuplicatePhone));
    assert!(sqlx::query("INSERT INTO mini_system_users(id,role,name,phone) VALUES('invalid','unknown_role','Invalid','901112299')")
        .execute(&pool).await.is_err());
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
