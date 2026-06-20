mod auth;
mod payload;
mod token_cleanup;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::core::push::ports::{NoopPushSender, PushSendError, PushSenderPort, PushTokenStorePort};

use self::auth::{ServiceAccount, ServiceAccountTokenProvider};
use self::payload::FcmPayload;
use self::token_cleanup::{should_drop_push_token, truncate_token};

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
            let payload = FcmPayload::new(&token.token, title, body, data.clone());
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
