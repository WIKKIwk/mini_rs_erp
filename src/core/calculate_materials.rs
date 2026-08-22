use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

include!("calculate_materials_parts/part_01.rs");
include!("calculate_materials_parts/part_02.rs");
