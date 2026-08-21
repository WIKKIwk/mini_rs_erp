use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};

use crate::core::admin::models::AdminWarehouse;
use crate::core::apparatus_standard::ApparatusId;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::production_map::CanonicalApparatusResolver;

include!("warehouses_parts/part_01.rs");
include!("warehouses_parts/part_02.rs");
