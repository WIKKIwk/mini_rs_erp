//! Backfill known legacy codes without rotating credentials or displaying secrets.
use std::io::Read;

use mini_rs_erp::db::postgres::connect_and_migrate_required;
use mini_rs_erp::db::postgres_auth::PostgresAuthStore;
use serde::Deserialize;

#[derive(Deserialize)]
struct Candidate {
    principal_ref: String,
    code: String,
}

#[tokio::main]
async fn main() {
    let result = run().await;
    match result {
        Ok(recovered) => println!("recovered_accounts={recovered}"),
        Err(_) => {
            eprintln!("Access-code recovery failed; no credentials were printed or reset.");
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<usize, Box<dyn std::error::Error>> {
    let mut input = String::new();
    std::io::stdin()
        .take(16 * 1024 * 1024)
        .read_to_string(&mut input)?;
    let candidates: Vec<Candidate> = serde_json::from_str(&input)?;
    let pool = connect_and_migrate_required().await?;
    let store = PostgresAuthStore::new(pool.clone());
    let mut recovered = 0;
    for candidate in candidates {
        if store
            .recover_access_code(&candidate.principal_ref, &candidate.code)
            .await?
        {
            recovered += 1;
        }
    }
    pool.close().await;
    Ok(recovered)
}
