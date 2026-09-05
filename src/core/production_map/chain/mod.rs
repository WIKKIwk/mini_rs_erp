use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::compiler::normalize_branch;
use super::queue_state::ApparatusQueueOrderState;
use super::{ProductionMapDefinition, ProductionMapEdge, ProductionMapNode, ProductionMapNodeKind};
use crate::core::apparatus_standard::ApparatusId;

include!("mod_parts/part_01.rs");
include!("mod_parts/part_02.rs");
