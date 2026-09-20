use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

// Bound memory use across simultaneous login attempts for different accounts.
static HASH_JOBS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);

/// PHC strings include the algorithm, work factors, salt and derived hash.
pub fn is_password_hash(value: &str) -> bool {
    value.starts_with("$argon2id$") && PasswordHash::new(value).is_ok()
}

pub fn hash_password_sync(password: &str) -> Result<String, String> {
    if password.is_empty() || password.len() > 1024 {
        return Err("access code must contain 1 to 1024 bytes".into());
    }
    let salt = SaltString::encode_b64(&rand::random::<[u8; 16]>())
        .map_err(|_| "password salt generation failed")?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| "password hashing failed".into())
}

pub async fn hash_password(password: String) -> Result<String, String> {
    let permit = HASH_JOBS
        .acquire()
        .await
        .map_err(|_| "password worker unavailable")?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        hash_password_sync(&password)
    })
    .await
    .map_err(|_| "password hashing task failed".to_string())?
}

pub async fn verify_password(hash: &str, password: &str) -> Result<bool, String> {
    if hash.is_empty() || password.is_empty() || password.len() > 1024 {
        return Ok(false);
    }
    if !is_password_hash(hash) {
        return Err("invalid stored password hash".into());
    }
    let hash = hash.to_string();
    let password = password.to_string();
    let permit = HASH_JOBS
        .acquire()
        .await
        .map_err(|_| "password worker unavailable")?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let parsed = PasswordHash::new(&hash).map_err(|_| "invalid stored password hash")?;
        match Argon2::default().verify_password(password.as_bytes(), &parsed) {
            Ok(()) => Ok(true),
            Err(argon2::password_hash::Error::Password) => Ok(false),
            Err(_) => Err("password verification failed".to_string()),
        }
    })
    .await
    .map_err(|_| "password verification task failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hashes_use_unique_salts_and_reject_plaintext_and_wrong_passwords() {
        let first = hash_password("test-secret-0123".into()).await.unwrap();
        let second = hash_password("test-secret-0123".into()).await.unwrap();
        assert_ne!(first, second);
        assert!(!first.contains("test-secret-0123"));
        assert!(verify_password(&first, "test-secret-0123").await.unwrap());
        assert!(!verify_password(&first, "wrong").await.unwrap());
        assert!(
            verify_password("test-secret-0123", "test-secret-0123")
                .await
                .is_err()
        );
        assert!(!verify_password(&first, &first).await.unwrap());
    }
}
