use crate::core::auth::ports::AuthConfigSink;

use super::helpers::{blank_default, normalize_config_phone};
use super::{AuthIdentity, AuthService};

impl AuthConfigSink for AuthService {
    fn set_runtime_identity(
        &self,
        werka_phone: &str,
        werka_code: &str,
        werka_name: &str,
        admin_phone: &str,
        admin_name: &str,
    ) {
        let normalized_werka_phone =
            normalize_config_phone(werka_phone).unwrap_or_else(|_| werka_phone.trim().to_string());
        let normalized_admin_phone =
            normalize_config_phone(admin_phone).unwrap_or_else(|_| admin_phone.trim().to_string());
        let current = self.identity.read().expect("auth identity lock").clone();
        let identity = AuthIdentity {
            werka_phone: normalized_werka_phone,
            werka_code: werka_code.trim().to_string(),
            werka_name: blank_default(werka_name, "Werka"),
            material_taminotchi_phone: current.material_taminotchi_phone,
            material_taminotchi_code: current.material_taminotchi_code,
            material_taminotchi_name: current.material_taminotchi_name,
            admin_phone: normalized_admin_phone,
            admin_name: blank_default(admin_name, "Admin"),
        };
        *self.identity.write().expect("auth identity lock") = identity;
    }
}
