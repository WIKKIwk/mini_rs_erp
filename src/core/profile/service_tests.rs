use std::sync::Arc;

use async_trait::async_trait;
use image::codecs::png::PngEncoder;
use image::{ColorType, GenericImageView, ImageEncoder, Rgba, RgbaImage};
use tokio::sync::Mutex;

use super::ports::{
    CustomerProfileRecord, DownloadedFile, ProfileAvatarStorage, ProfileLookup, ProfilePortError,
    ProfilePrefs, ProfileStoreError, ProfileStorePort, StoredProfileAvatar, SupplierProfileRecord,
};
use super::service::ProfileService;
use crate::core::auth::models::{Principal, PrincipalRole};

#[tokio::test]
async fn supplier_refresh_updates_phone_and_absolute_avatar_url() {
    let service = ProfileService::new("http://files.test".to_string())
        .with_profile_lookup(Arc::new(FakeProfileLookup));

    let principal = service
        .refresh(Principal {
            role: PrincipalRole::Supplier,
            display_name: "Supplier".to_string(),
            legal_name: "Supplier".to_string(),
            ref_: "SUP-001".to_string(),
            phone: "+998900000000".to_string(),
            avatar_url: String::new(),
        })
        .await;

    assert_eq!(principal.phone, "+998901234567");
    assert_eq!(principal.avatar_url, "http://files.test/files/supplier.png");
}

#[tokio::test]
async fn customer_refresh_updates_phone() {
    let service = ProfileService::new("http://files.test".to_string())
        .with_profile_lookup(Arc::new(FakeProfileLookup));

    let principal = service
        .refresh(Principal {
            role: PrincipalRole::Customer,
            display_name: "Customer".to_string(),
            legal_name: "Customer".to_string(),
            ref_: "CUST-001".to_string(),
            phone: "+998900000000".to_string(),
            avatar_url: String::new(),
        })
        .await;

    assert_eq!(principal.phone, "+998901234568");
}

#[tokio::test]
async fn supplier_avatar_download_uses_refreshed_avatar_url() {
    let service = ProfileService::new("http://files.test".to_string())
        .with_profile_lookup(Arc::new(FakeProfileLookup));

    let file = service
        .download_avatar(Principal {
            role: PrincipalRole::Supplier,
            display_name: "Supplier".to_string(),
            legal_name: "Supplier".to_string(),
            ref_: "SUP-001".to_string(),
            phone: "+998900000000".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("download result")
        .expect("file");

    assert_eq!(file.content_type, "image/png");
    assert_eq!(file.body, b"png".to_vec());
}

#[tokio::test]
async fn worker_avatar_upload_uses_profile_avatar_storage() {
    let store = Arc::new(FakeProfileStore::default());
    let avatar_storage = Arc::new(FakeAvatarStorage::default());
    let service = ProfileService::new("http://files.test".to_string())
        .with_store(store.clone())
        .with_avatar_storage(avatar_storage.clone());

    let principal = service
        .upload_avatar(
            Principal {
                role: PrincipalRole::Werka,
                display_name: "Werka".to_string(),
                legal_name: "Werka".to_string(),
                ref_: "werka_1".to_string(),
                phone: "+998900000001".to_string(),
                avatar_url: String::new(),
            },
            "avatar.png",
            "image/png",
            test_png(1600, 800),
        )
        .await
        .expect("upload avatar");

    assert_eq!(
        principal.avatar_url,
        "https://cdn.test/profile_avatars/werka/werka_1/avatar.jpg"
    );
    assert_eq!(
        store.get("werka:werka_1").await.expect("prefs").avatar_url,
        "https://cdn.test/profile_avatars/werka/werka_1/avatar.jpg"
    );
    let call = avatar_storage.last_call.lock().await.clone().expect("call");
    assert_eq!(call.role, "werka");
    assert_eq!(call.principal_ref, "werka_1");
    assert_eq!(call.filename, "avatar.jpg");
    assert_eq!(call.content_type, "image/jpeg");
    let decoded = image::load_from_memory(&call.content).expect("canonical avatar");
    assert_eq!(decoded.dimensions(), (1000, 500));
}

#[tokio::test]
async fn refresh_exposes_local_avatar_when_only_object_key_is_stored() {
    let store = Arc::new(FakeProfileStore::default());
    store
        .put(
            "admin:admin",
            ProfilePrefs {
                nickname: String::new(),
                avatar_url: String::new(),
                avatar_object_key: "profile_avatars/admin/admin/avatar.jpg".to_string(),
            },
        )
        .await
        .expect("seed prefs");
    let service = ProfileService::new(String::new()).with_store(store);

    let principal = service
        .refresh(Principal {
            role: PrincipalRole::Admin,
            display_name: "Admin".to_string(),
            legal_name: "Admin".to_string(),
            ref_: "admin".to_string(),
            phone: "+998880000000".to_string(),
            avatar_url: String::new(),
        })
        .await;

    assert_eq!(
        principal.avatar_url,
        "local://profile_avatars/admin/admin/avatar.jpg"
    );
}

struct FakeProfileLookup;

#[async_trait]
impl ProfileLookup for FakeProfileLookup {
    async fn get_supplier_profile(
        &self,
        _id: &str,
    ) -> Result<SupplierProfileRecord, ProfilePortError> {
        Ok(SupplierProfileRecord {
            phone: "+998901234567".to_string(),
            image: "/files/supplier.png".to_string(),
        })
    }

    async fn get_customer_profile(
        &self,
        _id: &str,
    ) -> Result<CustomerProfileRecord, ProfilePortError> {
        Ok(CustomerProfileRecord {
            phone: "+998901234568".to_string(),
        })
    }

    async fn download_file(&self, file_url: &str) -> Result<DownloadedFile, ProfilePortError> {
        assert_eq!(file_url, "http://files.test/files/supplier.png");
        Ok(DownloadedFile {
            content_type: "image/png".to_string(),
            body: b"png".to_vec(),
        })
    }

    async fn upload_supplier_image(
        &self,
        _supplier_id: &str,
        _filename: &str,
        _content_type: &str,
        _content: Vec<u8>,
    ) -> Result<String, ProfilePortError> {
        Ok("/files/uploaded.png".to_string())
    }
}

#[derive(Default)]
struct FakeProfileStore {
    prefs: Mutex<std::collections::HashMap<String, ProfilePrefs>>,
}

#[async_trait]
impl ProfileStorePort for FakeProfileStore {
    async fn get(&self, key: &str) -> Result<ProfilePrefs, ProfileStoreError> {
        Ok(self
            .prefs
            .lock()
            .await
            .get(key)
            .cloned()
            .unwrap_or_default())
    }

    async fn put(&self, key: &str, prefs: ProfilePrefs) -> Result<(), ProfileStoreError> {
        self.prefs.lock().await.insert(key.to_string(), prefs);
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct AvatarStorageCall {
    role: String,
    principal_ref: String,
    filename: String,
    content_type: String,
    content: Vec<u8>,
}

#[derive(Default)]
struct FakeAvatarStorage {
    last_call: Mutex<Option<AvatarStorageCall>>,
}

#[async_trait]
impl ProfileAvatarStorage for FakeAvatarStorage {
    async fn put_profile_avatar(
        &self,
        role: &str,
        principal_ref: &str,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<StoredProfileAvatar, ProfilePortError> {
        *self.last_call.lock().await = Some(AvatarStorageCall {
            role: role.to_string(),
            principal_ref: principal_ref.to_string(),
            filename: filename.to_string(),
            content_type: content_type.to_string(),
            content,
        });
        Ok(StoredProfileAvatar {
            object_key: "profile_avatars/werka/werka_1/avatar.jpg".to_string(),
            public_url: "https://cdn.test/profile_avatars/werka/werka_1/avatar.jpg".to_string(),
        })
    }

    async fn get_profile_avatar(
        &self,
        _object_key: &str,
    ) -> Result<DownloadedFile, ProfilePortError> {
        Ok(DownloadedFile {
            content_type: "image/png".to_string(),
            body: b"pngdata".to_vec(),
        })
    }
}

fn test_png(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_pixel(width, height, Rgba([120, 80, 40, 255]));
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(image.as_raw(), width, height, ColorType::Rgba8.into())
        .expect("encode png");
    bytes
}
