#[path = "capacity/memory_store.rs"]
mod capacity;
#[path = "catalog/memory_store.rs"]
mod maps;
#[path = "materials/memory_store.rs"]
mod materials;
mod queue;
#[path = "progress_session/runs.rs"]
mod runs;
mod state;
mod temporary_pause;
mod transfers;
mod warehouse_wip;

use super::*;

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use async_trait::async_trait;

use crate::core::apparatus_standard::ApparatusId;

pub use state::MemoryProductionMapStore;

include!("memory_store_parts/maps.rs");

include!("memory_store_parts/progress_batches_for_audit.rs");

include!("memory_store_parts/opening_wip_by_idempotency_key.rs");

include!("memory_store_parts/receive_finished_goods_batch.rs");
