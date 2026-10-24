use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

pub const ANDROID_MANIFEST_FILE: &str = "android.json";
pub const ANDROID_APK_ROUTE_PREFIX: &str = "/v1/mobile/app-update/android/apk/";
const UPDATE_SETTINGS_FILE: &str = "update-settings.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MobileUpdateSettings {
    pub mandatory_updates_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct MobileReleaseStore {
    directory: Arc<PathBuf>,
    live: Arc<OnceLock<watch::Receiver<Option<AndroidReleaseInfo>>>>,
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
            live: Arc::new(OnceLock::new()),
        }
    }

    /// One sampler per release directory observes atomic APK publications and
    /// policy changes. All connected clients share its last valid manifest.
    pub fn subscribe_android_release(&self) -> watch::Receiver<Option<AndroidReleaseInfo>> {
        self.live
            .get_or_init(|| {
                let store = Self::new(self.directory.as_ref().clone());
                let (tx, rx) = watch::channel(None);
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
                    loop {
                        tokio::select! {
                            _ = tx.closed() => break,
                            _ = interval.tick() => {
                                let next = match store.android_release_info().await {
                                    Ok(info) => Some(info),
                                    Err(MobileReleaseError::NotPublished) => None,
                                    // An incomplete publication must not replace a
                                    // valid manifest or lift a mandatory policy.
                                    Err(_) => continue,
                                };
                                tx.send_if_modified(|current| {
                                    if *current == next { return false; }
                                    *current = next;
                                    true
                                });
                            }
                        }
                    }
                });
                rx
            })
            .clone()
    }

    async fn saved_update_settings(
        &self,
    ) -> Result<Option<MobileUpdateSettings>, MobileReleaseError> {
        match tokio::fs::read(self.directory.join(UPDATE_SETTINGS_FILE)).await {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(MobileReleaseError::InvalidSettings),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(MobileReleaseError::ReadSettings(error)),
        }
    }

    pub async fn update_settings(&self) -> Result<MobileUpdateSettings, MobileReleaseError> {
        if let Some(settings) = self.saved_update_settings().await? {
            return Ok(settings);
        }
        // Preserve published release requirements until an admin sets the switch.
        match self.android_release().await {
            Ok(release) => Ok(MobileUpdateSettings {
                mandatory_updates_enabled: release.manifest.mandatory
                    || release.manifest.minimum_supported_version_code > 0,
            }),
            Err(MobileReleaseError::NotPublished) => Ok(MobileUpdateSettings::default()),
            Err(error) => Err(error),
        }
    }

    pub async fn save_update_settings(
        &self,
        settings: MobileUpdateSettings,
    ) -> Result<MobileUpdateSettings, MobileReleaseError> {
        tokio::fs::create_dir_all(self.directory.as_ref())
            .await
            .map_err(MobileReleaseError::WriteSettings)?;
        let temporary = self.directory.join(format!(
            ".update-settings-{:032x}.tmp",
            rand::random::<u128>()
        ));
        let bytes = serde_json::to_vec(&settings).map_err(MobileReleaseError::InvalidSettings)?;
        let saved = async {
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .await?;
            file.write_all(&bytes).await?;
            file.sync_all().await?;
            tokio::fs::rename(&temporary, self.directory.join(UPDATE_SETTINGS_FILE)).await
        }
        .await;
        if let Err(error) = saved {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(MobileReleaseError::WriteSettings(error));
        }
        Ok(settings)
    }

    pub async fn android_release_info(&self) -> Result<AndroidReleaseInfo, MobileReleaseError> {
        let mut info = self.android_release().await?.public_info();
        if let Some(settings) = self.saved_update_settings().await? {
            info.mandatory = settings.mandatory_updates_enabled;
            // The admin switch is authoritative, including releases published
            // with a legacy minimum-version requirement.
            if !settings.mandatory_updates_enabled {
                info.minimum_supported_version_code = 0;
            }
        }
        Ok(info)
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    #[error("failed to read mobile update settings: {0}")]
    ReadSettings(std::io::Error),
    #[error("mobile update settings are invalid: {0}")]
    InvalidSettings(serde_json::Error),
    #[error("failed to save mobile update settings: {0}")]
    WriteSettings(std::io::Error),
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

    async fn publish(directory: &Path, version: u64) {
        tokio::fs::write(directory.join("accord.apk"), b"apk")
            .await
            .unwrap();
        let manifest = serde_json::json!({
            "version_code": version, "version_name": format!("1.0.{version}"),
            "apk_file": "accord.apk", "sha256": "a".repeat(64), "size_bytes": 3,
        });
        tokio::fs::write(directory.join(".android.tmp"), manifest.to_string())
            .await
            .unwrap();
        tokio::fs::rename(
            directory.join(".android.tmp"),
            directory.join(ANDROID_MANIFEST_FILE),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn live_subscribers_receive_publications_and_admin_policy_changes() {
        let directory = tempfile::tempdir().unwrap();
        let store = MobileReleaseStore::new(directory.path());
        let mut first = store.subscribe_android_release();
        let mut second = store.clone().subscribe_android_release();
        publish(directory.path(), 6).await;
        tokio::time::timeout(std::time::Duration::from_secs(3), first.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.borrow_and_update().as_ref().unwrap().version_code, 6);
        second.changed().await.unwrap();
        assert_eq!(second.borrow_and_update().as_ref().unwrap().version_code, 6);

        publish(directory.path(), 7).await;
        tokio::time::timeout(std::time::Duration::from_secs(3), first.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.borrow_and_update().as_ref().unwrap().version_code, 7);
        store
            .save_update_settings(MobileUpdateSettings {
                mandatory_updates_enabled: true,
            })
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), first.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(first.borrow_and_update().as_ref().unwrap().mandatory);
    }

    #[tokio::test]
    async fn invalid_publication_does_not_replace_a_valid_live_release() {
        let directory = tempfile::tempdir().unwrap();
        publish(directory.path(), 6).await;
        let store = MobileReleaseStore::new(directory.path());
        let mut release = store.subscribe_android_release();
        release.changed().await.unwrap();
        release.borrow_and_update();
        tokio::fs::write(directory.path().join(ANDROID_MANIFEST_FILE), b"incomplete")
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(1200), release.changed())
                .await
                .is_err()
        );
        assert_eq!(release.borrow().as_ref().unwrap().version_code, 6);
    }

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
