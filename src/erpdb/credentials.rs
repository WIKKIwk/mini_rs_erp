use aes::Aes128;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE;
use hmac::{Hmac, Mac};
use rand::Rng;
use sha2::Sha256;
use time::OffsetDateTime;

use crate::core::admin::ports::{AdminCredentialPort, AdminPortError};
use crate::erpdb::reader::DirectDbReader;

type HmacSha256 = Hmac<Sha256>;
type Aes128CbcEnc = cbc::Encryptor<Aes128>;
type Aes128CbcDec = cbc::Decryptor<Aes128>;

#[async_trait]
impl AdminCredentialPort for DirectDbReader {
    async fn admin_api_auth(&self, username: &str) -> Result<(String, String), AdminPortError> {
        let user = normalized_user(username);
        let api_key: String = sqlx::query_scalar(
            r#"
            SELECT COALESCE(api_key, '')
            FROM tabUser
            WHERE name = ?
            LIMIT 1
            "#,
        )
        .bind(&user)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        let api_key = api_key.trim().to_string();
        if api_key.is_empty() {
            return Err(AdminPortError::LookupFailed);
        }

        let encrypted_secret: String = sqlx::query_scalar(
            r#"
            SELECT password
            FROM __Auth
            WHERE doctype = 'User'
              AND name = ?
              AND fieldname = 'api_secret'
              AND encrypted = 1
            LIMIT 1
            "#,
        )
        .bind(&user)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        let api_secret = decrypt_fernet(encrypted_secret.trim(), &self.encryption_key)
            .map_err(|_| AdminPortError::LookupFailed)?;
        if api_secret.trim().is_empty() {
            return Err(AdminPortError::LookupFailed);
        }
        Ok((api_key, api_secret.trim().to_string()))
    }

    async fn update_admin_api_auth(
        &self,
        username: &str,
        api_key: &str,
        api_secret: &str,
    ) -> Result<(), AdminPortError> {
        let user = normalized_user(username);
        let api_key = api_key.trim();
        let api_secret = api_secret.trim();
        if api_key.is_empty() || api_secret.is_empty() {
            return Err(AdminPortError::LookupFailed);
        }

        let encrypted_secret = encrypt_fernet(api_secret, &self.encryption_key)
            .map_err(|_| AdminPortError::LookupFailed)?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        sqlx::query(
            r#"
            UPDATE tabUser
            SET api_key = ?
            WHERE name = ?
            "#,
        )
        .bind(api_key)
        .bind(&user)
        .execute(&mut *tx)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        sqlx::query(
            r#"
            INSERT INTO __Auth (doctype, name, fieldname, password, encrypted)
            VALUES ('User', ?, 'api_secret', ?, 1)
            ON DUPLICATE KEY UPDATE password = VALUES(password), encrypted = VALUES(encrypted)
            "#,
        )
        .bind(&user)
        .bind(encrypted_secret)
        .execute(&mut *tx)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        tx.commit().await.map_err(|_| AdminPortError::LookupFailed)
    }
}

fn normalized_user(username: &str) -> String {
    let user = username.trim();
    if user.is_empty() {
        "Administrator".to_string()
    } else {
        user.to_string()
    }
}

fn encrypt_fernet(plaintext: &str, encryption_key: &str) -> Result<String, String> {
    let key = decode_fernet_key(encryption_key)?;
    let signing_key = &key[..16];
    let encryption_key = &key[16..];

    let mut iv = [0_u8; 16];
    rand::rng().fill(&mut iv);
    let ciphertext = Aes128CbcEnc::new(encryption_key.into(), (&iv).into())
        .encrypt_padded_vec_mut::<Pkcs7>(plaintext.as_bytes());

    let mut payload = Vec::with_capacity(1 + 8 + iv.len() + ciphertext.len() + 32);
    payload.push(0x80);
    payload.extend_from_slice(&(OffsetDateTime::now_utc().unix_timestamp() as u64).to_be_bytes());
    payload.extend_from_slice(&iv);
    payload.extend_from_slice(&ciphertext);

    let mut mac = HmacSha256::new_from_slice(signing_key).map_err(|error| error.to_string())?;
    mac.update(&payload);
    payload.extend_from_slice(&mac.finalize().into_bytes());
    Ok(URL_SAFE.encode(payload))
}

fn decrypt_fernet(token: &str, encryption_key: &str) -> Result<String, String> {
    let key = decode_fernet_key(encryption_key)?;
    let signing_key = &key[..16];
    let encryption_key = &key[16..];
    let raw = URL_SAFE.decode(token).map_err(|error| error.to_string())?;
    if raw.len() < 1 + 8 + 16 + 32 || raw[0] != 0x80 {
        return Err("invalid fernet token".to_string());
    }
    let mac_offset = raw.len() - 32;
    let payload = &raw[..mac_offset];
    let expected_mac = &raw[mac_offset..];
    let mut mac = HmacSha256::new_from_slice(signing_key).map_err(|error| error.to_string())?;
    mac.update(payload);
    mac.verify_slice(expected_mac)
        .map_err(|_| "invalid fernet token signature".to_string())?;

    let iv = &raw[9..25];
    let ciphertext = &raw[25..mac_offset];
    let plaintext = Aes128CbcDec::new(encryption_key.into(), iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .map_err(|error| error.to_string())?;
    String::from_utf8(plaintext).map_err(|error| error.to_string())
}

fn decode_fernet_key(encryption_key: &str) -> Result<Vec<u8>, String> {
    let key = encryption_key.trim();
    if key.is_empty() {
        return Err("encryption key is required".to_string());
    }
    let decoded = URL_SAFE.decode(key).map_err(|error| error.to_string())?;
    if decoded.len() != 32 {
        return Err("invalid encryption key length".to_string());
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::{decrypt_fernet, encrypt_fernet};
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE;

    #[test]
    fn fernet_round_trip_matches_frappe_shape() {
        let key = URL_SAFE.encode([7_u8; 32]);
        let token = encrypt_fernet("secret-value", &key).expect("encrypt");

        assert!(token.starts_with("gAAAA"));
        assert_eq!(
            decrypt_fernet(&token, &key).expect("decrypt"),
            "secret-value"
        );
    }
}
