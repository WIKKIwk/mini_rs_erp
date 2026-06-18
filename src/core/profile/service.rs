use std::sync::Arc;

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::profile::ports::{
    DownloadedFile, ProfileLookup, ProfilePortError, ProfilePrefs, ProfileStoreError,
    ProfileStorePort,
};

#[derive(Clone)]
pub struct ProfileService {
    file_base_url: String,
    lookup: Option<Arc<dyn ProfileLookup>>,
    store: Option<Arc<dyn ProfileStorePort>>,
}

impl ProfileService {
    pub fn new(file_base_url: String) -> Self {
        Self {
            file_base_url: file_base_url.trim().trim_end_matches('/').to_string(),
            lookup: None,
            store: None,
        }
    }

    #[cfg(test)]
    pub fn with_profile_lookup(mut self, lookup: Arc<dyn ProfileLookup>) -> Self {
        self.lookup = Some(lookup);
        self
    }

    pub fn with_store(mut self, store: Arc<dyn ProfileStorePort>) -> Self {
        self.store = Some(store);
        self
    }

    pub async fn refresh(&self, mut principal: Principal) -> Principal {
        let Some(lookup) = self.lookup.as_ref() else {
            return principal;
        };

        match principal.role {
            PrincipalRole::Supplier => {
                if let Ok(profile) = lookup.get_supplier_profile(&principal.ref_).await {
                    principal.phone = profile.phone;
                    if !profile.image.trim().is_empty() {
                        principal.avatar_url =
                            absolute_file_url(&self.file_base_url, &profile.image);
                    }
                }
            }
            PrincipalRole::Customer | PrincipalRole::Aparatchi => {
                if let Ok(profile) = lookup.get_customer_profile(&principal.ref_).await {
                    principal.phone = profile.phone;
                }
            }
            PrincipalRole::Werka | PrincipalRole::Qolipchi | PrincipalRole::Admin => {}
        }

        self.merge_prefs(principal).await
    }

    pub async fn update_nickname(
        &self,
        principal: Principal,
        nickname: &str,
    ) -> Result<Principal, ProfileStoreError> {
        let Some(store) = &self.store else {
            return Ok(principal);
        };
        let key = profile_key(&principal);
        let mut prefs = store.get(&key).await?;
        prefs.nickname = nickname.trim().to_string();
        store.put(&key, prefs).await?;
        Ok(self.merge_prefs(principal).await)
    }

    pub async fn upload_avatar(
        &self,
        mut principal: Principal,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<Principal, ProfilePortError> {
        if principal.role != PrincipalRole::Supplier {
            return Ok(principal);
        }
        let Some(lookup) = &self.lookup else {
            return Err(ProfilePortError::LookupFailed);
        };
        let file_url = lookup
            .upload_supplier_image(&principal.ref_, filename, content_type, content)
            .await?;
        principal.avatar_url = absolute_file_url(&self.file_base_url, &file_url);

        if let Some(store) = &self.store {
            let key = profile_key(&principal);
            let mut prefs = store
                .get(&key)
                .await
                .map_err(|_| ProfilePortError::LookupFailed)?;
            prefs.avatar_url = principal.avatar_url.clone();
            store
                .put(&key, prefs)
                .await
                .map_err(|_| ProfilePortError::LookupFailed)?;
        }

        Ok(self.merge_prefs(principal).await)
    }

    pub async fn download_avatar(
        &self,
        principal: Principal,
    ) -> Result<Option<DownloadedFile>, ProfilePortError> {
        if principal.role != PrincipalRole::Supplier {
            return Ok(None);
        }

        let current = self.refresh(principal).await;
        if current.avatar_url.trim().is_empty() {
            return Ok(None);
        }

        let Some(lookup) = &self.lookup else {
            return Err(ProfilePortError::LookupFailed);
        };

        lookup.download_file(&current.avatar_url).await.map(Some)
    }

    async fn merge_prefs(&self, mut principal: Principal) -> Principal {
        if let Some(store) = &self.store
            && let Ok(prefs) = store.get(&profile_key(&principal)).await
        {
            principal = merge_profile_prefs(principal, prefs);
        }
        if principal.display_name.is_empty() {
            principal.display_name = principal.legal_name.clone();
        }
        principal
    }
}

fn absolute_file_url(base_url: &str, file_url: &str) -> String {
    let trimmed = file_url.trim();
    if trimmed.is_empty() || trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("{}{}", base_url.trim_end_matches('/'), trimmed)
    }
}

fn merge_profile_prefs(mut principal: Principal, prefs: ProfilePrefs) -> Principal {
    if !prefs.nickname.trim().is_empty() {
        principal.display_name = prefs.nickname.trim().to_string();
    }
    if !prefs.avatar_url.trim().is_empty() {
        principal.avatar_url = prefs.avatar_url.trim().to_string();
    }
    principal
}

fn profile_key(principal: &Principal) -> String {
    format!("{}:{}", role_key(&principal.role), principal.ref_.trim())
}

fn role_key(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::Admin => "admin",
    }
}
