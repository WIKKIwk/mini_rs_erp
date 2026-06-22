use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupplierProfileRecord {
    pub phone: String,
    pub image: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CustomerProfileRecord {
    pub phone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DownloadedFile {
    pub content_type: String,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfilePrefs {
    pub nickname: String,
    pub avatar_url: String,
    #[serde(default)]
    pub avatar_object_key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredProfileAvatar {
    pub object_key: String,
    pub public_url: String,
}

#[async_trait]
pub trait ProfileLookup: Send + Sync {
    async fn get_supplier_profile(
        &self,
        id: &str,
    ) -> Result<SupplierProfileRecord, ProfilePortError>;

    async fn get_customer_profile(
        &self,
        id: &str,
    ) -> Result<CustomerProfileRecord, ProfilePortError>;

    async fn download_file(&self, file_url: &str) -> Result<DownloadedFile, ProfilePortError>;

    async fn upload_supplier_image(
        &self,
        supplier_id: &str,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<String, ProfilePortError>;
}

#[async_trait]
pub trait ProfileStorePort: Send + Sync {
    async fn get(&self, key: &str) -> Result<ProfilePrefs, ProfileStoreError>;
    async fn put(&self, key: &str, prefs: ProfilePrefs) -> Result<(), ProfileStoreError>;
}

#[async_trait]
pub trait ProfileAvatarStorage: Send + Sync {
    async fn put_profile_avatar(
        &self,
        role: &str,
        principal_ref: &str,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<StoredProfileAvatar, ProfilePortError>;

    async fn get_profile_avatar(
        &self,
        object_key: &str,
    ) -> Result<DownloadedFile, ProfilePortError>;
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum ProfilePortError {
    #[error("lookup failed")]
    LookupFailed,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileStoreError {
    #[error("store failed")]
    StoreFailed,
}
