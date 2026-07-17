use std::collections::BTreeMap;

use data_encoding::{HEXLOWER, HEXUPPER};
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::core::chat_media::ChatMediaStorageError;

type HmacSha256 = Hmac<Sha256>;

pub(super) fn normalized_region(region: &str) -> String {
    let region = region.trim();
    if region.is_empty() {
        "auto".to_string()
    } else {
        region.to_string()
    }
}

pub(super) fn validate_object_key(object_key: &str) -> Result<(), ChatMediaStorageError> {
    if object_key.split('/').any(|part| {
        part.is_empty() || part == "." || part == ".." || part.contains('\0') || part.contains('\\')
    }) {
        Err(ChatMediaStorageError::InvalidObjectKey)
    } else {
        Ok(())
    }
}

pub(super) fn signing_dates(now: OffsetDateTime) -> (String, String) {
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

pub(super) fn request_host(url: &reqwest::Url) -> Result<String, ChatMediaStorageError> {
    url.host_str()
        .map(|host| match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        })
        .ok_or(ChatMediaStorageError::OperationFailed)
}

pub(super) fn canonical_query(values: &BTreeMap<&str, String>) -> String {
    values
        .iter()
        .map(|(key, value)| format!("{}={}", aws_encode(key), aws_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

pub(super) fn aws_encode(value: &str) -> String {
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

pub(super) fn insert_header(
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

pub(super) fn optional_header(headers: &HeaderMap, key: HeaderName) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_matches('"').trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    HEXLOWER.encode(&Sha256::digest(bytes))
}

pub(super) fn signing_key(
    secret: &str,
    date: &str,
    region: &str,
) -> Result<Vec<u8>, ChatMediaStorageError> {
    let date_key = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes())?;
    let region_key = hmac_sha256(&date_key, region.as_bytes())?;
    let service_key = hmac_sha256(&region_key, b"s3")?;
    hmac_sha256(&service_key, b"aws4_request")
}

pub(super) fn hmac_sha256(
    key: &[u8],
    bytes: &[u8],
) -> Result<Vec<u8>, ChatMediaStorageError> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|_| ChatMediaStorageError::OperationFailed)?;
    mac.update(bytes);
    Ok(mac.finalize().into_bytes().to_vec())
}
