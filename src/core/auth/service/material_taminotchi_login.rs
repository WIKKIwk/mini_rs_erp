use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::auth::ports::{MaterialTaminotchiLookup, MaterialTaminotchiRecord};

use super::helpers::{
    local_phone_query, merge_material_taminotchi_records, phone_matches_normalized,
};
use super::{AuthError, AuthService};

impl AuthService {
    pub(super) async fn login_material_taminotchi_party(
        &self,
        normalized_phone: &str,
        code: &str,
    ) -> Result<Principal, AuthError> {
        let lookup = self
            .material_taminotchi_lookup
            .as_ref()
            .ok_or(AuthError::InvalidCredentials)?;
        let admin_state_lookup = self
            .admin_state_lookup
            .as_ref()
            .ok_or(AuthError::InvalidCredentials)?;

        let materials = self
            .search_material_taminotchilar_for_login(lookup.as_ref(), normalized_phone)
            .await?;
        let states = admin_state_lookup
            .list_states()
            .await
            .map_err(|_| AuthError::Internal)?;

        for material in materials {
            let state = states.get(material.id.trim()).cloned().unwrap_or_default();
            if state.removed || state.blocked {
                continue;
            }
            let code_value = state.custom_code.trim();
            if code_value.is_empty() {
                continue;
            }
            if code.trim() == code_value
                && phone_matches_normalized(&material.phone, normalized_phone)
            {
                return Ok(Principal {
                    role: PrincipalRole::MaterialTaminotchi,
                    display_name: material.name.clone(),
                    legal_name: material.name,
                    ref_: material.id,
                    phone: material.phone,
                    avatar_url: String::new(),
                });
            }
        }

        Err(AuthError::InvalidCredentials)
    }

    async fn search_material_taminotchilar_for_login(
        &self,
        lookup: &dyn MaterialTaminotchiLookup,
        normalized_phone: &str,
    ) -> Result<Vec<MaterialTaminotchiRecord>, AuthError> {
        let mut materials = lookup
            .search_material_taminotchilar(normalized_phone, 50)
            .await
            .map_err(|_| AuthError::Internal)?;
        if let Some(local_phone) = local_phone_query(normalized_phone) {
            let local_matches = lookup
                .search_material_taminotchilar(&local_phone, 50)
                .await
                .map_err(|_| AuthError::Internal)?;
            merge_material_taminotchi_records(&mut materials, local_matches);
        }
        if materials.is_empty() {
            materials = lookup
                .search_material_taminotchilar("", 500)
                .await
                .map_err(|_| AuthError::Internal)?;
        }
        Ok(materials)
    }
}
