use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use data_encoding::HEXLOWER;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::core::apparatus_standard::ApparatusId;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::text::{lowercase_ascii_owned, trim_owned};

include!("inventory_movements_parts/part_01.rs");
include!("inventory_movements_parts/part_02.rs");
