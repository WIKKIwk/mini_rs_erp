use std::io::Read;
use std::path::PathBuf;

use mini_rs_erp::config::DotEnvPersister;
use mini_rs_erp::core::admin::ports::{AdminReadPort, AdminStatePort};
use mini_rs_erp::db::postgres::connect_and_migrate_required;
use mini_rs_erp::db::postgres_auth::{LegacyBuiltinCredential, PostgresAuthStore};
use mini_rs_erp::store::admin_store::JsonAdminStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help") {
        println!(
            "Stop the old ERP server before cutover.\nmini_rs_auth_migrate [--admin-phone PHONE] [--admin-code-stdin]\nmini_rs_auth_migrate --reset-admin --admin-code-stdin\nThe migration imports legacy .env/JSON access codes once, then removes their local copies.\nUse stdin for the admin code; never put a code on the command line."
        );
        return Ok(());
    }
    let mut phone_override = None;
    let mut reset = false;
    let mut read_code = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--admin-phone" => {
                index += 1;
                phone_override = Some(args.get(index).ok_or("missing admin phone")?.clone());
            }
            "--admin-code-stdin" => read_code = true,
            "--reset-admin" => reset = true,
            _ => return Err("unknown option; use --help".into()),
        }
        index += 1;
    }
    // Import and scrub the same explicit file; never discover a parent's .env.
    if std::path::Path::new(".env").exists() {
        dotenvy::from_path(".env").map_err(|_| "cannot parse local .env")?;
    }
    let mut admin_code = String::new();
    if read_code {
        std::io::stdin()
            .take(1026)
            .read_to_string(&mut admin_code)?;
        admin_code = admin_code.trim().to_string();
        if admin_code.is_empty() || admin_code.len() > 1024 {
            return Err("invalid admin code length".into());
        }
    } else if reset {
        return Err("admin reset requires --admin-code-stdin".into());
    } else {
        admin_code = env_value("ADMINKA_CODE");
    }
    let pool = connect_and_migrate_required().await?;
    let store = PostgresAuthStore::new(pool);
    if reset {
        store.reset_admin_code(admin_code).await?;
        println!("Admin credential updated. Existing sessions are unchanged.");
        return Ok(());
    }
    let path = std::env::var_os("MOBILE_API_ADMIN_STORE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| "data/mobile_admin_store.json".into());
    let legacy = JsonAdminStore::try_new(path)?;
    let mut builtins = Vec::new();
    let admin_phone = phone_override.unwrap_or_else(|| env_value("ADMINKA_PHONE"));
    if !admin_phone.is_empty() || !admin_code.is_empty() {
        builtins.push(LegacyBuiltinCredential {
            principal_ref: "admin".into(),
            phone: admin_phone,
            name: std::env::var("ADMINKA_NAME").unwrap_or_else(|_| "Admin".into()),
            code: admin_code,
        });
    }
    for (ref_, phone_key, name_key, code_key) in [
        (
            "werka",
            "WERKA_PHONE",
            "MOBILE_DEV_WERKA_NAME",
            "MOBILE_DEV_WERKA_CODE",
        ),
        (
            "material_taminotchi",
            "MOBILE_DEV_MATERIAL_TAMINOTCHI_PHONE",
            "MOBILE_DEV_MATERIAL_TAMINOTCHI_NAME",
            "MOBILE_DEV_MATERIAL_TAMINOTCHI_CODE",
        ),
    ] {
        let code = env_value(code_key);
        if !code.is_empty() {
            let phone = if ref_ == "werka" && env_value(phone_key).is_empty() {
                "+99888862440".to_string()
            } else {
                env_value(phone_key)
            };
            builtins.push(LegacyBuiltinCredential {
                principal_ref: ref_.into(),
                phone,
                name: std::env::var(name_key).unwrap_or_else(|_| ref_.into()),
                code,
            });
        }
    }
    let imported = store
        .migrate_legacy(
            legacy.states().await?,
            legacy.suppliers_page("", 0, 0).await?,
            builtins,
        )
        .await?;
    store.require_ready().await?;
    // Commit first. If cleanup fails, rerunning completes cleanup without reimport.
    legacy.clear_legacy_access_secrets().await?;
    DotEnvPersister::new(".env").remove_keys(&[
        "ADMINKA_CODE",
        "MOBILE_DEV_WERKA_CODE",
        "MOBILE_DEV_MATERIAL_TAMINOTCHI_CODE",
        "ADMINKA_PHONE",
        "ADMINKA_NAME",
    ])?;
    println!(
        "Credential cutover {}. Local legacy access secrets removed.",
        if imported {
            "completed"
        } else {
            "already completed"
        }
    );
    Ok(())
}

fn env_value(key: &str) -> String {
    std::env::var(key).unwrap_or_default().trim().to_string()
}
