use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::core::push::ports::{NoopPushSender, PushSendError, PushSenderPort, PushTokenStorePort};

const FCM_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

pub fn discover_push_sender(store: Arc<dyn PushTokenStorePort>) -> Arc<dyn PushSenderPort> {
    let Some(path) = discover_service_account_path() else {
        tracing::info!("push sender disabled: no firebase admin sdk json found");
        return Arc::new(NoopPushSender);
    };
    let raw = match std::fs::read(&path) {
        Ok(raw) => raw,
        Err(error) => {
            tracing::warn!(%error, "push sender disabled: read service account failed");
            return Arc::new(NoopPushSender);
        }
    };
    let account: ServiceAccount = match serde_json::from_slice(&raw) {
        Ok(account) => account,
        Err(error) => {
            tracing::warn!(%error, "push sender disabled: parse service account failed");
            return Arc::new(NoopPushSender);
        }
    };
    let project_id = account.project_id.trim().to_string();
    if project_id.is_empty() {
        tracing::warn!("push sender disabled: project_id missing in service account");
        return Arc::new(NoopPushSender);
    }

    tracing::info!(%project_id, "push sender enabled");
    Arc::new(FcmPushSender::new(store, account, project_id))
}

fn discover_service_account_path() -> Option<PathBuf> {
    if let Ok(env) = std::env::var("FCM_SERVICE_ACCOUNT_PATH") {
        let path = PathBuf::from(env.trim());
        if !path.as_os_str().is_empty() && path.is_file() {
            return Some(path);
        }
    }

    let mut matches = std::fs::read_dir(".")
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.contains("firebase-adminsdk") && name.ends_with(".json"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    matches.sort();
    if let Some(path) = matches.into_iter().next() {
        return Some(path);
    }

    let fallback = PathBuf::from("service-account.json");
    fallback.is_file().then_some(fallback)
}

pub struct FcmPushSender {
    store: Arc<dyn PushTokenStorePort>,
    http_client: reqwest::Client,
    token_provider: ServiceAccountTokenProvider,
    endpoint: String,
}

impl FcmPushSender {
    fn new(
        store: Arc<dyn PushTokenStorePort>,
        account: ServiceAccount,
        project_id: String,
    ) -> Self {
        Self {
            store,
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("reqwest client"),
            token_provider: ServiceAccountTokenProvider::new(account),
            endpoint: format!("https://fcm.googleapis.com/v1/projects/{project_id}/messages:send"),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_tests(
        store: Arc<dyn PushTokenStorePort>,
        client_email: &str,
        private_key: &str,
        token_uri: &str,
        endpoint: &str,
    ) -> Self {
        Self {
            store,
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("reqwest client"),
            token_provider: ServiceAccountTokenProvider::new(ServiceAccount {
                project_id: "demo".to_string(),
                client_email: client_email.to_string(),
                private_key: private_key.to_string(),
                token_uri: token_uri.to_string(),
            }),
            endpoint: endpoint.to_string(),
        }
    }
}

#[async_trait]
impl PushSenderPort for FcmPushSender {
    async fn send_to_key(
        &self,
        key: &str,
        title: &str,
        body: &str,
        data: HashMap<String, String>,
    ) -> Result<(), PushSendError> {
        let key = key.trim();
        let tokens = self.store.list(key).await?;
        if tokens.is_empty() {
            tracing::info!(%key, "push sender skipped: no tokens");
            return Ok(());
        }
        let access_token = self.token_provider.access_token(&self.http_client).await?;
        tracing::info!(%key, count = tokens.len(), "push sender sending");

        let mut sent_any = false;
        let mut last_error = None;
        for token in tokens {
            let payload = FcmPayload {
                message: FcmMessage {
                    token: token.token.clone(),
                    notification: FcmNotification {
                        title: title.to_string(),
                        body: body.to_string(),
                    },
                    data: data.clone(),
                    android: FcmAndroid {
                        priority: "HIGH",
                        notification: FcmAndroidNotification {
                            channel_id: "accord_updates",
                            sound: "default",
                        },
                    },
                },
            };
            let response = self
                .http_client
                .post(&self.endpoint)
                .bearer_auth(&access_token)
                .json(&payload)
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        %key,
                        token = %truncate_token(&token.token),
                        "push sender request failed"
                    );
                    last_error = Some(PushSendError::SendFailed);
                    continue;
                }
            };
            let status = response.status();
            let body = response
                .bytes()
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_string())
                .unwrap_or_default();
            if !status.is_success() {
                tracing::warn!(
                    %key,
                    token = %truncate_token(&token.token),
                    status = status.as_u16(),
                    body = %body.trim(),
                    "push sender token failed"
                );
                if should_drop_push_token(status.as_u16(), &body) {
                    if let Err(error) = self.store.delete(key, &token.token).await {
                        tracing::warn!(
                            ?error,
                            %key,
                            token = %truncate_token(&token.token),
                            "push sender failed to drop stale token"
                        );
                    } else {
                        tracing::info!(
                            %key,
                            token = %truncate_token(&token.token),
                            "push sender dropped stale token"
                        );
                    }
                }
                last_error = Some(PushSendError::SendFailed);
                continue;
            }
            sent_any = true;
            tracing::info!(
                %key,
                token = %truncate_token(&token.token),
                "push sender delivered"
            );
        }

        if sent_any {
            Ok(())
        } else {
            Err(last_error.unwrap_or(PushSendError::SendFailed))
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ServiceAccount {
    project_id: String,
    client_email: String,
    private_key: String,
    #[serde(default)]
    token_uri: String,
}

#[derive(Debug)]
struct ServiceAccountTokenProvider {
    account: ServiceAccount,
    cache: Mutex<Option<CachedAccessToken>>,
}

impl ServiceAccountTokenProvider {
    fn new(account: ServiceAccount) -> Self {
        Self {
            account,
            cache: Mutex::new(None),
        }
    }

    async fn access_token(&self, client: &reqwest::Client) -> Result<String, PushSendError> {
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
            .map_err(|_| PushSendError::SendFailed)?;
        if !response.status().is_success() {
            return Err(PushSendError::SendFailed);
        }
        let token: OAuthTokenResponse = response
            .json()
            .await
            .map_err(|_| PushSendError::SendFailed)?;
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

    fn signed_assertion(&self, now: i64, token_uri: &str) -> Result<String, PushSendError> {
        let claims = JwtClaims {
            iss: self.account.client_email.trim(),
            scope: FCM_SCOPE,
            aud: token_uri,
            iat: now,
            exp: now + 3600,
        };
        let key = EncodingKey::from_rsa_pem(self.account.private_key.as_bytes())
            .map_err(|_| PushSendError::SendFailed)?;
        encode(&Header::new(Algorithm::RS256), &claims, &key).map_err(|_| PushSendError::SendFailed)
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

#[derive(Serialize)]
struct FcmPayload {
    message: FcmMessage,
}

#[derive(Serialize)]
struct FcmMessage {
    token: String,
    notification: FcmNotification,
    data: HashMap<String, String>,
    android: FcmAndroid,
}

#[derive(Serialize)]
struct FcmNotification {
    title: String,
    body: String,
}

#[derive(Serialize)]
struct FcmAndroid {
    priority: &'static str,
    notification: FcmAndroidNotification,
}

#[derive(Serialize)]
struct FcmAndroidNotification {
    channel_id: &'static str,
    sound: &'static str,
}

fn should_drop_push_token(status_code: u16, body: &str) -> bool {
    if status_code != 404 && status_code != 400 {
        return false;
    }
    let lower = body.to_lowercase();
    lower.contains("unregistered")
        || lower.contains("requested entity was not found")
        || lower.contains("registration token is not a valid fcm registration token")
}

fn truncate_token(token: &str) -> String {
    let trimmed = token.trim();
    if trimmed.len() <= 12 {
        return trimmed.to_string();
    }
    format!("{}...{}", &trimmed[..6], &trimmed[trimmed.len() - 6..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_token_detection_matches_go() {
        assert!(should_drop_push_token(
            404,
            "Requested entity was not found."
        ));
        assert!(should_drop_push_token(400, "UNREGISTERED"));
        assert!(should_drop_push_token(
            400,
            "registration token is not a valid FCM registration token"
        ));
        assert!(!should_drop_push_token(500, "UNREGISTERED"));
        assert!(!should_drop_push_token(400, "quota exceeded"));
    }

    #[test]
    fn token_truncation_matches_go_shape() {
        assert_eq!(truncate_token("short"), "short");
        assert_eq!(truncate_token("abcdef1234567890"), "abcdef...567890");
    }
}
