//! Admin read windows. A cursor belongs to one query and one canonical revision.
//! These projections must never be used as the authority for queue mutations.
use super::*;
use crate::core::production_map::{
    ApparatusQueuePolicy, ProductionMapLiveSnapshot, ProductionMapSaved,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[path = "order_page_list.rs"]
mod list;
pub(super) const SEQUENCE_LIST_PROJECTION: &str = "sequence_list_v1";
pub(super) const ORDERS_LIST_PROJECTION: &str = "orders_list_v1";

pub(super) fn valid_list_projection(view: &str, projection: &str) -> bool {
    projection.is_empty()
        || matches!(
            (view, projection),
            ("sequence", SEQUENCE_LIST_PROJECTION) | ("orders", ORDERS_LIST_PROJECTION)
        )
}

pub(super) fn worker_list_fields(
    snapshot: &ProductionMapLiveSnapshot,
    customers: &BTreeMap<String, String>,
) -> serde_json::Value {
    list::project_worker(snapshot, customers)
}

pub(super) fn worker_pending_report_ids(
    snapshot: &ProductionMapLiveSnapshot,
    completed: &[crate::core::production_map::CompletedQueueOrder],
) -> BTreeMap<String, Vec<String>> {
    list::worker_pending_report_ids(snapshot, completed)
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct OrderPageQuery {
    #[serde(default)]
    pub(super) view: String,
    #[serde(default)]
    pub(super) apparatus: String,
    #[serde(default)]
    pub(super) target: String,
    #[serde(default)]
    pub(super) search: String,
    #[serde(default)]
    pub(super) cursor: String,
    #[serde(default)]
    pub(super) if_version: String,
    #[serde(default)]
    pub(super) projection: String,
    #[serde(default)]
    pub(super) compression: String,
    pub(super) limit: Option<usize>,
}

#[derive(serde::Serialize, Deserialize)]
struct Cursor {
    version: String,
    offset: usize,
}

fn version(scope: &str, epoch: &str, revision: u64, pending: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{scope}\0{epoch}\0{revision}\0{pending}"))
    )
}

fn offset(cursor: &str, version: &str) -> Result<usize, AdminError> {
    if cursor.is_empty() {
        return Ok(0);
    }
    if cursor.len() > 512 {
        return Err(bad_request("invalid_order_page_cursor"));
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(cursor)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Cursor>(&bytes).ok())
        .ok_or_else(|| bad_request("invalid_order_page_cursor"))?;
    if decoded.version != version {
        return Err(conflict("order_page_changed"));
    }
    Ok(decoded.offset)
}

fn next_cursor(version: &str, end: usize, total: usize) -> Option<String> {
    (end < total).then(|| {
        URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&Cursor {
                version: version.into(),
                offset: end,
            })
            .unwrap(),
        )
    })
}

fn not_modified() -> Response {
    (
        StatusCode::NOT_MODIFIED,
        [(header::CACHE_CONTROL, "no-store")],
    )
        .into_response()
}

pub async fn production_map_order_page(
    State(state): State<AppState>,
    Query(query): Query<OrderPageQuery>,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ProductionMapRead,
        ],
    )
    .await?;
    let identity = query.compression == "identity";
    let response = match read_page(&state, &principal, query).await? {
        Some(payload) => json_response(payload),
        None => not_modified(),
    };
    Ok(if identity {
        crate::http::router::identity_response(response)
    } else {
        response
    })
}

include!("order_page_parts/read_page.rs");

include!("order_page_parts/read_detail.rs");
