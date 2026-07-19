use mini_rs_erp::db::postgres::connect_and_migrate_required;

#[tokio::main]
async fn main() {
    let pool = connect_and_migrate_required()
        .await
        .unwrap_or_else(|error| panic!("database migration failed: {error}"));
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .expect("read current database");
    let migration_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mini_schema_migrations")
        .fetch_one(&pool)
        .await
        .expect("count applied migrations");
    println!("database={database} applied_migrations={migration_count}");
    pool.close().await;
}
