//! Telegram user-account integration.
//!
//! This module owns MTProto login sessions. Bot API tokens are deliberately
//! kept in `telegram::bot` and are never reused as user-account credentials.

use std::collections::BTreeMap;
use std::env;
use std::io::Cursor;
use std::sync::Arc;

use ferogram::{
    Client, InputMessage, LoginToken, PasswordToken, SendCodeOutcome, ShutdownToken, SignInError,
    TransportKind,
};
use tokio::sync::Mutex;

use crate::core::calculate_orders::CalculateOrderImage;

use super::models::{TelegramDeliveryMode, TelegramUserGroup};
use super::store::{TelegramStore, TelegramStoreError};

include!("mod_parts/part_01.rs");
include!("mod_parts/part_02.rs");
