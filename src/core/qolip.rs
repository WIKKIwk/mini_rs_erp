mod memory_store;
mod models;
pub(crate) mod normalize;
mod normalize_qr;
mod ports;
mod service;
mod shared_sets;
mod set_ids;
mod management;
#[cfg(test)]
mod service_tests;

pub use memory_store::MemoryQolipStore;
pub use models::{
    QolipBlock, QolipCellQr, QolipCellQrInput, QolipCheckout, QolipCheckoutCreate,
    QolipCheckoutReturn, QolipError, QolipLocation, QolipLocationMove, QolipLocationMoveBatch,
    QolipLocationUpsert, QolipOrderStartPreparation, QolipProduct, QolipProductSpec,
    QolipProductSpecBatchUpsert, QolipProductSpecDelete, QolipProductSpecUpsert,
    QolipProductTransfer,
};
pub use normalize::role_code;
pub use normalize_qr::qolip_scan_code_from_qr;
pub use ports::QolipStorePort;
pub use service::{QolipService, selected_product_specs};
pub use shared_sets::{QolipSetProduct, QolipSetReuse, QolipSetSnapshot};
pub use management::{QolipManagedSet, QolipMoldDraft, QolipSetEdit, QolipProductMoldEdit, QolipProductMoldEdits};
pub(crate) use shared_sets::candidate_codes;
