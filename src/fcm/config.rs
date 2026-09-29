//! Admin-managed FCM credentials. Never expose the credential document in status responses.
use super::{
    FcmPushSender, auth::ServiceAccount, discover_service_account_path, payload::FcmPayload,
};
use crate::core::auth::{code_vault::CodeCipher, models::Principal};
use crate::core::push::{
    ports::{PushSendError, PushSenderPort, PushTokenStorePort},
    service::push_token_key,
};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, RwLock},
};
use tokio::sync::Mutex;

const MAX_CREDENTIAL_BYTES: usize = 32 * 1024;
const BINDING: &str = "firebase-push-configuration";

#[derive(Clone, Default, Serialize)]
pub struct FcmConfigStatus {
    pub configured: bool,
    pub project_id: String,
    pub client_email: String,
    pub source: String,
    pub last_verified_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum FcmConfigError {
    #[error("push_config_invalid_credentials")]
    InvalidCredentials,
    #[error("push_config_google_rejected")]
    GoogleRejected,
    #[error("push_config_unreachable")]
    Unreachable,
    #[error("push_config_save_failed")]
    SaveFailed,
    #[error("push_config_not_configured")]
    NotConfigured,
    #[error("push_config_device_not_registered")]
    DeviceNotRegistered,
    #[error("push_config_test_failed")]
    TestFailed,
}

#[derive(Default)]
struct RuntimeConfig {
    sender: Option<Arc<FcmPushSender>>,
    status: FcmConfigStatus,
}

pub struct FcmConfigService {
    store: Arc<dyn PushTokenStorePort>,
    path: PathBuf,
    runtime: RwLock<RuntimeConfig>,
    update: Mutex<()>,
    #[cfg(test)]
    cipher_override: Option<Arc<CodeCipher>>,
    #[cfg(test)]
    endpoints: Option<(String, String)>,
}

impl FcmConfigService {
    pub fn load(store: Arc<dyn PushTokenStorePort>) -> Arc<Self> {
        #[cfg(not(test))]
        let path = std::env::var_os("FCM_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data/fcm-config.enc"));
        #[cfg(test)]
        let path =
            std::env::temp_dir().join(format!("fcm-config-{:032x}.enc", rand::random::<u128>()));
        let service = Arc::new(Self::new(store, path));
        #[cfg(not(test))]
        service.restore();
        service
    }

    fn new(store: Arc<dyn PushTokenStorePort>, path: PathBuf) -> Self {
        Self {
            store,
            path,
            runtime: RwLock::new(RuntimeConfig::default()),
            update: Mutex::new(()),
            #[cfg(test)]
            cipher_override: None,
            #[cfg(test)]
            endpoints: None,
        }
    }

    fn cipher(&self) -> Result<Arc<CodeCipher>, FcmConfigError> {
        #[cfg(test)]
        if let Some(cipher) = &self.cipher_override {
            return Ok(cipher.clone());
        }
        CodeCipher::load()
            .map(Arc::new)
            .map_err(|_| FcmConfigError::SaveFailed)
    }

    fn restore(&self) {
        let loaded = if self.path.exists() {
            std::fs::read_to_string(&self.path)
                .map_err(|_| FcmConfigError::SaveFailed)
                .and_then(|encrypted| {
                    self.cipher()?
                        .decrypt(BINDING, "v1", &encrypted)
                        .map_err(|_| FcmConfigError::SaveFailed)
                })
                .map(|raw| (raw, "admin"))
        } else if let Some(path) = discover_service_account_path() {
            std::fs::read_to_string(path)
                .map(|raw| (raw, "environment"))
                .map_err(|_| FcmConfigError::SaveFailed)
        } else {
            return;
        };
        match loaded.and_then(|(raw, source)| {
            let document: Value =
                serde_json::from_str(&raw).map_err(|_| FcmConfigError::InvalidCredentials)?;
            let account = validate_account(&document)?;
            Ok(self.runtime_for(account, source, None))
        }) {
            Ok(runtime) => *self.runtime.write().unwrap() = runtime,
            Err(error) => {
                tracing::warn!(%error, "FCM configuration could not be loaded");
                self.runtime.write().unwrap().status.error = Some(error.to_string());
            }
        }
    }

    #[allow(unused_mut)]
    fn runtime_for(
        &self,
        mut account: ServiceAccount,
        source: &str,
        verified: Option<i64>,
    ) -> RuntimeConfig {
        let status = FcmConfigStatus {
            configured: true,
            project_id: account.project_id.clone(),
            client_email: account.client_email.clone(),
            source: source.into(),
            last_verified_at: verified,
            error: None,
        };
        #[cfg(test)]
        if let Some((auth, _)) = &self.endpoints {
            account.token_uri = auth.clone();
        }
        let project_id = account.project_id.clone();
        let mut sender = FcmPushSender::new(self.store.clone(), account, project_id);
        #[cfg(test)]
        if let Some((_, endpoint)) = &self.endpoints {
            sender.endpoint = endpoint.clone();
        }
        RuntimeConfig {
            sender: Some(Arc::new(sender)),
            status,
        }
    }

    pub fn status(&self) -> FcmConfigStatus {
        self.runtime.read().unwrap().status.clone()
    }

    fn sender(&self) -> Result<Arc<FcmPushSender>, FcmConfigError> {
        self.runtime
            .read()
            .unwrap()
            .sender
            .clone()
            .ok_or(FcmConfigError::NotConfigured)
    }

    pub async fn save(&self, document: Value) -> Result<FcmConfigStatus, FcmConfigError> {
        let account = validate_account(&document)?;
        let _guard = self.update.lock().await;
        // Validate the candidate before replacing either the working sender or durable credentials.
        let mut runtime = self.runtime_for(account, "admin", None);
        runtime
            .sender
            .as_ref()
            .unwrap()
            .verify_credentials()
            .await?;
        let raw =
            serde_json::to_string(&document).map_err(|_| FcmConfigError::InvalidCredentials)?;
        let encrypted = self
            .cipher()?
            .encrypt(BINDING, "v1", &raw)
            .map_err(|_| FcmConfigError::SaveFailed)?;
        persist_private(&self.path, encrypted.as_bytes())?;
        runtime.status.last_verified_at = Some(time::OffsetDateTime::now_utc().unix_timestamp());
        let status = runtime.status.clone();
        *self.runtime.write().unwrap() = runtime;
        Ok(status)
    }

    pub async fn check(&self) -> Result<FcmConfigStatus, FcmConfigError> {
        let _guard = self.update.lock().await;
        self.sender()?.verify_credentials().await?;
        let mut runtime = self.runtime.write().unwrap();
        runtime.status.last_verified_at = Some(time::OffsetDateTime::now_utc().unix_timestamp());
        runtime.status.error = None;
        Ok(runtime.status.clone())
    }

    pub async fn test_device(
        &self,
        principal: &Principal,
        token: &str,
    ) -> Result<(), FcmConfigError> {
        let token = token.trim();
        let registered = self
            .store
            .list(&push_token_key(principal))
            .await
            .map_err(|_| FcmConfigError::TestFailed)?;
        if token.is_empty() || !registered.iter().any(|record| record.token == token) {
            return Err(FcmConfigError::DeviceNotRegistered);
        }
        let sender = self.sender()?;
        let data = HashMap::from([
            ("event_type".into(), "push.configuration.test".into()),
            ("target_role".into(), "admin".into()),
            ("target_ref".into(), principal.ref_.clone()),
        ]);
        let payload = FcmPayload::new(token, "Accord", "Bildirishnomalar ulandi", data);
        sender
            .config_request(&payload)
            .await
            .map_err(|error| match error {
                FcmConfigError::Unreachable => error,
                _ => FcmConfigError::TestFailed,
            })
    }
}

#[async_trait]
impl PushSenderPort for FcmConfigService {
    async fn send_to_key(
        &self,
        key: &str,
        title: &str,
        body: &str,
        data: HashMap<String, String>,
    ) -> Result<(), PushSendError> {
        let sender = self.sender().map_err(|_| PushSendError::SendFailed)?;
        sender.send_to_key(key, title, body, data).await
    }
}

impl FcmPushSender {
    async fn verify_credentials(&self) -> Result<(), FcmConfigError> {
        // FCM validates project access and payload without delivering a notification.
        self.config_request(&json!({"validate_only":true,"message":{
            "topic":"accord_configuration_validation",
            "notification":{"title":"Accord","body":"Configuration validation"}
        }}))
        .await
    }

    async fn config_request(&self, payload: &impl Serialize) -> Result<(), FcmConfigError> {
        let token = self
            .token_provider
            .access_token(&self.http_client)
            .await
            .map_err(|_| FcmConfigError::GoogleRejected)?;
        let response = self
            .http_client
            .post(&self.endpoint)
            .bearer_auth(token)
            .json(payload)
            .send()
            .await
            .map_err(|_| FcmConfigError::Unreachable)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(FcmConfigError::GoogleRejected)
        }
    }
}

fn validate_account(document: &Value) -> Result<ServiceAccount, FcmConfigError> {
    if document.to_string().len() > MAX_CREDENTIAL_BYTES || document["type"] != "service_account" {
        return Err(FcmConfigError::InvalidCredentials);
    }
    let account: ServiceAccount =
        serde_json::from_value(document.clone()).map_err(|_| FcmConfigError::InvalidCredentials)?;
    if account.project_id.is_empty()
        || account.project_id.len() > 64
        || !account
            .project_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || !account.client_email.ends_with(".iam.gserviceaccount.com")
        || account.client_email.len() > 254
        || !account.client_email.contains('@')
        || !matches!(
            account.token_uri.as_str(),
            "" | "https://oauth2.googleapis.com/token"
                | "https://accounts.google.com/o/oauth2/token"
        )
        || jsonwebtoken::EncodingKey::from_rsa_pem(account.private_key.as_bytes()).is_err()
    {
        return Err(FcmConfigError::InvalidCredentials);
    }
    Ok(account)
}

fn persist_private(path: &std::path::Path, bytes: &[u8]) -> Result<(), FcmConfigError> {
    use std::io::Write;
    let result = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension(format!("{:032x}.tmp", rand::random::<u128>()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let write = (|| -> std::io::Result<()> {
            let mut file = options.open(&temp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            std::fs::rename(&temp, path)
        })();
        let _ = std::fs::remove_file(&temp);
        write
    })();
    result.map_err(|_| FcmConfigError::SaveFailed)
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
