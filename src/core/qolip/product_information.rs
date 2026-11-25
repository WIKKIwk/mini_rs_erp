use std::collections::HashMap;

use bytes::Bytes;
use serde::Serialize;

use super::{QolipError, QolipService};
use crate::core::auth::models::Principal;

pub const MAX_DETAILS_CHARS: usize = 4000;
pub const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;

#[derive(Clone, Copy, Default, Serialize)]
pub struct ProductInformationStatus {
    pub revision: i64,
    pub has_content: bool,
}

#[derive(Default, Serialize)]
pub struct ProductInformation {
    pub revision: i64,
    pub details: String,
    pub attachment_name: Option<String>,
    pub attachment_size: Option<i32>,
}

pub struct ProductAttachment {
    pub name: String,
    pub content: Bytes,
}

pub enum AttachmentEdit {
    Keep,
    Remove,
    Replace(ProductAttachment),
}

pub struct ProductInformationEdit {
    pub revision: i64,
    pub details: String,
    pub attachment: AttachmentEdit,
}

impl QolipService {
    pub async fn product_information_statuses(
        &self,
        codes: &[String],
    ) -> Result<HashMap<String, ProductInformationStatus>, QolipError> {
        self.store.product_information_statuses(codes).await
    }

    pub async fn product_information(&self, code: &str) -> Result<ProductInformation, QolipError> {
        self.store.product_information(code).await
    }

    pub async fn save_product_information(
        &self,
        code: &str,
        edit: ProductInformationEdit,
        principal: &Principal,
    ) -> Result<Option<ProductInformation>, QolipError> {
        self.store
            .save_product_information(code, edit, principal)
            .await
    }

    pub async fn product_attachment(
        &self,
        code: &str,
        revision: i64,
    ) -> Result<Option<ProductAttachment>, QolipError> {
        self.store.product_attachment(code, revision).await
    }
}
