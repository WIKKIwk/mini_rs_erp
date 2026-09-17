//! New-account QR authorization. Never encode bot invite links as login QR codes.
use super::*;
use base64::Engine;
use ferogram::{UpdateStream, tl};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use time::OffsetDateTime;
use tokio::time::{Instant, timeout};

const LOGIN_TTL: Duration = Duration::from_secs(300);
const RPC_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Serialize)]
pub struct QrLoginStatus {
    pub login_id: String,
    pub status: &'static str,
    pub qr_url: Option<String>,
    pub expires_at_unix: Option<i64>,
    pub password_hint: Option<String>,
    pub error_code: Option<&'static str>,
    pub user: Option<TelegramUserAccount>,
}

struct Connection {
    client: Client,
    shutdown: ShutdownToken,
    updates: UpdateStream,
    api_id: i32,
    api_hash: String,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

impl Connection {
    async fn connect(store: &TelegramStore, dc: Option<i32>) -> Result<Self, &'static str> {
        let (api_id, api_hash) = store
            .user_api_credentials()
            .await
            .map_err(|_| "store_failed")?
            .ok_or("not_configured")?;
        let mut builder = Client::builder()
            .api_id(api_id)
            .api_hash(&api_hash)
            .session_string("")
            .transport(TransportKind::Abridged)
            .probe_transport(false)
            .device_model("Accord Mobile - Mini RS ERP")
            .system_version("Rust")
            .app_version("Mini RS ERP 1.0")
            .system_lang_code("en-US")
            .lang_code("en")
            .resilient_connect(true);
        if let Some(dc) = dc {
            if !(1..=5).contains(&dc) {
                return Err("transport_failed");
            }
            builder = builder
                .dc_id_override(dc)
                .dc_addr(ferogram::dc_migration::fallback_dc_addr(dc));
        }
        let (client, shutdown) = builder.connect().await.map_err(|_| "transport_failed")?;
        let updates = client.stream_updates();
        Ok(Self {
            client,
            shutdown,
            updates,
            api_id,
            api_hash,
        })
    }

    async fn export(&self) -> Result<tl::enums::auth::LoginToken, ferogram::InvocationError> {
        self.client
            .invoke(&tl::functions::auth::ExportLoginToken {
                api_id: self.api_id,
                api_hash: self.api_hash.clone(),
                except_ids: vec![],
            })
            .await
    }
}

struct Pending {
    role: TelegramAccountRole,
    deadline: Instant,
    connection: Option<Connection>,
    status: QrLoginStatus,
}

struct Entry {
    owner: String,
    cancelled: AtomicBool,
    pending: Mutex<Pending>,
}

#[derive(Clone)]
pub(crate) struct QrLoginService {
    store: Arc<TelegramStore>,
    entries: Arc<Mutex<BTreeMap<String, Arc<Entry>>>>,
}

impl QrLoginService {
    pub fn new(store: Arc<TelegramStore>) -> Self {
        Self {
            store,
            entries: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub async fn start(
        &self,
        owner: String,
        role: TelegramAccountRole,
    ) -> Result<QrLoginStatus, &'static str> {
        self.store
            .user_api_credentials()
            .await
            .map_err(|_| "store_failed")?
            .ok_or("not_configured")?;
        let mut entries = self.entries.lock().await;
        if entries.len() >= 32 {
            return Err("too_many_logins");
        }
        if entries.values().any(|entry| entry.owner == owner) {
            return Err("login_pending");
        }
        let id =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
        let status = QrLoginStatus {
            login_id: id.clone(),
            status: "loading",
            qr_url: None,
            expires_at_unix: None,
            password_hint: None,
            error_code: None,
            user: None,
        };
        entries.insert(
            id.clone(),
            Arc::new(Entry {
                owner,
                cancelled: AtomicBool::new(false),
                pending: Mutex::new(Pending {
                    role,
                    deadline: Instant::now() + LOGIN_TTL,
                    connection: None,
                    status: status.clone(),
                }),
            }),
        );
        let entries = Arc::downgrade(&self.entries);
        // Closing the app or losing connectivity must not leave a login client alive.
        tokio::spawn(async move {
            tokio::time::sleep(LOGIN_TTL).await;
            if let Some(entries) = entries.upgrade() {
                let entry = entries.lock().await.remove(&id);
                if let Some(entry) = entry {
                    entry.cancelled.store(true, Ordering::Release);
                    entry.pending.lock().await.connection.take();
                }
            }
        });
        Ok(status)
    }

    async fn entry(&self, owner: &str, id: &str) -> Result<Arc<Entry>, &'static str> {
        self.entries
            .lock()
            .await
            .get(id)
            .filter(|entry| entry.owner == owner)
            .cloned()
            .ok_or("expired")
    }

    pub async fn cancel(&self, owner: &str, id: &str) -> Result<(), &'static str> {
        let entry = self.entry(owner, id).await?;
        entry.cancelled.store(true, Ordering::Release);
        self.entries.lock().await.remove(id);
        entry.pending.lock().await.connection.take();
        Ok(())
    }

    pub async fn poll(
        &self,
        owner: &str,
        id: &str,
        password: Option<String>,
    ) -> Result<QrLoginStatus, &'static str> {
        let entry = self.entry(owner, id).await?;
        let mut pending = entry.pending.lock().await;
        if entry.cancelled.load(Ordering::Acquire) || Instant::now() >= pending.deadline {
            return Err("expired");
        }
        if matches!(pending.status.status, "authorized" | "failed") {
            return Ok(pending.status.clone());
        }
        let result = timeout(
            RPC_TIMEOUT,
            pending.advance(&self.store, &entry.cancelled, password),
        )
        .await;
        if let Err(code) = result.unwrap_or(Err("transport_failed")) {
            pending.status.error_code = Some(code);
            if code != "invalid_password" {
                pending.status.status = "failed";
                pending.status.qr_url = None;
                pending.connection.take();
            }
        }
        Ok(pending.status.clone())
    }
}

impl Pending {
    async fn advance(
        &mut self,
        store: &TelegramStore,
        cancelled: &AtomicBool,
        password: Option<String>,
    ) -> Result<(), &'static str> {
        if self.connection.is_none() {
            self.connection = Some(Connection::connect(store, None).await?);
        }
        if self.status.status == "password_required" {
            let Some(password) = password else {
                return Ok(());
            };
            if password.is_empty() || password.len() > 1024 {
                return Err("invalid_password");
            }
            let client = &self.connection.as_ref().unwrap().client;
            // Fresh SRP parameters for every attempt; do not retain the user's password.
            let tl::enums::account::Password::Password(parameters) = client
                .invoke(&tl::functions::account::GetPassword {})
                .await
                .map_err(rpc_error)?;
            client
                .check_password(
                    PasswordToken {
                        password: parameters,
                    },
                    password,
                )
                .await
                .map_err(rpc_error)?;
            return self.finish(store, cancelled).await;
        }
        if self.status.status == "waiting" && self.status.expires_at_unix.unwrap_or(0) > now() {
            let updates = &mut self.connection.as_mut().unwrap().updates;
            // Re-export after updateLoginToken, or on expiry. Polling must not rotate
            // the QR every two seconds while Telegram's camera is reading it.
            let accepted = timeout(Duration::from_millis(5), async {
                while let Some(update) = updates.next_raw().await {
                    if update.constructor_id == 0x564fe691 {
                        return true;
                    }
                }
                false
            })
            .await
            .unwrap_or(false);
            if !accepted {
                return Ok(());
            }
        }
        // Use the raw response: ferogram 0.6.5's helper rejects LoginTokenSuccess
        // after DC migration, even though Telegram has authorized the session.
        let mut result = self.connection.as_ref().unwrap().export().await;
        for _ in 0..3 {
            match result {
                Ok(tl::enums::auth::LoginToken::LoginToken(token)) => {
                    if token.token.is_empty() || i64::from(token.expires) <= now() {
                        return Err("expired");
                    }
                    self.status.status = "waiting";
                    self.status.qr_url = Some(login_url(&token.token));
                    self.status.expires_at_unix = Some(i64::from(token.expires));
                    self.status.error_code = None;
                    return Ok(());
                }
                Ok(tl::enums::auth::LoginToken::MigrateTo(migration)) => {
                    // A fresh unauthenticated connection on the target DC owns the
                    // imported authorization and is the session we must persist.
                    self.connection = Some(Connection::connect(store, Some(migration.dc_id)).await?);
                    result = self
                        .connection
                        .as_ref()
                        .unwrap()
                        .client
                        .invoke(&tl::functions::auth::ImportLoginToken {
                            token: migration.token,
                        })
                        .await;
                }
                Ok(tl::enums::auth::LoginToken::Success(_)) => {
                    return self.finish(store, cancelled).await;
                }
                Err(error) if error.is("SESSION_PASSWORD_NEEDED") => {
                    let tl::enums::account::Password::Password(parameters) = self
                        .connection
                        .as_ref()
                        .unwrap()
                        .client
                        .invoke(&tl::functions::account::GetPassword {})
                        .await
                        .map_err(rpc_error)?;
                    self.status.status = "password_required";
                    self.status.password_hint = parameters.hint;
                    self.status.qr_url = None;
                    self.status.expires_at_unix = None;
                    return Ok(());
                }
                Err(error) => return Err(rpc_error(error)),
            }
        }
        Err("transport_failed")
    }

    async fn finish(
        &mut self,
        store: &TelegramStore,
        cancelled: &AtomicBool,
    ) -> Result<(), &'static str> {
        let client = &self.connection.as_ref().unwrap().client;
        let result = async {
            let me = client.get_me().await.map_err(rpc_error)?;
            if me.bot || me.id <= 0 {
                return Err("invalid_account");
            }
            let session = client
                .export_native_session_string()
                .await
                .map_err(rpc_error)?;
            if cancelled.load(Ordering::Acquire) {
                return Err("expired");
            }
            let account = TelegramUserAccount {
                telegram_user_id: me.id.to_string(),
                telegram_chat_id: String::new(),
                username: me.username.unwrap_or_default(),
                display_name: format!(
                    "{} {}",
                    me.first_name.unwrap_or_default(),
                    me.last_name.unwrap_or_default()
                )
                .trim()
                .to_string(),
                role: self.role,
                invite_token: String::new(),
                joined_at_unix: now(),
                phone_number: me.phone.unwrap_or_default(),
                delivery_mode: TelegramDeliveryMode::UserProfile,
                user_profile_connected: true,
                selected_chat_id: None,
                selected_chat_title: None,
                selected_chat_type: None,
            };
            store
                .register_qr_user(account, session)
                .await
                .map_err(|_| "store_failed")?
                .ok_or("already_registered")
        }
        .await;
        if result.is_err() {
            // Revoke only this fresh session, never an already registered user's session.
            let _ = timeout(
                Duration::from_secs(3),
                client.invoke(&tl::functions::auth::LogOut {}),
            )
            .await;
        }
        let account = result?;
        self.status.status = "authorized";
        self.status.user = Some(account);
        self.status.qr_url = None;
        self.status.expires_at_unix = None;
        self.status.password_hint = None;
        self.status.error_code = None;
        self.connection.take();
        Ok(())
    }
}

fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}
fn login_url(token: &[u8]) -> String {
    format!(
        "tg://login?token={}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(token)
    )
}
fn rpc_error(error: ferogram::InvocationError) -> &'static str {
    if error.is("PASSWORD_HASH_INVALID") {
        "invalid_password"
    } else if error.flood_wait_seconds().is_some() {
        "flood_wait"
    } else {
        "transport_failed"
    }
}

#[cfg(test)]
mod tests {
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
}
