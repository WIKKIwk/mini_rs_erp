use std::collections::HashMap;
use std::sync::Arc;

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::push::ports::{
    NoopPushSender, PushSendError, PushSenderPort, PushServiceError, PushTokenStorePort,
};

#[derive(Clone)]
pub struct PushService {
    store: Arc<dyn PushTokenStorePort>,
    sender: Arc<dyn PushSenderPort>,
}

impl PushService {
    pub fn new(store: Arc<dyn PushTokenStorePort>) -> Self {
        Self {
            store,
            sender: Arc::new(NoopPushSender),
        }
    }

    pub fn with_sender(mut self, sender: Arc<dyn PushSenderPort>) -> Self {
        self.sender = sender;
        self
    }

    #[cfg(test)]
    pub fn store_for_tests(&self) -> Arc<dyn PushTokenStorePort> {
        self.store.clone()
    }

    pub async fn register(
        &self,
        principal: &Principal,
        token: &str,
        platform: &str,
    ) -> Result<(), PushServiceError> {
        if token.trim().is_empty() {
            return Err(PushServiceError::TokenRequired);
        }
        self.store
            .move_token_to_key(&push_token_key(principal), token, platform)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, principal: &Principal, token: &str) -> Result<(), PushServiceError> {
        if token.trim().is_empty() {
            return Err(PushServiceError::TokenRequired);
        }
        self.store.delete(&push_token_key(principal), token).await?;
        Ok(())
    }

    pub async fn send_to_key(
        &self,
        key: &str,
        title: &str,
        body: &str,
        data: HashMap<String, String>,
    ) -> Result<(), PushSendError> {
        self.sender.send_to_key(key, title, body, data).await
    }
}

pub fn push_token_key(principal: &Principal) -> String {
    format!("{}:{}", role_key(&principal.role), principal.ref_.trim())
}

fn role_key(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::Boyoqchi => "boyoqchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    }
}
