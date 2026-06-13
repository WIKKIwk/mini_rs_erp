use std::sync::Arc;

use async_trait::async_trait;

use super::ports::{
    CustomerProfileRecord, DownloadedFile, ProfileLookup, ProfilePortError, SupplierProfileRecord,
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
