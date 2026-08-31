use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::queue_state;

use super::{
    ProductionMapDefinition, ProductionOrderLifecycleStatus, ProductionOrderLogEntry,
    QueueActionActor,
};

include!("progress_parts/part_01.rs");
include!("progress_parts/part_02.rs");
include!("progress_parts/part_03.rs");
include!("progress_parts/part_04.rs");
