//! Recoverable admin-managed access codes. Login continues to use Argon2id.
use std::path::PathBuf;

use base64::{Engine, engine::general_purpose::STANDARD};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};

pub struct CodeCipher(LessSafeKey);

impl CodeCipher {
    pub fn from_key(key: &[u8]) -> Result<Self, String> {
        UnboundKey::new(&AES_256_GCM, key)
            .map(|key| Self(LessSafeKey::new(key)))
            .map_err(|_| "invalid access-code encryption key".into())
    }

    pub fn load() -> Result<Self, String> {
        let encoded = match std::env::var("MINI_ERP_ACCESS_CODE_KEY") {
            Ok(value) => value,
            Err(_) => load_key_file()?,
        };
        let key = STANDARD
            .decode(encoded.trim())
            .map_err(|_| "invalid access-code encryption key".to_string())?;
        Self::from_key(&key)
    }

    pub fn encrypt(&self, principal: &str, hash: &str, code: &str) -> Result<String, String> {
        let nonce = rand::random::<[u8; 12]>();
        let mut body = code.as_bytes().to_vec();
        self.0
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(binding(principal, hash)),
                &mut body,
            )
            .map_err(|_| "access-code encryption failed")?;
        let mut envelope = nonce.to_vec();
        envelope.extend(body);
        Ok(format!("v1:{}", STANDARD.encode(envelope)))
    }

    pub fn decrypt(&self, principal: &str, hash: &str, encoded: &str) -> Result<String, String> {
        let encoded = encoded
            .strip_prefix("v1:")
            .ok_or("invalid access-code envelope")?;
        let mut envelope = STANDARD
            .decode(encoded)
            .map_err(|_| "invalid access-code envelope")?;
        if envelope.len() < 28 {
            return Err("invalid access-code envelope".into());
        }
        let nonce: [u8; 12] = envelope[..12]
            .try_into()
            .map_err(|_| "invalid access-code nonce")?;
        let plaintext = self
            .0
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(binding(principal, hash)),
                &mut envelope[12..],
            )
            .map_err(|_| "access-code decryption failed")?;
        String::from_utf8(plaintext.to_vec()).map_err(|_| "invalid access-code encoding".into())
    }
}

fn binding(principal: &str, hash: &str) -> Vec<u8> {
    format!("mini-rs-erp/access-code/v1\0{principal}\0{hash}").into_bytes()
}

fn load_key_file() -> Result<String, String> {
    use std::io::Write;
    let path = std::env::var_os("MINI_ERP_ACCESS_CODE_KEY_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/access-code.key"));
    if !path.exists() {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|_| "access-code key directory unavailable")?;
        }
        let temporary = path.with_extension(format!("key-{:032x}.tmp", rand::random::<u128>()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| {
            let mut file = options
                .open(&temporary)
                .map_err(|_| "access-code key creation failed")?;
            file.write_all(STANDARD.encode(rand::random::<[u8; 32]>()).as_bytes())
                .map_err(|_| "access-code key write failed")?;
            file.sync_all().map_err(|_| "access-code key sync failed")?;
            // Publish a complete file atomically without replacing another process's key.
            match std::fs::hard_link(&temporary, &path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
                Err(_) => Err("access-code key publication failed"),
            }
        })();
        let _ = std::fs::remove_file(temporary);
        result?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(&path).map_err(|_| "access-code key unavailable")?;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("access-code key file must have private permissions (0600)".into());
        }
    }
    std::fs::read_to_string(path).map_err(|_| "access-code key unavailable".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_survive_reopening_but_reject_tampering_wrong_keys_and_swapped_accounts() {
        let key = [17; 32];
        let cipher = CodeCipher::from_key(&key).unwrap();
        let first = cipher.encrypt("worker", "hash-1", "401234567890").unwrap();
        let second = cipher.encrypt("worker", "hash-1", "401234567890").unwrap();
        assert_ne!(first, second);
        assert!(!first.contains("401234567890"));
        let reopened = CodeCipher::from_key(&key).unwrap();
        assert_eq!(
            reopened.decrypt("worker", "hash-1", &first).unwrap(),
            "401234567890"
        );
        assert!(reopened.decrypt("other-worker", "hash-1", &first).is_err());
        assert!(reopened.decrypt("worker", "hash-2", &first).is_err());
        assert!(
            CodeCipher::from_key(&[18; 32])
                .unwrap()
                .decrypt("worker", "hash-1", &first)
                .is_err()
        );
        let mut bytes = STANDARD.decode(first.strip_prefix("v1:").unwrap()).unwrap();
        bytes[15] ^= 1;
        assert!(
            reopened
                .decrypt(
                    "worker",
                    "hash-1",
                    &format!("v1:{}", STANDARD.encode(bytes))
                )
                .is_err()
        );
    }
}
