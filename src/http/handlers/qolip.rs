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

pub use order_products::order_products;

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
