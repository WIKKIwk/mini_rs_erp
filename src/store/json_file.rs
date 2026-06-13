use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::AppError;

pub async fn read_map<T>(path: &Path) -> Result<BTreeMap<String, T>, AppError>
where
    T: DeserializeOwned,
{
    if tokio::fs::metadata(path).await.is_err() {
        return Ok(BTreeMap::new());
    }

    let raw = tokio::fs::read(path).await?;
    if raw.is_empty() {
        return Ok(BTreeMap::new());
    }

    let data = serde_json::from_slice(&raw)?;
    Ok(data)
}

pub async fn write_pretty<T>(path: &Path, value: &T) -> Result<(), AppError>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let raw = serde_json::to_vec_pretty(value)?;
    let tmp_path = path.with_extension("json.tmp");
    tokio::fs::write(&tmp_path, raw).await?;
    tokio::fs::rename(tmp_path, path).await?;

    Ok(())
}
