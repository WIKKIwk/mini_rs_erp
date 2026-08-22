use crate::core::apparatus_standard::ApparatusId;

use super::compiler::compile_map;
use super::queue_state;
use super::service::ProductionMapService;
use super::store_port::ProductionMapApparatusTransferWrite;
use super::types::*;

include!("service_parts/part_01.rs");
include!("service_parts/part_02.rs");
