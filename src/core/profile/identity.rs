use crate::core::auth::models::PrincipalRole;

use super::ports::{ProfilePrefs, ProfileStoreError, ProfileStorePort};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileIdentity {
    role_key: &'static str,
    principal_ref: String,
}

impl ProfileIdentity {
    pub fn new(role_key: &str, principal_ref: &str) -> Option<Self> {
        let role_key = canonical_role_key(role_key)?;
        let principal_ref = principal_ref.trim();
        if principal_ref.is_empty() {
            return None;
        }
        Some(Self {
            role_key,
            principal_ref: principal_ref.to_string(),
        })
    }

    pub fn from_principal(role: &PrincipalRole, principal_ref: &str) -> Option<Self> {
        Self::new(principal_role_key(role), principal_ref)
    }

    pub fn role_key(&self) -> &'static str {
        self.role_key
    }

    pub fn principal_ref(&self) -> &str {
        &self.principal_ref
    }

    pub fn vault_key(&self) -> String {
        format!("{}:{}", self.role_key, self.principal_ref)
    }

    pub fn lookup_keys(&self) -> Vec<String> {
        let mut keys = vec![self.vault_key()];
        let legacy_role = match self.role_key {
            "aparatchi" => Some("worker"),
            "qolipchi" => Some("system_user"),
            _ => None,
        };
        if let Some(legacy_role) = legacy_role {
            keys.push(format!("{legacy_role}:{}", self.principal_ref));
        }
        keys
    }
}

pub async fn load_profile_prefs(
    store: &dyn ProfileStorePort,
    identity: &ProfileIdentity,
) -> Result<ProfilePrefs, ProfileStoreError> {
    let canonical_key = identity.vault_key();
    for key in identity.lookup_keys() {
        let prefs = store.get(&key).await?;
        if !profile_prefs_has_data(&prefs) {
            continue;
        }
        if key != canonical_key {
            store.put(&canonical_key, prefs.clone()).await?;
        }
        return Ok(prefs);
    }
    Ok(ProfilePrefs::default())
}

pub fn profile_prefs_has_data(prefs: &ProfilePrefs) -> bool {
    !prefs.nickname.trim().is_empty()
        || !prefs.avatar_url.trim().is_empty()
        || !prefs.avatar_object_key.trim().is_empty()
}

fn principal_role_key(role: &PrincipalRole) -> &'static str {
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

fn canonical_role_key(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "supplier" => Some("supplier"),
        "werka" | "omborchi" => Some("werka"),
        "customer" | "haridor" => Some("customer"),
        "worker" | "ishchi" | "aparatchi" | "apparatchi" => Some("aparatchi"),
        "qolipchi" | "system_user" | "system-user" => Some("qolipchi"),
        "boyoqchi" | "bo'yoqchi" | "bo‘yoqchi" => Some("boyoqchi"),
        "material_taminotchi" | "material-taminotchi" | "materialtaminotchi" => {
            Some("material_taminotchi")
        }
        "admin" => Some("admin"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::ProfileIdentity;

    #[test]
    fn worker_alias_uses_aparatchi_vault_with_legacy_fallback() {
        let identity = ProfileIdentity::new("worker", " worker_001 ").expect("identity");

        assert_eq!(identity.role_key(), "aparatchi");
        assert_eq!(identity.principal_ref(), "worker_001");
        assert_eq!(identity.vault_key(), "aparatchi:worker_001");
        assert_eq!(
            identity.lookup_keys(),
            ["aparatchi:worker_001", "worker:worker_001"]
        );
    }
}
