use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use data_encoding::HEXLOWER;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::core::apparatus_standard::{
    ApparatusId, CanonicalApparatusService, EquipmentClassId, LifecycleState, PhysicalAssetId,
    RuntimeApparatusProjection,
};

include!("factory_locations_parts/part_01.rs");
include!("factory_locations_parts/part_02.rs");
