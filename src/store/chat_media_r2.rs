use std::collections::BTreeMap;

use async_trait::async_trait;
use data_encoding::HEXLOWER;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, ETAG, HeaderMap};
use time::OffsetDateTime;

use super::chat_media_r2_signing::{
    aws_encode, canonical_query, hmac_sha256, insert_header, normalized_region,
    optional_header, request_host, sha256_hex, signing_dates, signing_key,
    validate_object_key,
};
use crate::core::chat_media::{
    ChatMediaByteStream, ChatMediaStorage, ChatMediaStorageDownload, ChatMediaStorageError,
    ChatMediaStorageObject, ChatMediaStorageUpload, ChatMediaStoredContent,
};

#[derive(Clone)]
pub struct R2ChatMediaStorage {
    endpoint: String,
    bucket: String,
    access_key_id: String,
    secret_access_key: String,
    region: String,
    upload_url_ttl_seconds: i64,
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
        let client = reqwest::Client::builder().timeout(timeout).build().ok()?;
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

    fn object_url(&self, object_key: &str) -> Result<String, ChatMediaStorageError> {
        validate_object_key(object_key)?;
        Ok(format!(
            "{}/{}/{}",
            self.endpoint,
            aws_encode(&self.bucket),
            object_key
                .split('/')
                .map(aws_encode)
                .collect::<Vec<_>>()
                .join("/")
        ))
    }

    fn presigned_put_url(
        &self,
        object_key: &str,
        content_type: &str,
        now: OffsetDateTime,
    ) -> Result<String, ChatMediaStorageError> {
        let base_url = self.object_url(object_key)?;
        let url = reqwest::Url::parse(&base_url)
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        let host = request_host(&url)?;
        let (date, amz_date) = signing_dates(now);
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let mut query = BTreeMap::new();
        query.insert("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string());
        query.insert(
            "X-Amz-Credential",
            format!("{}/{scope}", self.access_key_id),
        );
        query.insert("X-Amz-Date", amz_date.clone());
        query.insert("X-Amz-Expires", self.upload_url_ttl_seconds.to_string());
        query.insert("X-Amz-SignedHeaders", "content-type;host".to_string());
        let canonical_query = canonical_query(&query);
        let canonical_headers = format!(
            "content-type:{}\nhost:{}",
            content_type.trim().to_ascii_lowercase(),
            host
        );
        let canonical_request = format!(
            "PUT\n{}\n{}\n{}\n\ncontent-type;host\nUNSIGNED-PAYLOAD",
            url.path(),
            canonical_query,
            canonical_headers
        );
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signature = HEXLOWER.encode(&hmac_sha256(
            &signing_key(&self.secret_access_key, &date, &self.region)?,
            string_to_sign.as_bytes(),
        )?);
        Ok(format!(
            "{base_url}?{canonical_query}&X-Amz-Signature={signature}"
        ))
    }

    fn presigned_get_url(
        &self,
        object_key: &str,
        now: OffsetDateTime,
    ) -> Result<String, ChatMediaStorageError> {
        let base_url = self.object_url(object_key)?;
        let url = reqwest::Url::parse(&base_url)
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        let host = request_host(&url)?;
        let (date, amz_date) = signing_dates(now);
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let mut query = BTreeMap::new();
        query.insert("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string());
        query.insert(
            "X-Amz-Credential",
            format!("{}/{scope}", self.access_key_id),
        );
        query.insert("X-Amz-Date", amz_date.clone());
        query.insert("X-Amz-Expires", self.upload_url_ttl_seconds.to_string());
        query.insert("X-Amz-SignedHeaders", "host".to_string());
        let canonical_query = canonical_query(&query);
        let canonical_request = format!(
            "GET\n{}\n{}\nhost:{}\n\nhost\nUNSIGNED-PAYLOAD",
            url.path(), canonical_query, host
        );
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signature = HEXLOWER.encode(&hmac_sha256(
            &signing_key(&self.secret_access_key, &date, &self.region)?,
            string_to_sign.as_bytes(),
        )?);
        Ok(format!(
            "{base_url}?{canonical_query}&X-Amz-Signature={signature}"
        ))
    }

    fn signed_request_headers(
        &self,
        method: &str,
        object_key: &str,
        now: OffsetDateTime,
    ) -> Result<HeaderMap, ChatMediaStorageError> {
        let url = reqwest::Url::parse(&self.object_url(object_key)?)
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        let host = request_host(&url)?;
        let (date, amz_date) = signing_dates(now);
        let payload_hash = sha256_hex(b"");
        let canonical_headers =
            format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}");
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "{method}\n{}\n\n{}\n\n{signed_headers}\n{payload_hash}",
            url.path(),
            canonical_headers
        );
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signature = HEXLOWER.encode(&hmac_sha256(
            &signing_key(&self.secret_access_key, &date, &self.region)?,
            string_to_sign.as_bytes(),
        )?);
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key_id
        );
        let mut headers = HeaderMap::new();
        insert_header(&mut headers, "x-amz-content-sha256", &payload_hash)?;
        insert_header(&mut headers, "x-amz-date", &amz_date)?;
        insert_header(&mut headers, "authorization", &authorization)?;
        Ok(headers)
    }

    fn signed_put_headers(
        &self,
        object_key: &str,
        content_type: &str,
        content: &[u8],
        now: OffsetDateTime,
    ) -> Result<HeaderMap, ChatMediaStorageError> {
        let url = reqwest::Url::parse(&self.object_url(object_key)?)
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        let host = request_host(&url)?;
        let (date, amz_date) = signing_dates(now);
        let payload_hash = sha256_hex(content);
        let normalized_type = content_type.trim().to_ascii_lowercase();
        let canonical_headers = format!(
            "content-type:{normalized_type}\nhost:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}"
        );
        let signed_headers = "content-type;host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "PUT\n{}\n\n{}\n\n{signed_headers}\n{payload_hash}",
            url.path(), canonical_headers
        );
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signature = HEXLOWER.encode(&hmac_sha256(
            &signing_key(&self.secret_access_key, &date, &self.region)?,
            string_to_sign.as_bytes(),
        )?);
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key_id
        );
        let mut headers = HeaderMap::new();
        insert_header(&mut headers, "content-type", &normalized_type)?;
        insert_header(&mut headers, "x-amz-content-sha256", &payload_hash)?;
        insert_header(&mut headers, "x-amz-date", &amz_date)?;
        insert_header(&mut headers, "authorization", &authorization)?;
        Ok(headers)
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

fn env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
