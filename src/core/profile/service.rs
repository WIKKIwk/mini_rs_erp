use std::sync::Arc;

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::profile::avatar_image::prepare_profile_avatar;
use crate::core::profile::identity::{ProfileIdentity, load_profile_prefs};
use crate::core::profile::ports::{
    DownloadedFile, ProfileAvatarStorage, ProfileLookup, ProfilePortError, ProfilePrefs,
    ProfileStoreError, ProfileStorePort,
};

#[derive(Clone)]
pub struct ProfileService {
    file_base_url: String,
    lookup: Option<Arc<dyn ProfileLookup>>,
    store: Option<Arc<dyn ProfileStorePort>>,
    avatar_storage: Option<Arc<dyn ProfileAvatarStorage>>,
}

impl ProfileService {
    pub fn new(file_base_url: String) -> Self {
        Self {
            file_base_url: file_base_url.trim().trim_end_matches('/').to_string(),
            lookup: None,
            store: None,
            avatar_storage: None,
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

    pub fn with_avatar_storage(mut self, storage: Arc<dyn ProfileAvatarStorage>) -> Self {
        self.avatar_storage = Some(storage);
        self
    }

    pub async fn refresh(&self, mut principal: Principal) -> Principal {
        let Some(lookup) = self.lookup.as_ref() else {
            return self.merge_prefs(principal).await;
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
            PrincipalRole::Werka
            | PrincipalRole::Qolipchi
            | PrincipalRole::Boyoqchi
            | PrincipalRole::MaterialTaminotchi
            | PrincipalRole::Admin => {}
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
        _content_type: &str,
        content: Vec<u8>,
    ) -> Result<Principal, ProfilePortError> {
        if self.avatar_storage.is_none() && principal.role != PrincipalRole::Supplier {
            return Ok(principal);
        }
        let prepared = prepare_profile_avatar(filename, content)?;
        if let Some(storage) = &self.avatar_storage {
            let avatar = storage
                .put_profile_avatar(
                    ProfileIdentity::from_principal(&principal.role, &principal.ref_)
                        .ok_or(ProfilePortError::LookupFailed)?
                        .role_key(),
                    &principal.ref_,
                    &prepared.filename,
                    &prepared.content_type,
                    prepared.body,
                )
                .await?;
            principal.avatar_url = avatar.public_url;

            if let Some(store) = &self.store {
                let key = profile_key(&principal);
                let mut prefs = store
                    .get(&key)
                    .await
                    .map_err(|_| ProfilePortError::LookupFailed)?;
                prefs.avatar_url = principal.avatar_url.clone();
                prefs.avatar_object_key = avatar.object_key;
                store
                    .put(&key, prefs)
                    .await
                    .map_err(|_| ProfilePortError::LookupFailed)?;
            }

            return Ok(self.merge_prefs(principal).await);
        }

        let Some(lookup) = &self.lookup else {
            return Err(ProfilePortError::LookupFailed);
        };
        let file_url = lookup
            .upload_supplier_image(
                &principal.ref_,
                &prepared.filename,
                &prepared.content_type,
                prepared.body,
            )
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
        if let Some(storage) = &self.avatar_storage
            && let Some(store) = &self.store
            && let Ok(prefs) = store.get(&profile_key(&principal)).await
            && !prefs.avatar_object_key.trim().is_empty()
        {
            return storage
                .get_profile_avatar(prefs.avatar_object_key.trim())
                .await
                .map(Some);
        }

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

    pub async fn download_avatar_for_profile(
        &self,
        role_key: &str,
        principal_ref: &str,
    ) -> Result<Option<DownloadedFile>, ProfilePortError> {
        let Some(storage) = &self.avatar_storage else {
            return Ok(None);
        };
        let Some(store) = &self.store else {
            return Ok(None);
        };
        let Some(identity) = ProfileIdentity::new(role_key, principal_ref) else {
            return Ok(None);
        };
        let Ok(prefs) = load_profile_prefs(store.as_ref(), &identity).await else {
            return Ok(None);
        };
        if prefs.avatar_object_key.trim().is_empty() {
            return Ok(None);
        }
        storage
            .get_profile_avatar(prefs.avatar_object_key.trim())
            .await
            .map(Some)
    }

    async fn merge_prefs(&self, mut principal: Principal) -> Principal {
        if let Some(store) = &self.store
            && let Some(identity) =
                ProfileIdentity::from_principal(&principal.role, &principal.ref_)
            && let Ok(prefs) = load_profile_prefs(store.as_ref(), &identity).await
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
    } else if !prefs.avatar_object_key.trim().is_empty() {
        principal.avatar_url = format!("local://{}", prefs.avatar_object_key.trim());
    }
    principal
}

fn profile_key(principal: &Principal) -> String {
    ProfileIdentity::from_principal(&principal.role, &principal.ref_)
        .expect("authenticated principal has a profile identity")
        .vault_key()
}
