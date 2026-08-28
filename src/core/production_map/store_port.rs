use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;

use crate::core::apparatus_standard::ApparatusId;
use crate::core::qolip::QolipCheckout;
use crate::core::returned_paint::ReturnedPaintRequest;

use super::capacity::*;
use super::materials::RawMaterialAssignment;
use super::opening_wip::*;
use super::types::*;

include!("store_port_parts/part_01.rs");
include!("store_port_parts/part_02.rs");
include!("store_port_parts/part_03.rs");
