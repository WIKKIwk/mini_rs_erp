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
mod transfers;

use super::*;

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use async_trait::async_trait;

use crate::core::apparatus_standard::ApparatusId;

pub use state::MemoryProductionMapStore;

include!("memory_store_impl_parts/part_01.rs");
include!("memory_store_impl_parts/part_02.rs");

include!("memory_store_trait_impl.rs");
