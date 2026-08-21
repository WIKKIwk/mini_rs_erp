use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;

use crate::core::apparatus_standard::ApparatusId;
use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderTemplate, hydrate_template_layers, validate_template,
};
use crate::core::production_map::{
    OrderProgressBatch, ProductionMapDefinition, ProductionMapNodeKind, ProductionMapProgram,
    ProductionMapSaved, compile_map,
};
use crate::core::production_map::{progress_batch_id, progress_qr_payload, queue_state};
use crate::core::returned_paint::{ReturnedPaintCalculation, ReturnedPaintItem};

#[path = "postgres_training_workspace_delete.rs"]
mod postgres_training_workspace_delete;

include!("postgres_training_workspace_parts/part_01.rs");
include!("postgres_training_workspace_parts/part_02.rs");
