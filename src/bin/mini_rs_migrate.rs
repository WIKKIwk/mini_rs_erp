use mini_rs_erp::db::postgres::{
    connect_and_migrate_required, connect_and_migrate_required_through,
};

#[tokio::main]
async fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let result = match arguments.as_slice() {
        [] => connect_and_migrate_required().await,
        [flag, target] if flag == "--through" => connect_and_migrate_required_through(target).await,
        _ => panic!("usage: mini_rs_migrate [--through <version>]"),
    };
    let pool = result.unwrap_or_else(|error| panic!("database migration failed: {error}"));
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
