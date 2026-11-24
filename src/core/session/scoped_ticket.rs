//! Scoped capabilities backed by an existing ERP session. The stored SHA-256
//! digest of a random 192-bit bearer supplies a public 128-bit lookup prefix
//! and an independent private 128-bit HMAC key. Never return the full digest.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::hmac;
use time::OffsetDateTime;

use crate::error::AppError;

#[derive(Clone, Copy)]
pub(crate) enum TicketScope<'a> {
    Avatar { profile: Option<(&'a str, &'a str)>, version: &'a str },
    Live { path: &'a str },
}

impl TicketScope<'_> {
    fn ttl_seconds(self) -> i64 {
        match self {
            Self::Avatar { .. } => 300,
            Self::Live { .. } => 30,
        }
    }

    fn message(self, locator: &[u8], expiry: &[u8; 8]) -> Vec<u8> {
        let mut msg = b"accord/scoped-ticket/v1".to_vec();
        msg.extend_from_slice(locator);
        msg.extend_from_slice(expiry);
        let mut field = |value: &str| {
            msg.extend_from_slice(&(value.len() as u64).to_be_bytes());
            msg.extend_from_slice(value.as_bytes());
        };
        match self {
            Self::Avatar { profile, version } => {
                if let Some((role, principal_ref)) = profile {
                    field("directory-avatar");
                    field(role.trim());
                    field(principal_ref.trim());
                } else {
                    field("own-avatar");
                }
                field(version);
            }
            Self::Live { path } => {
                field("live");
                field(path);
            }
        }
        msg
    }
}

pub(crate) fn issue_ticket(token: &str, scope: TicketScope<'_>) -> String {
    let expires = OffsetDateTime::now_utc().unix_timestamp() + scope.ttl_seconds();
    mint(token, scope, expires)
}

fn mint(token: &str, scope: TicketScope<'_>, expires: i64) -> String {
    let digest = super::store::session_key(token);
    let (locator, secret) = digest.split_at(16);
    let expiry = expires.to_be_bytes();
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    let tag = hmac::sign(&key, &scope.message(locator, &expiry));
    let mut ticket = [0_u8; 56];
    ticket[..16].copy_from_slice(locator);
    ticket[16..24].copy_from_slice(&expiry);
    ticket[24..].copy_from_slice(tag.as_ref());
    URL_SAFE_NO_PAD.encode(ticket)
}

pub(super) struct ScopedTicket {
    locator: [u8; 16],
    expiry: [u8; 8],
    tag: [u8; 32],
}

impl ScopedTicket {
    pub(super) fn parse(value: &str) -> Result<Self, AppError> {
        if value.len() != 75 {
            return Err(AppError::Unauthorized);
        }
        let raw: [u8; 56] = URL_SAFE_NO_PAD.decode(value)
            .ok().and_then(|bytes| bytes.try_into().ok()).ok_or(AppError::Unauthorized)?;
        let ticket = Self {
            locator: raw[..16].try_into().map_err(|_| AppError::Unauthorized)?,
            expiry: raw[16..24].try_into().map_err(|_| AppError::Unauthorized)?,
            tag: raw[24..].try_into().map_err(|_| AppError::Unauthorized)?,
        };
        ticket.check_expiry()?;
        Ok(ticket)
    }

    fn check_expiry(&self) -> Result<(), AppError> {
        if OffsetDateTime::now_utc().unix_timestamp() >= i64::from_be_bytes(self.expiry) {
            return Err(AppError::Unauthorized);
        }
        Ok(())
    }

    pub(super) fn locator(&self) -> &[u8; 16] {
        &self.locator
    }

    pub(super) fn verify(&self, digest: &[u8; 32], scope: TicketScope<'_>) -> Result<(), AppError> {
        self.check_expiry()?;
        let key = hmac::Key::new(hmac::HMAC_SHA256, &digest[16..]);
        hmac::verify(&key, &scope.message(&self.locator, &self.expiry), &self.tag)
            .map_err(|_| AppError::Unauthorized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::auth::models::{Principal, PrincipalRole};
    use crate::core::session::manager::SessionManager;

    fn person() -> Principal {
        Principal {
            role: PrincipalRole::Customer, ref_: "customer-1".into(),
            display_name: String::new(), legal_name: String::new(),
            avatar_url: String::new(), phone: String::new(),
        }
    }

    #[tokio::test]
    async fn avatar_tickets_bind_version_target_and_session() {
        let sessions = SessionManager::memory(Some(60));
        let token = sessions.create(person()).await.unwrap();
        let original = TicketScope::Avatar { profile: Some(("worker", "A")), version: "a.png" };
        let ticket = issue_ticket(&token, original);
        assert_eq!(sessions.resolve_ticket(&ticket, original).await.unwrap().1, person());
        for different in [
            TicketScope::Avatar { profile: None, version: "a.png" },
            TicketScope::Avatar { profile: Some(("worker", "B")), version: "a.png" },
            TicketScope::Avatar { profile: Some(("worker", "A")), version: "b.png" },
            TicketScope::Live { path: "/live" },
        ] {
            assert!(matches!(sessions.resolve_ticket(&ticket, different).await, Err(AppError::Unauthorized)));
        }
        let mut altered = URL_SAFE_NO_PAD.decode(&ticket).unwrap();
        altered[24] ^= 1;
        assert!(matches!(sessions.resolve_ticket(&URL_SAFE_NO_PAD.encode(altered), original).await,
            Err(AppError::Unauthorized)));
        let expired = mint(&token, original, OffsetDateTime::now_utc().unix_timestamp());
        assert!(matches!(sessions.resolve_ticket(&expired, original).await, Err(AppError::Unauthorized)));
        assert!(matches!(sessions.get(&ticket).await, Err(AppError::Unauthorized)));
    }

    #[tokio::test]
    async fn persistent_avatar_tickets_honor_revocation_across_managers() {
        let directory = tempfile::tempdir().unwrap();
        let scope = TicketScope::Avatar { profile: None, version: "avatar.jpg" };
        for use_lmdb in [false, true] {
            let make_manager = || {
                if use_lmdb {
                    SessionManager::lmdb(directory.path().join("sessions.lmdb"), 1024 * 1024, Some(600)).unwrap()
                } else {
                    SessionManager::persistent(directory.path().join("sessions.json"), Some(600))
                }
            };
            let issuer = make_manager();
            let token = issuer.create(person()).await.unwrap();
            let ticket = issue_ticket(&token, scope);
            let reader = make_manager();
            assert!(reader.get(&token).await.is_ok());
            let (digest, viewer) = reader.resolve_ticket(&ticket, scope).await.unwrap();
            assert_eq!(viewer, person());
            assert_eq!(reader.get_by_digest(&digest).await.unwrap(), person());
            issuer.delete(&token).await.unwrap();
            assert!(matches!(reader.resolve_ticket(&ticket, scope).await, Err(AppError::Unauthorized)));
            assert!(matches!(reader.get_by_digest(&digest).await, Err(AppError::Unauthorized)));
        }
    }
}
