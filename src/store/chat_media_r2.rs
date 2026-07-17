use std::collections::BTreeMap;
use std::path::Path;

use async_trait::async_trait;
use bytes::Bytes;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, ETAG};
use reqwest::Method;
use time::OffsetDateTime;
use tokio::sync::mpsc;

use super::chat_media_r2_signing::{normalized_region, optional_header};
use super::chat_media_r2_transfer::{
    complete_multipart_xml, env, file_stream, validate_multipart_parts, write_download,
    xml_tag, xml_unescape,
};
use crate::core::chat_media::{
    ChatMediaByteStream, ChatMediaMultipartUpload, ChatMediaStorage,
    ChatMediaStorageDownload, ChatMediaStorageError, ChatMediaStorageObject,
    ChatMediaStoragePart, ChatMediaStorageUpload, ChatMediaStoredContent,
};

#[derive(Clone)]
pub struct R2ChatMediaStorage {
    pub(super) endpoint: String,
    pub(super) bucket: String,
    pub(super) access_key_id: String,
    pub(super) secret_access_key: String,
    pub(super) region: String,
    pub(super) upload_url_ttl_seconds: i64,
    client: reqwest::Client,
}

impl R2ChatMediaStorage {
    pub fn new(config: R2ChatMediaConfig) -> Self {
        Self {
            endpoint: config.endpoint.trim().trim_end_matches('/').to_string(),
            bucket: config.bucket.trim().to_string(),
            access_key_id: config.access_key_id.trim().to_string(),
            secret_access_key: config.secret_access_key.trim().to_string(),
            region: normalized_region(&config.region),
            upload_url_ttl_seconds: config.upload_url_ttl_seconds.clamp(60, 900),
            client: config.client,
        }
    }

    pub fn from_env(timeout: std::time::Duration) -> Option<Self> {
        let endpoint = env("MOBILE_CHAT_MEDIA_R2_ENDPOINT")?;
        let bucket = env("MOBILE_CHAT_MEDIA_R2_BUCKET")?;
        let access_key_id = env("MOBILE_CHAT_MEDIA_R2_ACCESS_KEY_ID")?;
        let secret_access_key = env("MOBILE_CHAT_MEDIA_R2_SECRET_ACCESS_KEY")?;
        let region = env("MOBILE_CHAT_MEDIA_R2_REGION").unwrap_or_else(|| "auto".to_string());
        let upload_url_ttl_seconds = env("MOBILE_CHAT_MEDIA_UPLOAD_URL_TTL_SECONDS")
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(300);
        let operation_timeout = env("MOBILE_CHAT_MEDIA_R2_TIMEOUT_SECONDS")
            .and_then(|value| value.parse::<u64>().ok())
            .map(|seconds| std::time::Duration::from_secs(seconds.clamp(30, 7_200)))
            .unwrap_or_else(|| timeout.max(std::time::Duration::from_secs(3_600)));
        let client = reqwest::Client::builder()
            .timeout(operation_timeout)
            .build()
            .ok()?;
        Some(Self::new(R2ChatMediaConfig {
            endpoint,
            bucket,
            access_key_id,
            secret_access_key,
            region,
            upload_url_ttl_seconds,
            client,
        }))
    }
}

pub struct R2ChatMediaConfig {
    pub endpoint: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
    pub upload_url_ttl_seconds: i64,
    pub client: reqwest::Client,
}

#[async_trait]
impl ChatMediaStorage for R2ChatMediaStorage {
    async fn prepare_upload(
        &self,
        object_key: &str,
        content_type: &str,
        expected_size_bytes: i64,
    ) -> Result<ChatMediaStorageUpload, ChatMediaStorageError> {
        if expected_size_bytes <= 0 || content_type.trim().is_empty() {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        let now = OffsetDateTime::now_utc();
        let mut headers = BTreeMap::new();
        headers.insert(
            "content-type".to_string(),
            content_type.trim().to_ascii_lowercase(),
        );
        Ok(ChatMediaStorageUpload::DirectPut {
            url: self.presigned_put_url(object_key, content_type, now)?,
            headers,
            expires_at_unix: now.unix_timestamp() + self.upload_url_ttl_seconds,
        })
    }

    async fn put_object(
        &self,
        _object_key: &str,
        _content_type: &str,
        _expected_size_bytes: i64,
        _stream: ChatMediaByteStream,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        Err(ChatMediaStorageError::DirectUploadRequired)
    }

    async fn begin_multipart_upload(
        &self,
        object_key: &str,
        content_type: &str,
    ) -> Result<ChatMediaMultipartUpload, ChatMediaStorageError> {
        if content_type.trim().is_empty() {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        let query = BTreeMap::from([("uploads", String::new())]);
        let (url, headers) = self.signed_operation(
            "POST",
            object_key,
            &query,
            Some(content_type),
            b"",
            OffsetDateTime::now_utc(),
        )?;
        let response = self
            .client
            .request(Method::POST, url)
            .headers(headers)
            .send()
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ChatMediaStorageError::ObjectNotFound);
        }
        if !response.status().is_success() {
            return Err(ChatMediaStorageError::OperationFailed);
        }
        let body = response
            .text()
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        let storage_upload_id = xml_tag(&body, "UploadId")
            .map(xml_unescape)
            .filter(|value| !value.trim().is_empty())
            .ok_or(ChatMediaStorageError::OperationFailed)?;
        Ok(ChatMediaMultipartUpload {
            storage_upload_id,
        })
    }

    async fn put_multipart_part(
        &self,
        object_key: &str,
        storage_upload_id: &str,
        part_number: i32,
        content: Bytes,
    ) -> Result<ChatMediaStoragePart, ChatMediaStorageError> {
        if storage_upload_id.trim().is_empty() || part_number <= 0 || content.is_empty() {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        let query = BTreeMap::from([
            ("partNumber", part_number.to_string()),
            ("uploadId", storage_upload_id.to_string()),
        ]);
        let (url, headers) = self.signed_operation(
            "PUT",
            object_key,
            &query,
            None,
            &content,
            OffsetDateTime::now_utc(),
        )?;
        let response = self
            .client
            .request(Method::PUT, url)
            .headers(headers)
            .body(content.clone())
            .send()
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        if !response.status().is_success() {
            return Err(ChatMediaStorageError::OperationFailed);
        }
        let etag = optional_header(response.headers(), ETAG)
            .ok_or(ChatMediaStorageError::OperationFailed)?;
        Ok(ChatMediaStoragePart {
            part_number,
            size_bytes: i64::try_from(content.len())
                .map_err(|_| ChatMediaStorageError::SizeMismatch)?,
            etag,
        })
    }

    async fn complete_multipart_upload(
        &self,
        object_key: &str,
        _content_type: &str,
        storage_upload_id: &str,
        expected_size_bytes: i64,
        parts: &[ChatMediaStoragePart],
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        validate_multipart_parts(parts, expected_size_bytes)?;
        let body = complete_multipart_xml(parts);
        let query = BTreeMap::from([("uploadId", storage_upload_id.to_string())]);
        let (url, headers) = self.signed_operation(
            "POST",
            object_key,
            &query,
            Some("application/xml"),
            body.as_bytes(),
            OffsetDateTime::now_utc(),
        )?;
        let response = self
            .client
            .request(Method::POST, url)
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ChatMediaStorageError::ObjectNotFound);
        }
        if !response.status().is_success() {
            return Err(ChatMediaStorageError::OperationFailed);
        }
        let response_body = response
            .text()
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        if response_body.contains("<Error>") {
            return Err(ChatMediaStorageError::OperationFailed);
        }
        let object = self.object_metadata(object_key).await?;
        if object.size_bytes != expected_size_bytes {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        Ok(object)
    }

    async fn abort_multipart_upload(
        &self,
        object_key: &str,
        storage_upload_id: &str,
    ) -> Result<(), ChatMediaStorageError> {
        let query = BTreeMap::from([("uploadId", storage_upload_id.to_string())]);
        let (url, headers) = self.signed_operation(
            "DELETE",
            object_key,
            &query,
            None,
            b"",
            OffsetDateTime::now_utc(),
        )?;
        let response = self
            .client
            .request(Method::DELETE, url)
            .headers(headers)
            .send()
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(ChatMediaStorageError::OperationFailed)
        }
    }

    async fn object_metadata(
        &self,
        object_key: &str,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        let response = self
            .client
            .head(self.object_url(object_key)?)
            .headers(self.signed_request_headers("HEAD", object_key, OffsetDateTime::now_utc())?)
            .send()
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ChatMediaStorageError::ObjectNotFound);
        }
        if !response.status().is_success() {
            return Err(ChatMediaStorageError::OperationFailed);
        }
        let size_bytes = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|value| *value > 0)
            .ok_or(ChatMediaStorageError::OperationFailed)?;
        let content_type = optional_header(response.headers(), CONTENT_TYPE);
        let etag = optional_header(response.headers(), ETAG);
        Ok(ChatMediaStorageObject {
            size_bytes,
            content_type,
            etag,
        })
    }

    async fn delete_object(&self, object_key: &str) -> Result<(), ChatMediaStorageError> {
        let response = self
            .client
            .delete(self.object_url(object_key)?)
            .headers(self.signed_request_headers("DELETE", object_key, OffsetDateTime::now_utc())?)
            .send()
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(ChatMediaStorageError::OperationFailed)
        }
    }

    async fn read_object(
        &self,
        object_key: &str,
    ) -> Result<ChatMediaStoredContent, ChatMediaStorageError> {
        let response = self
            .client
            .get(self.object_url(object_key)?)
            .headers(self.signed_request_headers("GET", object_key, OffsetDateTime::now_utc())?)
            .send()
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ChatMediaStorageError::ObjectNotFound);
        }
        if !response.status().is_success() {
            return Err(ChatMediaStorageError::OperationFailed);
        }
        let content_type = optional_header(response.headers(), CONTENT_TYPE);
        let etag = optional_header(response.headers(), ETAG);
        let bytes = response
            .bytes()
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        Ok(ChatMediaStoredContent {
            bytes,
            content_type,
            etag,
        })
    }

    async fn download_object_to_file(
        &self,
        object_key: &str,
        destination: &Path,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        let mut response = self
            .client
            .get(self.object_url(object_key)?)
            .headers(self.signed_request_headers("GET", object_key, OffsetDateTime::now_utc())?)
            .send()
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ChatMediaStorageError::ObjectNotFound);
        }
        if !response.status().is_success() {
            return Err(ChatMediaStorageError::OperationFailed);
        }
        let expected_size = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|value| *value > 0)
            .ok_or(ChatMediaStorageError::OperationFailed)?;
        let content_type = optional_header(response.headers(), CONTENT_TYPE);
        let etag = optional_header(response.headers(), ETAG);
        let (sender, receiver) = mpsc::channel(4);
        let destination = destination.to_path_buf();
        let writer = tokio::task::spawn_blocking(move || write_download(destination, receiver));
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?
        {
            if sender.send(Ok(chunk)).await.is_err() {
                return Err(ChatMediaStorageError::OperationFailed);
            }
        }
        drop(sender);
        let written = writer
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)??;
        if written != expected_size {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        Ok(ChatMediaStorageObject {
            size_bytes: written,
            content_type,
            etag,
        })
    }

    async fn put_private_object(
        &self,
        object_key: &str,
        content_type: &str,
        content: bytes::Bytes,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        if content.is_empty() {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        let response = self
            .client
            .put(self.object_url(object_key)?)
            .headers(self.signed_put_headers(
                object_key,
                content_type,
                &content,
                OffsetDateTime::now_utc(),
            )?)
            .body(content.clone())
            .send()
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        if !response.status().is_success() {
            return Err(ChatMediaStorageError::OperationFailed);
        }
        Ok(ChatMediaStorageObject {
            size_bytes: i64::try_from(content.len())
                .map_err(|_| ChatMediaStorageError::SizeMismatch)?,
            content_type: Some(content_type.trim().to_ascii_lowercase()),
            etag: optional_header(response.headers(), ETAG),
        })
    }

    async fn put_private_file(
        &self,
        object_key: &str,
        content_type: &str,
        source: &Path,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        let size_bytes = i64::try_from(
            tokio::fs::metadata(source)
                .await
                .map_err(|_| ChatMediaStorageError::OperationFailed)?
                .len(),
        )
        .map_err(|_| ChatMediaStorageError::SizeMismatch)?;
        if size_bytes <= 0 || content_type.trim().is_empty() {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        let url = self.presigned_put_url(object_key, content_type, OffsetDateTime::now_utc())?;
        let response = self
            .client
            .put(url)
            .header(CONTENT_TYPE, content_type.trim().to_ascii_lowercase())
            .header(CONTENT_LENGTH, size_bytes)
            .body(reqwest::Body::wrap_stream(file_stream(source.to_path_buf())))
            .send()
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        if !response.status().is_success() {
            return Err(ChatMediaStorageError::OperationFailed);
        }
        Ok(ChatMediaStorageObject {
            size_bytes,
            content_type: Some(content_type.trim().to_ascii_lowercase()),
            etag: optional_header(response.headers(), ETAG),
        })
    }

    async fn prepare_download(
        &self,
        object_key: &str,
    ) -> Result<ChatMediaStorageDownload, ChatMediaStorageError> {
        let now = OffsetDateTime::now_utc();
        Ok(ChatMediaStorageDownload::DirectGet {
            url: self.presigned_get_url(object_key, now)?,
            expires_at_unix: now.unix_timestamp() + self.upload_url_ttl_seconds,
        })
    }
}
