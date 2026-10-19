//! Transactionally copy LMDB; copying a live data.mdb with cp/tar is unsafe.
use std::fs;
use std::path::Path;

use heed::{CompactionOption, EnvFlags, EnvOpenOptions};

fn snapshot(source: &Path, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if destination.exists() {
        return Err("LMDB snapshot destination already exists".into());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    // The helper is a separate process, so it cannot conflict with the server's
    // cached Env handles. READ_ONLY still uses LMDB's reader lock table.
    let bytes = usize::try_from(fs::metadata(source.join("data.mdb"))?.len())?;
    let map_size = bytes
        .checked_next_power_of_two()
        .ok_or("LMDB file is too large")?
        .max(64 * 1024 * 1024);
    let env = unsafe {
        EnvOpenOptions::new()
            .map_size(map_size)
            .max_dbs(128)
            .flags(EnvFlags::READ_ONLY)
            .open(source)?
    };
    copy_environment(&env, destination)
}

fn copy_environment(env: &heed::Env, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let copy = env.copy_to_path(destination, CompactionOption::Enabled)?;
    copy.sync_all()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() == 2 && args[0] == "--environment" {
        let path = Path::new(&args[1]).join(".env");
        if path.is_file() {
            dotenvy::from_path(path)?;
        }
        let values = std::env::vars().collect::<std::collections::BTreeMap<_, _>>();
        println!("{}", serde_json::to_string(&values)?);
        return Ok(());
    }
    if args.len() != 2 {
        return Err("usage: mini_rs_backup_state SOURCE_LMDB DESTINATION_DATA_MDB".into());
    }
    snapshot(Path::new(&args[0]), Path::new(&args[1]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use heed::types::Str;

    #[test]
    fn snapshot_keeps_committed_sessions_independent_of_later_writes() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original");
        let recovered = root.path().join("recovered");
        fs::create_dir(&original).unwrap();
        let env = unsafe { EnvOpenOptions::new().max_dbs(128).open(&original).unwrap() };
        let mut txn = env.write_txn().unwrap();
        let db = env
            .create_database::<Str, Str>(&mut txn, Some("sessions"))
            .unwrap();
        db.put(&mut txn, "token", "original-worker").unwrap();
        txn.commit().unwrap();
        fs::create_dir(&recovered).unwrap();
        copy_environment(&env, &recovered.join("data.mdb")).unwrap();
        let mut txn = env.write_txn().unwrap();
        db.put(&mut txn, "token", "later-worker").unwrap();
        txn.commit().unwrap();
        let restored = unsafe { EnvOpenOptions::new().max_dbs(128).open(&recovered).unwrap() };
        let txn = restored.read_txn().unwrap();
        let db = restored
            .open_database::<Str, Str>(&txn, Some("sessions"))
            .unwrap()
            .unwrap();
        assert_eq!(db.get(&txn, "token").unwrap(), Some("original-worker"));
    }
}
