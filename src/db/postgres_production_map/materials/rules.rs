use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{
    ProductionMapError, QueueActionActor, RawMaterialAssignment,
};
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, insert_raw_material_event_tx,
};

include!("rules_parts/part_01.rs");
include!("rules_parts/part_02.rs");
