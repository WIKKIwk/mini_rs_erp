use std::path::{Path, PathBuf};

use async_trait::async_trait;
use data_encoding::HEXLOWER;
use sha2::{Digest, Sha256};
use tokio::fs;

use crate::core::profile::ports::{
    DownloadedFile, ProfileAvatarStorage, ProfilePortError, StoredProfileAvatar,
};

#[derive(Clone)]
pub struct LocalProfileAvatarStorage {
    root: PathBuf,
}

impl LocalProfileAvatarStorage {
    pub fn from_env() -> Self {
        let root = std::env::var("MOBILE_PROFILE_AVATAR_STORE_DIR")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "data/profile_avatars".to_string());
        Self::new(root)
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for_key(&self, object_key: &str) -> Result<PathBuf, ProfilePortError> {
        let mut path = self.root.clone();
        for part in object_key.split('/') {
            if part.is_empty() || part == "." || part == ".." || part.contains('\\') {
                return Err(ProfilePortError::LookupFailed);
            }
            path.push(part);
        }
        Ok(path)
    }
}

#[async_trait]
impl ProfileAvatarStorage for LocalProfileAvatarStorage {
    async fn put_profile_avatar(
        &self,
        role: &str,
        principal_ref: &str,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<StoredProfileAvatar, ProfilePortError> {
        let object_key = avatar_object_key(role, principal_ref, filename, &content);
        let path = self.path_for_key(&object_key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|_| ProfilePortError::LookupFailed)?;
        }
        fs::write(&path, content)
            .await
            .map_err(|_| ProfilePortError::LookupFailed)?;
        fs::write(
            content_type_path(&path),
            normalize_content_type(content_type, filename),
        )
        .await
        .map_err(|_| ProfilePortError::LookupFailed)?;
        Ok(StoredProfileAvatar {
            object_key: object_key.clone(),
            public_url: format!("local://{object_key}"),
        })
    }

    async fn get_profile_avatar(
        &self,
        object_key: &str,
    ) -> Result<DownloadedFile, ProfilePortError> {
        let path = self.path_for_key(object_key)?;
        let body = fs::read(&path)
            .await
            .map_err(|_| ProfilePortError::LookupFailed)?;
        let content_type = fs::read_to_string(content_type_path(&path))
            .await
            .unwrap_or_else(|_| "image/jpeg".to_string());
        Ok(DownloadedFile {
            content_type: content_type.trim().to_string(),
            body,
        })
    }
}

fn content_type_path(path: &Path) -> PathBuf {
    let mut out = path.as_os_str().to_os_string();
    out.push(".content-type");
    PathBuf::from(out)
}

fn avatar_object_key(role: &str, principal_ref: &str, filename: &str, content: &[u8]) -> String {
    let role = safe_path_part(role);
    let principal_ref = safe_path_part(principal_ref);
    let hash = HEXLOWER.encode(&Sha256::digest(content));
    let hash = &hash[..16];
    let extension = avatar_extension(filename);
    format!("profile_avatars/{role}/{principal_ref}/{hash}.{extension}")
}

fn safe_path_part(value: &str) -> String {
    let mut out = String::new();
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '_' | '-') {
            out.push(ch);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "profile".to_string()
    } else {
        out
    }
}

fn avatar_extension(filename: &str) -> &'static str {
    match filename
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "png",
        "webp" => "webp",
        "jpg" | "jpeg" => "jpg",
        _ => "jpg",
    }
}

fn normalize_content_type(content_type: &str, filename: &str) -> String {
    match content_type.trim().to_ascii_lowercase().as_str() {
        "image/png" => "image/png".to_string(),
        "image/webp" => "image/webp".to_string(),
        "image/jpeg" | "image/jpg" => "image/jpeg".to_string(),
        _ => match avatar_extension(filename) {
            "png" => "image/png".to_string(),
            "webp" => "image/webp".to_string(),
            _ => "image/jpeg".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::LocalProfileAvatarStorage;
    use crate::core::profile::ports::ProfileAvatarStorage;

    #[tokio::test]
    async fn stores_and_loads_avatar_by_object_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = LocalProfileAvatarStorage::new(dir.path());

        let stored = storage
            .put_profile_avatar(
                "Werka",
                " worker/1 ",
                "avatar.jpg",
                "image/jpeg",
                b"jpg".to_vec(),
            )
            .await
            .expect("store avatar");
        let file = storage
            .get_profile_avatar(&stored.object_key)
            .await
            .expect("load avatar");

        assert_eq!(
            stored.object_key,
            "profile_avatars/werka/worker_1/f8146b6cb4961f87.jpg"
        );
        assert_eq!(stored.public_url, format!("local://{}", stored.object_key));
        assert_eq!(file.content_type, "image/jpeg");
        assert_eq!(file.body, b"jpg".to_vec());
    }
}
