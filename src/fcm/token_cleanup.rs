pub(super) fn should_drop_push_token(status_code: u16, body: &str) -> bool {
    if status_code != 404 && status_code != 400 {
        return false;
    }
    let lower = body.to_lowercase();
    lower.contains("unregistered")
        || lower.contains("requested entity was not found")
        || lower.contains("registration token is not a valid fcm registration token")
}

pub(super) fn truncate_token(token: &str) -> String {
    let trimmed = token.trim();
    if trimmed.len() <= 12 {
        return trimmed.to_string();
    }
    format!("{}...{}", &trimmed[..6], &trimmed[trimmed.len() - 6..])
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
    fn token_truncation_matches_go_shape() {
        assert_eq!(truncate_token("short"), "short");
        assert_eq!(truncate_token("abcdef1234567890"), "abcdef...567890");
    }
}
