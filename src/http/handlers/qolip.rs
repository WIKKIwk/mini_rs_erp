use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};

use crate::app::AppState;
use crate::core::authz::Capability;
use crate::core::gscale::ProgressLabelPrintRequest;
use crate::core::qolip::{
    QolipBlock, QolipCellQrInput, QolipCheckoutCreate, QolipCheckoutReturn, QolipError,
    QolipLocationMove, QolipLocationMoveBatch, QolipLocationUpsert, QolipProductSpecBatchUpsert,
    QolipProductSpecDelete, QolipProductSpecUpsert,
};
use crate::core::warehouses::{WarehouseDeleteRequest, WarehouseUpsert};

mod support;

use self::support::*;
pub use self::support::{
    QolipBlockUpdate, QolipBlockUpsert, QolipCellQrLookupQuery, QolipCellQrPrintRequest,
    QolipCheckoutsQuery, QolipCodeQrPrintRequest, QolipErrorResponse, QolipSearchQuery,
};

include!("qolip_parts/part_01.rs");
include!("qolip_parts/part_02.rs");
