use super::*;

#[test]
fn telegram_qr_payload_is_url_safe_login_token_not_bot_invite() {
    let token = [255, 254, 253, 1];
    let url = login_url(&token);
    assert!(url.starts_with("tg://login?token="));
    assert_eq!(url.matches('=').count(), 1);
    assert_eq!(
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(url.split_once('=').unwrap().1)
            .unwrap(),
        token
    );
}

#[tokio::test]
async fn telegram_qr_challenge_is_owner_bound_and_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let service = QrLoginService::new(Arc::new(TelegramStore::new(
        dir.path().join("telegram.json"),
    )));
    let entry = Arc::new(Entry {
        owner: "admin:a".into(),
        cancelled: AtomicBool::new(false),
        pending: Mutex::new(Pending {
            role: TelegramAccountRole::Admin,
            deadline: Instant::now() + LOGIN_TTL,
            connection: None,
            status: QrLoginStatus {
                login_id: "challenge".into(),
                status: "authorized",
                qr_url: None,
                expires_at_unix: None,
                password_hint: None,
                error_code: None,
                user: None,
            },
        }),
    });
    service
        .entries
        .lock()
        .await
        .insert("challenge".into(), entry.clone());
    assert!(service.poll("admin:b", "challenge", None).await.is_err());
    assert!(service.cancel("admin:b", "challenge").await.is_err());
    assert_eq!(
        service
            .poll("admin:a", "challenge", None)
            .await
            .unwrap()
            .status,
        "authorized"
    );
    service.cancel("admin:a", "challenge").await.unwrap();
    assert!(entry.cancelled.load(Ordering::Acquire));
    assert!(service.entry("admin:a", "challenge").await.is_err());
}

/// Opt-in, read-only Telegram protocol smoke test. No scan, bot message,
/// account registration or production store mutation is performed.
#[tokio::test]
#[ignore = "requires Telegram API credentials and network"]
async fn telegram_qr_live_export_returns_login_uri() {
    let _ = dotenvy::from_filename(".env");
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TelegramStore::new(dir.path().join("telegram.json")));
    let service = QrLoginService::new(store.clone());
    let start = service
        .start("smoke-test".into(), TelegramAccountRole::SalesManager)
        .await
        .unwrap();
    let status = service
        .poll("smoke-test", &start.login_id, None)
        .await
        .unwrap();
    assert_eq!(
        status.status, "waiting",
        "QR state error: {:?}",
        status.error_code
    );
    let url = status.qr_url.unwrap();
    assert!(url.starts_with("tg://login?token="));
    assert!(status.expires_at_unix.unwrap() > now());
    let polled = service
        .poll("smoke-test", &start.login_id, None)
        .await
        .unwrap();
    assert!(
        polled.qr_url.as_deref() == Some(url.as_str()),
        "QR must remain stable before scan/expiry"
    );
    service.cancel("smoke-test", &start.login_id).await.unwrap();
    assert!(store.users().await.unwrap().is_empty());
}
