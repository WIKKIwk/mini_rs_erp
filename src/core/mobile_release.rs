use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const ANDROID_MANIFEST_FILE: &str = "android.json";
pub const ANDROID_APK_ROUTE_PREFIX: &str = "/v1/mobile/app-update/android/apk/";

#[derive(Debug, Clone)]
pub struct MobileReleaseStore {
    directory: Arc<PathBuf>,
}

impl MobileReleaseStore {
    pub fn from_env() -> Self {
        let directory = std::env::var("MOBILE_APP_RELEASE_DIR")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "data/mobile_releases".to_string());
        Self::new(directory)
    }

    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: Arc::new(directory.into()),
        }
    }

    pub async fn android_release(&self) -> Result<AndroidRelease, MobileReleaseError> {
        let manifest_path = self.directory.join(ANDROID_MANIFEST_FILE);
        let bytes = tokio::fs::read(&manifest_path)
            .await
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => MobileReleaseError::NotPublished,
                _ => MobileReleaseError::ReadManifest(error),
            })?;
        let manifest: AndroidReleaseManifest =
            serde_json::from_slice(&bytes).map_err(MobileReleaseError::InvalidManifest)?;
        manifest.validate()?;

        let apk_path = self.directory.join(&manifest.apk_file);
        let metadata = tokio::fs::metadata(&apk_path)
            .await
            .map_err(MobileReleaseError::ReadApk)?;
        if !metadata.is_file() {
            return Err(MobileReleaseError::ApkNotFile);
        }
        if metadata.len() != manifest.size_bytes {
            return Err(MobileReleaseError::ApkSizeMismatch {
                expected: manifest.size_bytes,
                actual: metadata.len(),
            });
        }

        Ok(AndroidRelease { manifest, apk_path })
    }

    pub async fn android_apk_path(&self, file_name: &str) -> Result<PathBuf, MobileReleaseError> {
        if !is_safe_apk_file_name(file_name) {
            return Err(MobileReleaseError::InvalidField("apk_file"));
        }
        let path = self.directory.join(file_name);
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => MobileReleaseError::ApkNotFound,
                _ => MobileReleaseError::ReadApk(error),
            })?;
        if !metadata.is_file() {
            return Err(MobileReleaseError::ApkNotFile);
        }
        Ok(path)
    }
}

#[derive(Debug, Clone)]
pub struct AndroidRelease {
    pub manifest: AndroidReleaseManifest,
    pub apk_path: PathBuf,
}

impl AndroidRelease {
    pub fn public_info(&self) -> AndroidReleaseInfo {
        AndroidReleaseInfo {
            version_code: self.manifest.version_code,
            version_name: self.manifest.version_name.clone(),
            minimum_supported_version_code: self.manifest.minimum_supported_version_code,
            mandatory: self.manifest.mandatory,
            apk_url: format!("{}{}", ANDROID_APK_ROUTE_PREFIX, self.manifest.apk_file),
            sha256: self.manifest.sha256.clone(),
            size_bytes: self.manifest.size_bytes,
            release_notes: self.manifest.release_notes.clone(),
            published_at: self.manifest.published_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AndroidReleaseManifest {
    pub version_code: u64,
    pub version_name: String,
    #[serde(default)]
    pub minimum_supported_version_code: u64,
    #[serde(default)]
    pub mandatory: bool,
    pub apk_file: String,
    pub sha256: String,
    pub size_bytes: u64,
    #[serde(default)]
    pub release_notes: String,
    #[serde(default)]
    pub published_at: String,
}

impl AndroidReleaseManifest {
    fn validate(&self) -> Result<(), MobileReleaseError> {
        if self.version_code == 0 {
            return Err(MobileReleaseError::InvalidField("version_code"));
        }
        if self.version_name.trim().is_empty() {
            return Err(MobileReleaseError::InvalidField("version_name"));
        }
        if self.minimum_supported_version_code > self.version_code {
            return Err(MobileReleaseError::InvalidField(
                "minimum_supported_version_code",
            ));
        }
        if self.size_bytes == 0 {
            return Err(MobileReleaseError::InvalidField("size_bytes"));
        }
        if !is_safe_apk_file_name(&self.apk_file) {
            return Err(MobileReleaseError::InvalidField("apk_file"));
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(MobileReleaseError::InvalidField("sha256"));
        }
        Ok(())
    }
}

fn is_safe_file_name(value: &str) -> bool {
    let path = Path::new(value);
    let mut components = path.components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn is_safe_apk_file_name(value: &str) -> bool {
    is_safe_file_name(value) && value.ends_with(".apk")
}

#[derive(Debug, Clone, Serialize)]
pub struct AndroidReleaseInfo {
    pub version_code: u64,
    pub version_name: String,
    pub minimum_supported_version_code: u64,
    pub mandatory: bool,
    pub apk_url: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub release_notes: String,
    pub published_at: String,
}

#[derive(Debug, Error)]
pub enum MobileReleaseError {
    #[error("no Android release is published")]
    NotPublished,
    #[error("failed to read Android release manifest: {0}")]
    ReadManifest(std::io::Error),
    #[error("Android release manifest is invalid: {0}")]
    InvalidManifest(serde_json::Error),
    #[error("Android release manifest field is invalid: {0}")]
    InvalidField(&'static str),
    #[error("failed to read published APK: {0}")]
    ReadApk(std::io::Error),
    #[error("published APK does not exist")]
    ApkNotFound,
    #[error("published APK path is not a file")]
    ApkNotFile,
    #[error("published APK size mismatch: expected {expected}, got {actual}")]
    ApkSizeMismatch { expected: u64, actual: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_apk_path_traversal() {
        let manifest = AndroidReleaseManifest {
            version_code: 5,
            version_name: "0.2.0".to_string(),
            minimum_supported_version_code: 4,
            mandatory: false,
            apk_file: "../accord.apk".to_string(),
            sha256: "a".repeat(64),
            size_bytes: 4,
            release_notes: String::new(),
            published_at: String::new(),
        };

        assert!(matches!(
            manifest.validate(),
            Err(MobileReleaseError::InvalidField("apk_file"))
        ));
    }
}
