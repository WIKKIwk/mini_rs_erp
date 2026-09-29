use super::config::{FcmConfigError, FcmConfigService, persist_private};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FirebaseClientConfig {
    pub project_id: String,
    pub app_id: String,
    pub api_key: String,
    pub messaging_sender_id: String,
    pub application_id: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FirebaseMobileConfigs {
    pub android: Option<FirebaseClientConfig>,
    pub ios: Option<FirebaseClientConfig>,
}

impl FirebaseClientConfig {
    pub fn validate(&self, platform: &str, project: &str) -> Result<(), FcmConfigError> {
        let application = match platform {
            "android" => "com.example.accord_mobile_v2",
            "ios" => "com.example.accordMobileV2.mirsaid.uzkingshark",
            _ => return Err(FcmConfigError::InvalidClientConfig),
        };
        if self.project_id != project {
            return Err(FcmConfigError::ProjectMismatch);
        }
        let prefix = format!("1:{}:{platform}:", self.messaging_sender_id);
        if self.application_id != application
            || self.project_id.is_empty()
            || self.project_id.len() > 64
            || !self
                .project_id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || self.messaging_sender_id.is_empty()
            || self.messaging_sender_id.len() > 32
            || !self.messaging_sender_id.bytes().all(|b| b.is_ascii_digit())
            || !self.app_id.starts_with(&prefix)
            || self.app_id.len() <= prefix.len()
            || self.app_id.len() > 200
            || !self.api_key.starts_with("AIza")
            || self.api_key.len() > 200
            || !self
                .api_key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(FcmConfigError::InvalidClientConfig);
        }
        Ok(())
    }
}

impl FcmConfigService {
    pub fn mobile_configs(&self) -> FirebaseMobileConfigs {
        let project = self.runtime.read().unwrap().status.project_id.clone();
        let mut configs = self.mobile.read().unwrap().clone();
        for (platform, value) in [("android", &mut configs.android), ("ios", &mut configs.ios)] {
            if value
                .as_ref()
                .is_some_and(|c| c.validate(platform, &project).is_err())
            {
                *value = None;
            }
        }
        configs
    }

    pub async fn save_mobile_config(
        &self,
        platform: &str,
        config: FirebaseClientConfig,
    ) -> Result<(), FcmConfigError> {
        let _guard = self.update.lock().await;
        let status = self.status();
        if !status.configured {
            return Err(FcmConfigError::NotConfigured);
        }
        config.validate(platform, &status.project_id)?;
        let mut configs = self.mobile_configs();
        match platform {
            "android" => configs.android = Some(config),
            "ios" => configs.ios = Some(config),
            _ => return Err(FcmConfigError::InvalidClientConfig),
        }
        let bytes = serde_json::to_vec(&configs).map_err(|_| FcmConfigError::SaveFailed)?;
        persist_private(&self.path.with_extension("clients.json"), &bytes)?;
        *self.mobile.write().unwrap() = configs;
        Ok(())
    }
}
