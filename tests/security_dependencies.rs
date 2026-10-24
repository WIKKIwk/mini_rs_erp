use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde_json::{Value, json};

// Public test fixtures from jsonwebtoken 10.4.0 (MIT), never service credentials.
const PRIVATE_KEY: &[u8] = include_bytes!("fixtures/jwt/private_rsa_key_pkcs8.pem");
const PUBLIC_KEY: &[u8] = include_bytes!("fixtures/jwt/public_rsa_key_pkcs8.pem");

#[test]
fn security_dependency_update_preserves_rs256_service_account_assertions() {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let audience = "https://oauth2.googleapis.com/token";
    for scope in [
        "https://www.googleapis.com/auth/firebase.messaging",
        "https://www.googleapis.com/auth/spreadsheets",
    ] {
        let claims = json!({
            "iss": "synthetic@example.invalid", "aud": audience, "scope": scope,
            "iat": now, "exp": now + 3600,
        });
        let token = encode(
            &Header::new(Algorithm::RS256),
            &claims,
            &EncodingKey::from_rsa_pem(PRIVATE_KEY).unwrap(),
        )
        .expect("RS256 signing with the configured AWS LC crypto provider");
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[audience]);
        let verified = decode::<Value>(
            &token,
            &DecodingKey::from_rsa_pem(PUBLIC_KEY).unwrap(),
            &validation,
        )
        .expect("verify the signature and service-account claims");
        assert_eq!(verified.claims, claims);
    }
}

#[test]
fn security_dependency_update_rejects_malformed_optional_not_before_claims() {
    let secret = b"synthetic-test-secret-never-used-by-the-application";
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_nbf = true;
    for malformed in [json!("99999999999"), json!(false), json!([]), json!({})] {
        let claims = json!({"exp": now + 3600, "nbf": malformed});
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret),
        )
        .unwrap();
        assert!(decode::<Value>(&token, &DecodingKey::from_secret(secret), &validation).is_err());
    }
}
