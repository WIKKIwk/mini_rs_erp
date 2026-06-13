use data_encoding::BASE32_NOPAD;
use sha2::{Digest, Sha256};

use crate::core::auth::service::{AuthError, normalize_phone};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupplierAccessInput {
    pub ref_: String,
    pub name: String,
    pub phone: String,
}

pub fn supplier_access_code(input: &SupplierAccessInput) -> Result<String, AuthError> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AuthError::InvalidCredentials);
    }

    let mut seed = input.ref_.trim().to_string();
    if seed.is_empty()
        && let Ok(phone) = normalize_phone(input.phone.trim())
    {
        seed = phone;
    }
    if seed.is_empty() {
        seed = name.to_string();
    }

    Ok(format!("10{}", hash_token(&seed, 10)))
}

fn hash_token(value: &str, length: usize) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let encoded = BASE32_NOPAD.encode(&digest).to_uppercase();

    if length == 0 || length >= encoded.len() {
        encoded
    } else {
        encoded[..length].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{SupplierAccessInput, supplier_access_code};

    #[test]
    fn supplier_code_is_deterministic_and_prefixed() {
        let input = SupplierAccessInput {
            ref_: "SUP-001".to_string(),
            name: "Abdulloh".to_string(),
            phone: "+998901234567".to_string(),
        };

        let first = supplier_access_code(&input).expect("first code");
        let second = supplier_access_code(&input).expect("second code");

        assert_eq!(first, second);
        assert!(first.starts_with("10"));
        assert_eq!(first.len(), 12);
    }
}
