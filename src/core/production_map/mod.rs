mod apparatus;
mod apparatus_resolver;
mod capacity;
pub mod chain;
mod compiler;
mod errors;
mod formula;
mod formula_parser;
pub mod materials;
mod materials_support;
#[cfg(test)]
mod memory_store;
mod opening_wip;
pub mod pechat;
#[path = "progress_session/mod.rs"]
mod progress;
mod queue;
pub mod queue_state;
mod service;
mod service_astatka;
mod service_audit;
mod service_completion {
    include!("progress_session/service_completion.rs");
}
mod service_capacity;
mod service_capacity_scheduler;
mod service_maps;
mod service_opening_wip;
mod service_order_control;
mod service_progress {
    include!("progress_session/service_progress.rs");
}
mod service_progress_correction {
    include!("progress_session/service_progress_correction.rs");
}
mod service_progress_metrics {
    include!("progress_session/service_progress_metrics.rs");
}
mod service_progress_support {
    include!("progress_session/service_progress_support.rs");
}
mod service_paddon;
mod service_qolip;
mod service_queue_support;
mod service_transfer;
mod service_wip;
mod store_port;
mod types;

#[cfg(test)]
pub(crate) use apparatus_resolver::TestCanonicalApparatusResolver;
pub use apparatus_resolver::{CanonicalApparatusResolver, CanonicalServiceApparatusResolver};
pub use capacity::*;
pub use compiler::{compile_map, run_map_with_variables};
#[cfg(test)]
pub use materials::ApparatusMaterialRuleUpsert;
pub use materials::{
    ApparatusMaterialRequirementGroup, ApparatusMaterialRule, MaterialScanProgressAction,
    RawMaterialAssignment, RawMaterialAssignmentDeleteInput, RawMaterialAssignmentInput,
    RawMaterialStartPolicy, RawMaterialStartRequirements, TrustedQolipStartValidation,
};
#[cfg(test)]
pub use memory_store::MemoryProductionMapStore;
pub use opening_wip::*;
pub(crate) use progress::progress_label_item_name;
pub(crate) use progress::{
    derive_production_order_lifecycle_with_completed_stage_nodes,
    derive_production_order_operational_status,
};
pub(crate) use progress::{progress_batch_id, progress_qr_payload};
pub use service::{PreparedApparatusQueueAction, ProductionMapLiveSnapshot, ProductionMapService};
pub(crate) use store_port::validate_queue_progress_write;
pub use store_port::{
    ProductionMapApparatusTransferWrite, ProductionMapStorePort, QueueActionProgressWrite,
    QueueActionProgressWriteResult, RawMaterialStockTransition, RawMaterialStockTransitionKind,
};
pub use types::*;

#[cfg(test)]
mod tests;
