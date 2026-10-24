use super::*;

#[cfg(test)]
#[path = "session_security_tests.rs"]
mod tests;

impl AdminService {
    pub(super) async fn persist_access_state(
        &self,
        ref_: &str,
        state: AdminState,
    ) -> Result<(), AdminPortError> {
        let revoke = if self.session_revoker.is_some() {
            let previous = self.state_for(ref_).await?;
            state.blocked
                || state.removed
                || (!state.custom_code.is_empty() && state.custom_code != previous.custom_code)
        } else {
            false
        };
        self.state_port
            .as_ref()
            .ok_or(AdminPortError::LookupFailed)?
            .put_state(ref_, state)
            .await?;
        if revoke {
            if let Some(revoker) = &self.session_revoker {
                revoker.revoke_principal_sessions(ref_).await?;
            }
        }
        Ok(())
    }
}
