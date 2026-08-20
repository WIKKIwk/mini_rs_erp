use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::AppError;

pub async fn read_map<T>(path: &Path) -> Result<BTreeMap<String, T>, AppError>
where
    T: DeserializeOwned,
{
    match tokio::fs::metadata(path).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BTreeMap::new());
        }
        Err(error) => {
            tracing::error!(path = %path.display(), %error, "failed to inspect JSON store");
            return Err(AppError::Io(error));
        }
    }

    let raw = tokio::fs::read(path).await.map_err(|error| {
        tracing::error!(path = %path.display(), %error, "failed to read JSON store");
        AppError::Io(error)
    })?;

    let data = serde_json::from_slice(&raw).map_err(|error| {
        tracing::error!(path = %path.display(), %error, "invalid JSON store snapshot");
        AppError::Json(error)
    })?;
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

#[cfg(test)]
mod tests {
    use super::read_map;
    use crate::error::AppError;

    #[tokio::test]
    async fn missing_json_store_is_the_only_empty_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = read_map::<serde_json::Value>(&dir.path().join("missing.json"))
            .await
            .expect("missing store is empty");

        assert!(data.is_empty());
    }

    #[tokio::test]
    async fn empty_json_store_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("empty.json");
        tokio::fs::write(&path, b"").await.expect("write empty store");

        assert!(matches!(
            read_map::<serde_json::Value>(&path).await,
            Err(AppError::Json(_))
        ));
    }

    #[tokio::test]
    async fn malformed_json_store_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("malformed.json");
        tokio::fs::write(&path, b"not-json")
            .await
            .expect("write malformed store");

        assert!(matches!(
            read_map::<serde_json::Value>(&path).await,
            Err(AppError::Json(_))
        ));
    }
}
