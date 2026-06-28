use async_trait::async_trait;
use data_encoding::HEXLOWER;
use hmac::{Hmac, Mac};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::core::profile::ports::{
    DownloadedFile, ProfileAvatarStorage, ProfilePortError, StoredProfileAvatar,
};

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct R2ProfileAvatarStorage {
    endpoint: String,
    bucket: String,
    access_key_id: String,
    secret_access_key: String,
    public_base_url: String,
    region: String,
    client: reqwest::Client,
}

impl R2ProfileAvatarStorage {
    pub fn new(config: R2ProfileAvatarConfig) -> Self {
        Self {
            endpoint: config.endpoint.trim().trim_end_matches('/').to_string(),
            bucket: config.bucket.trim().to_string(),
            access_key_id: config.access_key_id.trim().to_string(),
            secret_access_key: config.secret_access_key.trim().to_string(),
            public_base_url: config
                .public_base_url
                .trim()
                .trim_end_matches('/')
                .to_string(),
            region: if config.region.trim().is_empty() {
                "auto".to_string()
            } else {
                config.region.trim().to_string()
            },
            client: config.client,
        }
    }

    pub fn from_env(timeout: std::time::Duration) -> Option<Self> {
        let endpoint = env("MOBILE_PROFILE_AVATAR_R2_ENDPOINT")?;
        let bucket = env("MOBILE_PROFILE_AVATAR_R2_BUCKET")?;
        let access_key_id = env("MOBILE_PROFILE_AVATAR_R2_ACCESS_KEY_ID")?;
        let secret_access_key = env("MOBILE_PROFILE_AVATAR_R2_SECRET_ACCESS_KEY")?;
        let public_base_url = env("MOBILE_PROFILE_AVATAR_PUBLIC_BASE_URL")?;
        let region = std::env::var("MOBILE_PROFILE_AVATAR_R2_REGION")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "auto".to_string());
        let client = reqwest::Client::builder().timeout(timeout).build().ok()?;
        Some(Self::new(R2ProfileAvatarConfig {
            endpoint,
            bucket,
            access_key_id,
            secret_access_key,
            public_base_url,
            region,
            client,
        }))
    }

    fn object_url(&self, object_key: &str) -> String {
        format!(
            "{}/{}/{}",
            self.endpoint,
            self.bucket,
            object_key
                .split('/')
                .map(urlencoding::encode)
                .collect::<Vec<_>>()
                .join("/")
        )
    }

    fn signed_headers(
        &self,
        method: &str,
        object_key: &str,
        content_type: &str,
        body: &[u8],
    ) -> Result<HeaderMap, ProfilePortError> {
        let now = OffsetDateTime::now_utc();
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
        let payload_hash = sha256_hex(body);
        let url = reqwest::Url::parse(&self.object_url(object_key))
            .map_err(|_| ProfilePortError::LookupFailed)?;
        let host = url
            .host_str()
            .map(|host| match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_string(),
            })
            .ok_or(ProfilePortError::LookupFailed)?;
        let canonical_uri = url.path();
        let canonical_headers = format!(
            "content-type:{}\nhost:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
            content_type.trim(),
            host,
            payload_hash,
            amz_date
        );
        let signed_headers = "content-type;host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "{method}\n{canonical_uri}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
        );
        let credential_scope = format!("{date}/{}/s3/aws4_request", self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signature = HEXLOWER.encode(&hmac_sha256(
            &signing_key(&self.secret_access_key, &date, &self.region)?,
            string_to_sign.as_bytes(),
        )?);
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.access_key_id, credential_scope, signed_headers, signature
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(content_type.trim())
                .map_err(|_| ProfilePortError::LookupFailed)?,
        );
        headers.insert(
            "x-amz-content-sha256",
            HeaderValue::from_str(&payload_hash).map_err(|_| ProfilePortError::LookupFailed)?,
        );
        headers.insert(
            "x-amz-date",
            HeaderValue::from_str(&amz_date).map_err(|_| ProfilePortError::LookupFailed)?,
        );
        headers.insert(
            "authorization",
            HeaderValue::from_str(&authorization).map_err(|_| ProfilePortError::LookupFailed)?,
        );
        Ok(headers)
    }
}

pub struct R2ProfileAvatarConfig {
    pub endpoint: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub public_base_url: String,
    pub region: String,
    pub client: reqwest::Client,
}

#[async_trait]
impl ProfileAvatarStorage for R2ProfileAvatarStorage {
    async fn put_profile_avatar(
        &self,
        role: &str,
        principal_ref: &str,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<StoredProfileAvatar, ProfilePortError> {
        let object_key = avatar_object_key(role, principal_ref, filename, &content);
        let content_type = normalize_content_type(content_type, filename);
        let headers = self.signed_headers("PUT", &object_key, &content_type, &content)?;
        let response = self
            .client
            .put(self.object_url(&object_key))
            .headers(headers)
            .body(content)
            .send()
            .await
            .map_err(|_| ProfilePortError::LookupFailed)?;
        if !response.status().is_success() {
            return Err(ProfilePortError::LookupFailed);
        }
        Ok(StoredProfileAvatar {
            public_url: public_avatar_url(&self.public_base_url, &object_key),
            object_key,
        })
    }

    async fn get_profile_avatar(
        &self,
        object_key: &str,
    ) -> Result<DownloadedFile, ProfilePortError> {
        let headers = self.signed_headers("GET", object_key, "application/octet-stream", b"")?;
        let response = self
            .client
            .get(self.object_url(object_key))
            .headers(headers)
            .send()
            .await
            .map_err(|_| ProfilePortError::LookupFailed)?;
        if !response.status().is_success() {
            return Err(ProfilePortError::LookupFailed);
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let body = response
            .bytes()
            .await
            .map_err(|_| ProfilePortError::LookupFailed)?
            .to_vec();
        Ok(DownloadedFile { content_type, body })
    }
}

fn env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn avatar_object_key(role: &str, principal_ref: &str, filename: &str, content: &[u8]) -> String {
    let role = safe_path_part(role);
    let principal_ref = safe_path_part(principal_ref);
    let hash = sha256_hex(content);
    let hash = &hash[..16];
    let extension = avatar_extension(filename);
    format!("profile_avatars/{role}/{principal_ref}/{hash}.{extension}")
}

fn public_avatar_url(public_base_url: &str, object_key: &str) -> String {
    format!(
        "{}/{}",
        public_base_url.trim().trim_end_matches('/'),
        object_key.trim().trim_start_matches('/')
    )
}

fn safe_path_part(value: &str) -> String {
    let mut out = String::new();
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '_' | '-') {
            out.push(ch);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

fn avatar_extension(filename: &str) -> &'static str {
    match filename
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "png",
        "webp" => "webp",
        "jpg" | "jpeg" => "jpg",
        _ => "jpg",
    }
}

fn normalize_content_type(content_type: &str, filename: &str) -> String {
    match content_type.trim().to_ascii_lowercase().as_str() {
        "image/png" => "image/png".to_string(),
        "image/webp" => "image/webp".to_string(),
        "image/jpeg" | "image/jpg" => "image/jpeg".to_string(),
        _ => match avatar_extension(filename) {
            "png" => "image/png".to_string(),
            "webp" => "image/webp".to_string(),
            _ => "image/jpeg".to_string(),
        },
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    HEXLOWER.encode(&Sha256::digest(bytes))
}

fn signing_key(secret: &str, date: &str, region: &str) -> Result<Vec<u8>, ProfilePortError> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes())?;
    let k_region = hmac_sha256(&k_date, region.as_bytes())?;
    let k_service = hmac_sha256(&k_region, b"s3")?;
    hmac_sha256(&k_service, b"aws4_request")
}

fn hmac_sha256(key: &[u8], bytes: &[u8]) -> Result<Vec<u8>, ProfilePortError> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| ProfilePortError::LookupFailed)?;
    mac.update(bytes);
    Ok(mac.finalize().into_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::{avatar_object_key, public_avatar_url};

    #[test]
    fn profile_avatar_key_uses_role_ref_and_content_hash() {
        assert_eq!(
            avatar_object_key("Werka", " worker/1 ", "face.PNG", b"abc"),
            "profile_avatars/werka/worker_1/ba7816bf8f01cfea.png"
        );
    }

    #[test]
    fn public_avatar_url_trims_base_and_preserves_key_path() {
        assert_eq!(
            public_avatar_url(
                "https://cdn.test/avatars/",
                "profile_avatars/werka/worker_1/avatar.png"
            ),
            "https://cdn.test/avatars/profile_avatars/werka/worker_1/avatar.png"
        );
    }
}
