use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use mini_rs_erp::core::production_map::{MixedStageBackfillManifest, ProductionMapService};
use mini_rs_erp::db::postgres::PostgresConfig;
use mini_rs_erp::db::postgres_production_map::PostgresProductionMapStore;

struct Arguments {
    manifest: PathBuf,
    apply: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let arguments = parse_arguments(std::env::args().skip(1))?;
    let manifest: MixedStageBackfillManifest = serde_json::from_str(
        &std::fs::read_to_string(&arguments.manifest)
            .map_err(|error| io::Error::other(format!("cannot read manifest: {error}")))?,
    )
    .map_err(|error| io::Error::other(format!("invalid manifest JSON: {error}")))?;

    let config = PostgresConfig::from_env().map_err(|_| {
        io::Error::other("MINI_ERP_DATABASE_URL is required; no migration is run by this tool")
    })?;
    let pool = config
        .pool_options()
        .connect(&config.database_url)
        .await
        .map_err(|error| io::Error::other(format!("database connection failed: {error}")))?;
    let service =
        ProductionMapService::new(Arc::new(PostgresProductionMapStore::new(pool.clone())));
    let plan = service
        .plan_mixed_stage_backfill(&manifest)
        .await
        .map_err(|error| io::Error::other(format!("backfill validation failed: {error}")))?;

    if arguments.apply {
        let report = service
            .apply_mixed_stage_backfill(&plan)
            .await
            .map_err(|error| io::Error::other(format!("backfill apply failed: {error}")))?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "mode": "apply",
                "report": report,
            }))?
        );
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "mode": "dry_run",
                "plan": plan,
            }))?
        );
    }
    pool.close().await;
    Ok(())
}

fn parse_arguments(arguments: impl Iterator<Item = String>) -> Result<Arguments, Box<dyn Error>> {
    let mut manifest = None;
    let mut apply = false;
    let mut dry_run = false;
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--manifest" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| io::Error::other("--manifest requires a JSON file path"))?;
                if value.trim().is_empty() {
                    return Err(io::Error::other("--manifest path must not be empty").into());
                }
                manifest = Some(PathBuf::from(value));
            }
            "--apply" => apply = true,
            "--dry-run" => dry_run = true,
            "--help" | "-h" => {
                println!(
                    "Usage: mini_rs_mixed_stage_backfill --manifest FILE [--dry-run | --apply]\n\nDefaults to dry-run; --apply is the only write mode. The tool never runs migrations."
                );
                std::process::exit(0);
            }
            value => return Err(io::Error::other(format!("unknown argument: {value}")).into()),
        }
    }
    if apply && dry_run {
        return Err(io::Error::other("--apply and --dry-run cannot be combined").into());
    }
    Ok(Arguments {
        manifest: manifest.ok_or_else(|| io::Error::other("--manifest is required"))?,
        apply,
    })
}
