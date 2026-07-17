use std::collections::BTreeMap;

use async_trait::async_trait;
use data_encoding::{HEXLOWER, HEXUPPER};
use hmac::{Hmac, Mac};
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, ETAG, HeaderMap, HeaderValue};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::core::chat_media::{
    ChatMediaByteStream, ChatMediaStorage, ChatMediaStorageError, ChatMediaStorageObject,
    ChatMediaStorageUpload,
};

type HmacSha256 = Hmac<Sha256>;

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
}

fn env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalized_region(region: &str) -> String {
    let region = region.trim();
    if region.is_empty() {
        "auto".to_string()
    } else {
        region.to_string()
    }
}

fn validate_object_key(object_key: &str) -> Result<(), ChatMediaStorageError> {
    if object_key.split('/').any(|part| {
        part.is_empty() || part == "." || part == ".." || part.contains('\0') || part.contains('\\')
    }) {
        Err(ChatMediaStorageError::InvalidObjectKey)
    } else {
        Ok(())
    }
}

fn signing_dates(now: OffsetDateTime) -> (String, String) {
    let date = format!(
        "{:04}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    );
    let amz_date = format!(
        "{date}T{:02}{:02}{:02}Z",
        now.hour(),
        now.minute(),
        now.second()
    );
    (date, amz_date)
}

fn request_host(url: &reqwest::Url) -> Result<String, ChatMediaStorageError> {
    url.host_str()
        .map(|host| match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        })
        .ok_or(ChatMediaStorageError::OperationFailed)
}

fn canonical_query(values: &BTreeMap<&str, String>) -> String {
    values
        .iter()
        .map(|(key, value)| format!("{}={}", aws_encode(key), aws_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn aws_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push('%');
            encoded.push_str(&HEXUPPER.encode(&[*byte]));
        }
    }
    encoded
}

fn insert_header(
    headers: &mut HeaderMap,
    key: &'static str,
    value: &str,
) -> Result<(), ChatMediaStorageError> {
    headers.insert(
        key,
        HeaderValue::from_str(value).map_err(|_| ChatMediaStorageError::OperationFailed)?,
    );
    Ok(())
}

fn optional_header(headers: &HeaderMap, key: reqwest::header::HeaderName) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_matches('"').trim().to_string())
        .filter(|value| !value.is_empty())
}

fn sha256_hex(bytes: &[u8]) -> String {
    HEXLOWER.encode(&Sha256::digest(bytes))
}

fn signing_key(
    secret: &str,
    date: &str,
    region: &str,
) -> Result<Vec<u8>, ChatMediaStorageError> {
    let date_key = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes())?;
    let region_key = hmac_sha256(&date_key, region.as_bytes())?;
    let service_key = hmac_sha256(&region_key, b"s3")?;
    hmac_sha256(&service_key, b"aws4_request")
}

fn hmac_sha256(key: &[u8], bytes: &[u8]) -> Result<Vec<u8>, ChatMediaStorageError> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|_| ChatMediaStorageError::OperationFailed)?;
    mac.update(bytes);
    Ok(mac.finalize().into_bytes().to_vec())
}
