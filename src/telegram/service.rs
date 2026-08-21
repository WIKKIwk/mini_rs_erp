use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use base64::Engine;
use time::OffsetDateTime;

use crate::core::calculate_orders::{CalculateOrderImage, CalculateOrderTemplate};
use crate::core::production_map::ProductionMapDefinition;

use super::models::{
    TelegramAdminOverview, TelegramBotSettings, TelegramBotSettingsUpdate, TelegramChat,
    TelegramDeliveryMode, TelegramInviteRequest, TelegramInviteResponse, TelegramStartRequest,
    TelegramUserAccount, TelegramUserGroup,
};
use super::order::TelegramOrderDraft;
use super::order_catalog::TelegramOrderCatalog;
use super::store::{TelegramStore, TelegramStoreError};
use super::useraccount::{CodeOutcome, LoginOutcome, TelegramUserAccountService, UserAccountError};

include!("service_parts/part_01.rs");
include!("service_parts/part_02.rs");
include!("service_parts/part_03.rs");
