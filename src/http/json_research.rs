//! Opt-in JSON codec research at the HTTP/WebSocket boundary.
//!
//! Persistent JSON, fingerprints, cursors and the serde_json DOM stay outside
//! this boundary. A normal build and an unconfigured research build use Serde.
use std::sync::OnceLock;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

pub const ENCODER_ENV: &str = "MINI_ERP_RESEARCH_JSON_ENCODER";
pub const DECODER_ENV: &str = "MINI_ERP_RESEARCH_JSON_DECODER";
static SETTINGS: OnceLock<Settings> = OnceLock::new();

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Engine {
    #[default]
    SerdeJson,
    SonicRs,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub encoder: Engine,
    pub decoder: Engine,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[cfg(feature = "sonic-json-research")]
    #[error(transparent)]
    Sonic(#[from] sonic_rs::Error),
    #[error("sonic_rs requires the sonic-json-research Cargo feature")]
    FeatureDisabled,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Self::SerdeJson => "serde_json",
            Self::SonicRs => "sonic_rs",
        }
    }

    fn configured(key: &'static str, value: &str) -> Result<Self, AppError> {
        match value {
            "" | "serde_json" => Ok(Self::SerdeJson),
            "sonic_rs" if cfg!(feature = "sonic-json-research") => Ok(Self::SonicRs),
            _ => Err(AppError::InvalidConfig {
                key,
                value: value.into(),
            }),
        }
    }

    pub fn to_vec<T: Serialize + ?Sized>(self, value: &T) -> Result<Vec<u8>, Error> {
        match self {
            Self::SerdeJson => Ok(serde_json::to_vec(value)?),
            Self::SonicRs => {
                #[cfg(feature = "sonic-json-research")]
                {
                    Ok(sonic_rs::to_vec(value)?)
                }
                #[cfg(not(feature = "sonic-json-research"))]
                {
                    Err(Error::FeatureDisabled)
                }
            }
        }
    }

    pub fn to_string<T: Serialize + ?Sized>(self, value: &T) -> Result<String, Error> {
        match self {
            Self::SerdeJson => Ok(serde_json::to_string(value)?),
            Self::SonicRs => {
                #[cfg(feature = "sonic-json-research")]
                {
                    Ok(sonic_rs::to_string(value)?)
                }
                #[cfg(not(feature = "sonic-json-research"))]
                {
                    Err(Error::FeatureDisabled)
                }
            }
        }
    }

    pub fn from_slice<'de, T: Deserialize<'de>>(self, body: &'de [u8]) -> Result<T, Error> {
        match self {
            Self::SerdeJson => Ok(serde_json::from_slice(body)?),
            Self::SonicRs => {
                #[cfg(feature = "sonic-json-research")]
                {
                    Ok(sonic_rs::from_slice(body)?)
                }
                #[cfg(not(feature = "sonic-json-research"))]
                {
                    Err(Error::FeatureDisabled)
                }
            }
        }
    }

    pub fn response<T: Serialize>(self, value: T) -> Response {
        if self == Self::SerdeJson {
            // Preserve Axum's exact existing success/error behavior by default.
            return Json(value).into_response();
        }
        match self.to_vec(&value) {
            Ok(bytes) => (
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                bytes,
            )
                .into_response(),
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        }
    }
}

impl Settings {
    pub fn from_names(encoder: &str, decoder: &str) -> Result<Self, AppError> {
        Ok(Self {
            encoder: Engine::configured(ENCODER_ENV, encoder)?,
            decoder: Engine::configured(DECODER_ENV, decoder)?,
        })
    }
}

pub fn initialize_from_env() -> Result<Settings, AppError> {
    let read = |key| match std::env::var(key) {
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => Ok(String::new()),
        Err(std::env::VarError::NotUnicode(_)) => Err(AppError::InvalidConfig {
            key,
            value: "non-UTF-8 value".into(),
        }),
    };
    let configured = Settings::from_names(&read(ENCODER_ENV)?, &read(DECODER_ENV)?)?;
    if let Some(current) = SETTINGS.get() {
        if *current != configured {
            return Err(AppError::Storage(
                "JSON research settings already initialized".into(),
            ));
        }
    } else {
        SETTINGS
            .set(configured)
            .map_err(|_| AppError::Storage("JSON research settings already initialized".into()))?;
    }
    tracing::info!(
        encoder = configured.encoder.name(),
        decoder = configured.decoder.name(),
        "HTTP/production stream JSON codecs initialized"
    );
    Ok(configured)
}

pub fn settings() -> Settings {
    SETTINGS.get().copied().unwrap_or_default()
}

pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String, Error> {
    settings().encoder.to_string(value)
}

pub fn to_vec<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, Error> {
    settings().encoder.to_vec(value)
}

pub fn value_to_string(value: &serde_json::Value) -> String {
    to_string(value).expect("serializable wire JSON value")
}

pub fn from_slice<'de, T: Deserialize<'de>>(body: &'de [u8]) -> Result<T, Error> {
    settings().decoder.from_slice(body)
}

pub fn response<T: Serialize>(value: T) -> Response {
    settings().encoder.response(value)
}
