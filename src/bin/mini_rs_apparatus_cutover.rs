use std::error::Error;
use std::path::Path;

use mini_rs_erp::core::apparatus_standard::{
    LegacyCutoverDraftManifest, LegacyCutoverManifest, build_cutover_manifest,
};
use mini_rs_erp::db::postgres::canonical_apparatus_service;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let command = arguments.first().map(String::as_str).unwrap_or_default();
    let database_url = required_database_url()?;
    let pool = PgPoolOptions::new()
        .min_connections(1)
        .max_connections(1)
        .connect(&database_url)
        .await?;
    let service = canonical_apparatus_service(pool.clone());
    match command {
        "preflight" => {
            let report = service.cutover_preflight().await?;
            emit_json(&report, arguments.get(1).map(String::as_str))?;
        }
        "preview" => {
            let manifest = read_manifest(arguments.get(1))?;
            let report = service.cutover_preflight().await?;
            let resolved = service.preview_legacy_cutover(&report, manifest).await?;
            emit_json(&resolved, arguments.get(2).map(String::as_str))?;
        }
        "build" => {
            let draft = read_json::<LegacyCutoverDraftManifest>(arguments.get(1))?;
            let report = service.cutover_preflight().await?;
            let manifest = build_cutover_manifest(&report, draft)?;
            emit_json(&manifest, arguments.get(2).map(String::as_str))?;
        }
        "apply" => {
            let manifest = read_manifest(arguments.get(1))?;
            let resolved = service.apply_legacy_cutover(manifest).await?;
            emit_json(&resolved, arguments.get(2).map(String::as_str))?;
        }
        _ => {
            return Err(
                "usage: mini_rs_apparatus_cutover <preflight [output.json] | build drafts.json [manifest.json] | preview manifest.json [output.json] | apply manifest.json [output.json]>"
                    .into(),
            );
        }
    }
    pool.close().await;
    Ok(())
}

fn required_database_url() -> Result<String, Box<dyn Error>> {
    for key in ["MINI_ERP_MIGRATION_DATABASE_URL", "MINI_ERP_DATABASE_URL"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return Ok(value);
            }
        }
    }
    Err("MINI_ERP_MIGRATION_DATABASE_URL or MINI_ERP_DATABASE_URL is required".into())
}

fn read_manifest(path: Option<&String>) -> Result<LegacyCutoverManifest, Box<dyn Error>> {
    read_json(path)
}

fn read_json<T: serde::de::DeserializeOwned>(path: Option<&String>) -> Result<T, Box<dyn Error>> {
    let path = path.ok_or("manifest path is required")?;
    let bytes = std::fs::read(Path::new(path))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn emit_json(value: &impl serde::Serialize, output: Option<&str>) -> Result<(), Box<dyn Error>> {
    let bytes = serde_json::to_vec_pretty(value)?;
    if let Some(path) = output {
        std::fs::write(Path::new(path), &bytes)?;
    } else {
        println!("{}", String::from_utf8(bytes)?);
    }
    Ok(())
}
