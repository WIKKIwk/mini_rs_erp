use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::Serialize;

use crate::app::AppState;
use crate::core::authz::Capability;
use crate::core::gscale::ProgressLabelPrintRequest;
use crate::core::qolip::{
    QolipBlock, QolipCellQrInput, QolipCheckoutCreate, QolipCheckoutReturn, QolipError,
    QolipLocationMove, QolipLocationMoveBatch, QolipLocationUpsert, QolipProduct,
    QolipProductSpecBatchUpsert, QolipProductSpecDelete, QolipProductSpecUpsert,
};
use crate::core::warehouses::{WarehouseDeleteRequest, WarehouseUpsert};

mod support;
mod order_products;
mod product_transfer;
mod shared_sets;
mod management;

pub use order_products::order_products;
pub use product_transfer::product_transfer;
pub use shared_sets::shared_sets;
pub use management::{sets, create_set, product_molds};

use self::support::*;
pub use self::support::{
    QolipBlockUpdate, QolipBlockUpsert, QolipCellQrLookupQuery, QolipCellQrPrintRequest,
    QolipCheckoutsQuery, QolipCodeQrPrintRequest, QolipErrorResponse, QolipSearchQuery,
};

#[derive(Serialize)]
struct QolipProductResponse {
    #[serde(flatten)]
    product: QolipProduct,
    #[serde(skip_serializing_if = "String::is_empty")]
    order_image_order_id: String,
}

include!("qolip_parts/part_01.rs");
include!("qolip_parts/part_02.rs");
