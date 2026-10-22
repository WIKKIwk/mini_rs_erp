//! Application WebSocket boundary. Text frames retain the existing JSON contract;
//! RFC 7692 compression is negotiated by the patched, pinned Sockudo transport.

use std::fmt;
use std::future::Future;
use std::ops::Deref;
use std::sync::OnceLock;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use sockudo_ws::{Compression, Config, DeflateConfig};

use crate::error::AppError;

/// Validated UTF-8 with cheap clones for the shared prepared-frame cache.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Utf8Bytes(Bytes);

impl Utf8Bytes {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        // Construction validates the invariant, without unsafe conversions.
        std::str::from_utf8(&self.0).expect("Utf8Bytes contains validated UTF-8")
    }
}

impl Deref for Utf8Bytes {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl From<String> for Utf8Bytes {
    fn from(value: String) -> Self {
        Self(Bytes::from(value))
    }
}

impl From<&str> for Utf8Bytes {
    fn from(value: &str) -> Self {
        Self(Bytes::copy_from_slice(value.as_bytes()))
    }
}

impl fmt::Display for Utf8Bytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(formatter)
    }
}

#[derive(Clone, Debug)]
pub enum Message {
    Text(Utf8Bytes),
    Binary(Bytes),
    Ping(Bytes),
    Pong(Bytes),
    Close(Option<sockudo_ws::error::CloseReason>),
}

impl From<Message> for sockudo_ws::Message {
    fn from(message: Message) -> Self {
        match message {
            Message::Text(text) => Self::Text(text.0),
            Message::Binary(data) => Self::Binary(data),
            Message::Ping(data) => Self::Ping(data),
            Message::Pong(data) => Self::Pong(data),
            Message::Close(reason) => Self::Close(reason),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WebSocketError {
    #[error(transparent)]
    Protocol(#[from] sockudo_ws::Error),
    #[error("invalid UTF-8 WebSocket text")]
    InvalidText(#[from] std::str::Utf8Error),
}

impl TryFrom<sockudo_ws::Message> for Message {
    type Error = WebSocketError;

    fn try_from(message: sockudo_ws::Message) -> Result<Self, Self::Error> {
        Ok(match message {
            sockudo_ws::Message::Text(data) => {
                std::str::from_utf8(&data)?;
                Self::Text(Utf8Bytes(data))
            }
            sockudo_ws::Message::Binary(data) => Self::Binary(data),
            sockudo_ws::Message::Ping(data) => Self::Ping(data),
            sockudo_ws::Message::Pong(data) => Self::Pong(data),
            sockudo_ws::Message::Close(reason) => Self::Close(reason),
        })
    }
}

pub struct WebSocket {
    inner: sockudo_ws::axum_integration::WebSocket,
}

impl WebSocket {
    pub async fn send(&mut self, message: Message) -> Result<(), WebSocketError> {
        self.inner.send(message.into()).await.map_err(Into::into)
    }

    pub async fn recv(&mut self) -> Option<Result<Message, WebSocketError>> {
        self.inner
            .recv()
            .await
            .map(|message| message.map_err(Into::into).and_then(Message::try_from))
    }

    pub async fn close(self, code: u16, reason: &str) -> Result<(), WebSocketError> {
        self.inner.close(code, reason).await.map_err(Into::into)
    }
}

pub struct WebSocketUpgrade(sockudo_ws::axum_integration::WebSocketUpgrade);

#[derive(Debug)]
pub enum WebSocketUpgradeRejection {
    Protocol(sockudo_ws::axum_integration::WebSocketUpgradeRejection),
    Configuration,
}

impl IntoResponse for WebSocketUpgradeRejection {
    fn into_response(self) -> Response {
        match self {
            Self::Protocol(rejection) => rejection.into_response(),
            Self::Configuration => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "WebSocket transport configuration is invalid",
            )
                .into_response(),
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for WebSocketUpgrade {
    type Rejection = WebSocketUpgradeRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        static CONFIG: OnceLock<Result<Config, AppError>> = OnceLock::new();
        let config = CONFIG
            .get_or_init(|| {
                TransportSettings::from_lookup(read_env).map(|settings| settings.config())
            })
            .as_ref()
            .map_err(|_| WebSocketUpgradeRejection::Configuration)?;
        let upgrade =
            sockudo_ws::axum_integration::WebSocketUpgrade::from_request_parts(parts, state)
                .await
                .map_err(WebSocketUpgradeRejection::Protocol)?;
        Ok(Self(upgrade.config(config.clone())))
    }
}

impl WebSocketUpgrade {
    /// Explicit configuration is useful for controlled transport comparisons.
    pub fn config(mut self, config: Config) -> Self {
        self.0 = self.0.config(config);
        self
    }

    pub fn on_upgrade<F, Fut>(
        self,
        handler: F,
    ) -> sockudo_ws::axum_integration::WebSocketUpgradeResponse
    where
        F: FnOnce(WebSocket) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.0.on_upgrade(move |inner| handler(WebSocket { inner }))
    }
}

/// Deterministic default, independent of process environment, for benchmarks.
pub fn low_traffic_config() -> Config {
    TransportSettings::default().config()
}

/// Fail startup on invalid transport settings; do not silently fall back.
pub(crate) fn validate_environment() -> Result<(), AppError> {
    TransportSettings::from_lookup(read_env).map(|_| ())
}

fn read_env(key: &'static str) -> Result<Option<String>, AppError> {
    match std::env::var(key) {
        Ok(value) => Ok(Some(value.trim().to_owned())),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(invalid(key, "must be UTF-8")),
    }
}

fn invalid(key: &'static str, reason: &str) -> AppError {
    AppError::InvalidConfig {
        key,
        value: reason.to_owned(),
    }
}

#[derive(Debug)]
struct TransportSettings {
    compression: Compression,
    level: u32,
    threshold: usize,
    window_bits: u8,
    context_takeover: bool,
    max_message: usize,
    max_frame: usize,
    max_backpressure: usize,
    ping_seconds: u32,
    pong_seconds: u32,
    idle_seconds: u32,
}

impl Default for TransportSettings {
    fn default() -> Self {
        Self {
            compression: Compression::Dedicated,
            level: 3,
            threshold: 128,
            window_bits: 15,
            context_takeover: true,
            max_message: 64 * 1024 * 1024,
            max_frame: 16 * 1024 * 1024,
            max_backpressure: 64 * 1024 * 1024,
            ping_seconds: 25,
            pong_seconds: 20,
            idle_seconds: 120,
        }
    }
}

impl TransportSettings {
    fn from_lookup(
        mut lookup: impl FnMut(&'static str) -> Result<Option<String>, AppError>,
    ) -> Result<Self, AppError> {
        let mut settings = Self::default();
        const MODE: &str = "MINI_ERP_WS_COMPRESSION";
        if let Some(value) = lookup(MODE)? {
            settings.compression = match value.as_str() {
                "dedicated" => Compression::Dedicated,
                "shared" => Compression::Shared,
                "disabled" => Compression::Disabled,
                _ => return Err(invalid(MODE, "must be dedicated, shared or disabled")),
            };
        }
        const TAKEOVER: &str = "MINI_ERP_WS_CONTEXT_TAKEOVER";
        if let Some(value) = lookup(TAKEOVER)? {
            settings.context_takeover = match value.as_str() {
                "1" | "true" => true,
                "0" | "false" => false,
                _ => return Err(invalid(TAKEOVER, "must be true or false")),
            };
        }
        macro_rules! number {
            ($field:ident, $key:literal, $min:expr, $max:expr) => {
                if let Some(raw) = lookup($key)? {
                    let value: u64 = raw
                        .parse()
                        .map_err(|_| invalid($key, "must be an unsigned integer"))?;
                    if !($min..=$max).contains(&value) {
                        return Err(invalid($key, "value is out of range"));
                    }
                    settings.$field = value as _;
                }
            };
        }
        number!(level, "MINI_ERP_WS_COMPRESSION_LEVEL", 0, 9);
        number!(
            threshold,
            "MINI_ERP_WS_COMPRESSION_THRESHOLD",
            0,
            1024 * 1024
        );
        // flate2 supports encoder windows of 9..=15 bits, not the advertised 8.
        number!(window_bits, "MINI_ERP_WS_WINDOW_BITS", 9, 15);
        number!(
            max_message,
            "MINI_ERP_WS_MAX_MESSAGE_BYTES",
            125,
            64 * 1024 * 1024
        );
        number!(
            max_frame,
            "MINI_ERP_WS_MAX_FRAME_BYTES",
            125,
            64 * 1024 * 1024
        );
        number!(
            max_backpressure,
            "MINI_ERP_WS_MAX_BACKPRESSURE_BYTES",
            1024,
            64 * 1024 * 1024
        );
        number!(ping_seconds, "MINI_ERP_WS_PING_SECONDS", 1, 300);
        number!(pong_seconds, "MINI_ERP_WS_PONG_SECONDS", 1, 300);
        number!(idle_seconds, "MINI_ERP_WS_IDLE_SECONDS", 3, 3600);
        if settings.max_frame > settings.max_message {
            return Err(invalid(
                "MINI_ERP_WS_MAX_FRAME_BYTES",
                "must not exceed max message bytes",
            ));
        }
        if settings.idle_seconds <= settings.ping_seconds + settings.pong_seconds {
            return Err(invalid(
                "MINI_ERP_WS_IDLE_SECONDS",
                "must exceed ping interval plus pong deadline",
            ));
        }
        Ok(settings)
    }

    fn config(&self) -> Config {
        let mut config = Config::builder()
            .compression(self.compression)
            .max_message_size(self.max_message)
            .max_frame_size(self.max_frame)
            .max_backpressure(self.max_backpressure)
            .write_buffer_size(16 * 1024)
            .auto_ping(true)
            .ping_interval(self.ping_seconds)
            .pong_timeout(self.pong_seconds)
            .idle_timeout(self.idle_seconds)
            .build();
        if self.compression != Compression::Disabled {
            config.deflate = Some(DeflateConfig {
                server_max_window_bits: self.window_bits,
                client_max_window_bits: 15,
                server_no_context_takeover: !self.context_takeover
                    || self.compression == Compression::Shared,
                client_no_context_takeover: !self.context_takeover,
                compression_level: self.level,
                compression_threshold: self.threshold,
            });
        }
        config
    }
}

#[cfg(test)]
mod tests;
