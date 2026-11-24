pub(super) fn should_drop_push_token(status_code: u16, body: &str) -> bool {
    if status_code != 404 && status_code != 400 {
        return false;
    }
    let lower = body.to_lowercase();
    lower.contains("unregistered")
        || lower.contains("requested entity was not found")
        || lower.contains("registration token is not a valid fcm registration token")
}

pub(super) fn token_log_id(token: &str) -> String {
    use sha2::{Digest, Sha256};

    format!("{:x}", Sha256::digest(token.trim().as_bytes()))[..12].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_token_detection_matches_go() {
        assert!(should_drop_push_token(
            404,
            "Requested entity was not found."
        ));
        assert!(should_drop_push_token(400, "UNREGISTERED"));
        assert!(should_drop_push_token(
            400,
            "registration token is not a valid FCM registration token"
        ));
        assert!(!should_drop_push_token(500, "UNREGISTERED"));
        assert!(!should_drop_push_token(400, "quota exceeded"));
    }

    #[test]
    fn log_identifiers_are_safe_for_legacy_unicode_and_short_tokens() {
        let id = token_log_id("abcd🙂abcdef🙂");
        assert_eq!(id.len(), 12);
        assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(id, token_log_id("abcd🙂abcdef🙂"));
        assert_ne!(id, token_log_id("short"));
        assert!(!token_log_id("short").contains("short"));
    }
}
