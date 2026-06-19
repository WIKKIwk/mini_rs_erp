use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::{DEFAULT_TOKEN_URI, OrderSheetError, SHEETS_SCOPE};

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ServiceAccount {
    pub(super) client_email: String,
    pub(super) private_key: String,
    #[serde(default)]
    pub(super) token_uri: String,
}

#[derive(Debug)]
pub(super) struct ServiceAccountTokenProvider {
    account: ServiceAccount,
    cache: Mutex<Option<CachedAccessToken>>,
}

impl ServiceAccountTokenProvider {
    pub(super) fn new(account: ServiceAccount) -> Self {
        Self {
            account,
            cache: Mutex::new(None),
        }
    }

    pub(super) async fn access_token(
        &self,
        client: &reqwest::Client,
    ) -> Result<String, OrderSheetError> {
        let mut cache = self.cache.lock().await;
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        if let Some(cached) = cache.as_ref()
            && cached.expires_at > now + 60
        {
            return Ok(cached.access_token.clone());
        }

        let token_uri = self.token_uri();
        let assertion = self.signed_assertion(now, &token_uri)?;
        let form = format!(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={}",
            urlencoding::encode(&assertion)
        );
        let response = client
            .post(&token_uri)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(form)
            .send()
            .await
            .map_err(|_| OrderSheetError::AuthFailed)?;
        if !response.status().is_success() {
            return Err(OrderSheetError::AuthFailed);
        }
        let token: OAuthTokenResponse = response
            .json()
            .await
            .map_err(|_| OrderSheetError::AuthFailed)?;
        let expires_at = now + token.expires_in.unwrap_or(3600);
        *cache = Some(CachedAccessToken {
            access_token: token.access_token.clone(),
            expires_at,
        });
        Ok(token.access_token)
    }

    fn token_uri(&self) -> String {
        let value = self.account.token_uri.trim();
        if value.is_empty() {
            DEFAULT_TOKEN_URI.to_string()
        } else {
            value.to_string()
        }
    }

    fn signed_assertion(&self, now: i64, token_uri: &str) -> Result<String, OrderSheetError> {
        let claims = JwtClaims {
            iss: self.account.client_email.trim(),
            scope: SHEETS_SCOPE,
            aud: token_uri,
            iat: now,
            exp: now + 3600,
        };
        let key = EncodingKey::from_rsa_pem(self.account.private_key.as_bytes())
            .map_err(|_| OrderSheetError::AuthFailed)?;
        encode(&Header::new(Algorithm::RS256), &claims, &key)
            .map_err(|_| OrderSheetError::AuthFailed)
    }
}

#[derive(Debug, Clone)]
struct CachedAccessToken {
    access_token: String,
    expires_at: i64,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    expires_in: Option<i64>,
}

#[derive(Serialize)]
struct JwtClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
}
