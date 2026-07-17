use std::collections::BTreeMap;

use data_encoding::HEXLOWER;
use reqwest::header::HeaderMap;
use time::OffsetDateTime;

use super::chat_media_r2::R2ChatMediaStorage;
use super::chat_media_r2_signing::{
    aws_encode, canonical_query, hmac_sha256, insert_header, request_host, sha256_hex,
    signing_dates, signing_key, validate_object_key,
};
use crate::core::chat_media::ChatMediaStorageError;

impl R2ChatMediaStorage {
    pub(super) fn object_url(
        &self,
        object_key: &str,
    ) -> Result<String, ChatMediaStorageError> {
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

    pub(super) fn presigned_put_url(
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

    pub(super) fn presigned_get_url(
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

    pub(super) fn signed_request_headers(
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

    pub(super) fn signed_put_headers(
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

    pub(super) fn signed_operation(
        &self,
        method: &str,
        object_key: &str,
        query: &BTreeMap<&str, String>,
        content_type: Option<&str>,
        content: &[u8],
        now: OffsetDateTime,
    ) -> Result<(String, HeaderMap), ChatMediaStorageError> {
        let base_url = self.object_url(object_key)?;
        let url = reqwest::Url::parse(&base_url)
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        let host = request_host(&url)?;
        let (date, amz_date) = signing_dates(now);
        let payload_hash = sha256_hex(content);
        let canonical_query = canonical_query(query);
        let normalized_type = content_type
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase);
        let (canonical_headers, signed_headers) = match normalized_type.as_deref() {
            Some(content_type) => (
                format!(
                    "content-type:{content_type}\nhost:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}"
                ),
                "content-type;host;x-amz-content-sha256;x-amz-date",
            ),
            None => (
                format!(
                    "host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}"
                ),
                "host;x-amz-content-sha256;x-amz-date",
            ),
        };
        let canonical_request = format!(
            "{method}\n{}\n{canonical_query}\n{canonical_headers}\n\n{signed_headers}\n{payload_hash}",
            url.path()
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
        if let Some(content_type) = normalized_type.as_deref() {
            insert_header(&mut headers, "content-type", content_type)?;
        }
        insert_header(&mut headers, "x-amz-content-sha256", &payload_hash)?;
        insert_header(&mut headers, "x-amz-date", &amz_date)?;
        insert_header(&mut headers, "authorization", &authorization)?;
        let request_url = if canonical_query.is_empty() {
            base_url
        } else {
            format!("{base_url}?{canonical_query}")
        };
        Ok((request_url, headers))
    }
}
