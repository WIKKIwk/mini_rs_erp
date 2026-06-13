use std::sync::Arc;

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::push::service::{PushService, push_token_key};
use crate::store::push_token_store::PushTokenStore;

#[tokio::test]
async fn push_token_key_matches_go_role_ref_format() {
    assert_eq!(
        push_token_key(&principal(PrincipalRole::Supplier, "SUP-001")),
        "supplier:SUP-001"
    );
    assert_eq!(
        push_token_key(&principal(PrincipalRole::Werka, "werka")),
        "werka:werka"
    );
}

#[tokio::test]
async fn move_token_to_key_removes_previous_owner_only_like_go() {
    let store = Arc::new(PushTokenStore::new(unique_path()));
    let service = PushService::new(store.clone());
    service
        .register(
            &principal(PrincipalRole::Supplier, "SUP-001"),
            "device-a",
            "ios",
        )
        .await
        .expect("register a");
    service
        .register(
            &principal(PrincipalRole::Supplier, "SUP-001"),
            "shared",
            "ios",
        )
        .await
        .expect("register shared");
    service
        .register(
            &principal(PrincipalRole::Werka, "werka"),
            "shared",
            "android",
        )
        .await
        .expect("move shared");

    let supplier = crate::core::push::ports::PushTokenStorePort::list(&*store, "supplier:SUP-001")
        .await
        .expect("supplier list");
    let werka = crate::core::push::ports::PushTokenStorePort::list(&*store, "werka:werka")
        .await
        .expect("werka list");

    assert_eq!(supplier.len(), 1);
    assert_eq!(supplier[0].token, "device-a");
    assert_eq!(werka.len(), 1);
    assert_eq!(werka[0].token, "shared");
    assert_eq!(werka[0].platform, "android");
}

fn principal(role: PrincipalRole, ref_: &str) -> Principal {
    Principal {
        role,
        display_name: "User".to_string(),
        legal_name: "User".to_string(),
        ref_: ref_.to_string(),
        phone: String::new(),
        avatar_url: String::new(),
    }
}

fn unique_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "accord-push-service-{}-{}.json",
        std::process::id(),
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    ))
}
