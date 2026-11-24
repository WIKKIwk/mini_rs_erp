//! Preparation list/detail boundary. List rows are presentation data; order
//! actions continue to load and validate their existing canonical authority.
use super::*;
use crate::core::apparatus_standard::{QueueDiscipline, RuntimeApparatusConfiguration};
use crate::core::production_map::{
    OrderControlState, ProductionMapDefinition, ProductionMapLiveSnapshot, ProductionMapService,
};
use crate::http::handlers::admin::production_maps::order_page;
use crate::http::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::time::{Duration, timeout};

const PROJECTION: &str = "preparation_order_list_v1";
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_LIVE_PREFIX: usize = 10_000;

#[derive(Clone, Default, Deserialize)]
pub struct OrderListQuery {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    all_apparatus: bool,
    #[serde(default)]
    search: String,
    #[serde(default)]
    cursor: String,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    epoch: String,
    #[serde(default)]
    rev: Option<u64>,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    if_version: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    length: Option<usize>,
    #[serde(default)]
    refresh: bool,
}

fn default_limit() -> usize {
    30
}

impl OrderListQuery {
    fn normalized(mut self) -> Result<Self, ApiError> {
        self.apparatus = self.apparatus.trim().to_string();
        self.search = self.search.trim().to_lowercase();
        if !(1..=50).contains(&self.limit)
            || self.search.chars().count() > 120
            || self.cursor.len() > 256
            || self.scope.len() > 256
            || self.epoch.len() > 256
            || self.if_version.len() > 128
            || self.version.len() > 128
            || self
                .length
                .is_some_and(|length| !(1..=MAX_LIVE_PREFIX).contains(&length))
            || (self.all_apparatus && !self.apparatus.is_empty())
            || (!self.apparatus.is_empty()
                && !crate::core::apparatus_standard::ApparatusId::is_valid(&self.apparatus))
        {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "preparation_orders_invalid",
            ));
        }
        Ok(self)
    }
}

fn api_error(status: StatusCode, code: &str) -> ApiError {
    (status, Json(json!({"ok":false,"error":code,"code":code})))
}

fn digest(value: impl Serialize) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&value).expect("order list serialization"))
    )
}

fn responsibilities(value: &Value) -> Vec<(String, String)> {
    let mut values = value
        .get("materials")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|v| {
            (
                v["material_id"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_lowercase(),
                v["material_name"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_lowercase(),
            )
        })
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

async fn authorize_orders(state: &AppState, headers: &HeaderMap) -> Result<Principal, ApiError> {
    let principal = authorize(state, headers).await?;
    if !state
        .admin
        .principal_is_active(&principal)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "preparation_orders_scope",
            )
        })?
    {
        return Err(api_error(StatusCode::FORBIDDEN, "forbidden"));
    }
    Ok(principal)
}

struct ReadScope {
    key: String,
    apparatus_id: String,
    apparatus: Value,
    selected: Option<RuntimeApparatusConfiguration>,
    responsibilities: Vec<(String, String)>,
    all_apparatus: bool,
}

async fn read_scope(
    state: &AppState,
    principal: &Principal,
    query: &OrderListQuery,
) -> Result<ReadScope, ApiError> {
    if !state
        .admin
        .principal_is_active(principal)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "preparation_orders_scope",
            )
        })?
        || !state
            .admin
            .principal_has_capability(principal, Capability::PreparationAccess)
            .await
    {
        return Err(api_error(StatusCode::FORBIDDEN, "forbidden"));
    }
    let assignments = store(state)?
        .list_responsibilities(&principal.ref_)
        .await
        .map_err(error)?;
    let assignments = responsibilities(&assignments);
    let mut configurations = if query.all_apparatus {
        Vec::new()
    } else {
        state
            .apparatus
            .list_runtime_configurations()
            .await
            .map_err(|_| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "preparation_orders_apparatus",
                )
            })?
    };
    configurations.retain(|item| item.is_active());
    let selected = configurations
        .iter()
        .find(|item| item.runtime.apparatus_id.as_str() == query.apparatus)
        .or_else(|| configurations.first())
        .cloned();
    let apparatus_id = selected
        .as_ref()
        .map(|c| c.runtime.apparatus_id.to_string())
        .unwrap_or_default();
    let apparatus = json!(
        configurations
            .iter()
            .map(|c| json!({
                "id":c.runtime.apparatus_id,"name":c.runtime.display.display_name,
            }))
            .collect::<Vec<_>>()
    );
    let picker_scope = configurations
        .iter()
        .map(|c| {
            (
                c.runtime.apparatus_id.as_str(),
                c.runtime.source_revision,
                c.runtime.display.display_name.as_str(),
                c.queue.discipline,
            )
        })
        .collect::<Vec<_>>();
    let key = if query.all_apparatus {
        digest((
            PROJECTION,
            "all_apparatus",
            &principal.role,
            &principal.ref_,
            &query.search,
            query.limit,
            &assignments,
        ))
    } else {
        digest((
            PROJECTION,
            &principal.role,
            &principal.ref_,
            &apparatus_id,
            &query.search,
            query.limit,
            &assignments,
            &picker_scope,
        ))
    };
    Ok(ReadScope {
        key,
        apparatus_id,
        apparatus,
        selected,
        responsibilities: assignments,
        all_apparatus: query.all_apparatus,
    })
}

#[derive(Clone, PartialEq, Eq, Serialize)]
struct OrderRow {
    id: String,
    code: String,
    order_number: String,
    title: String,
    customer_name: String,
    image_id: String,
    position: usize,
}

struct CompactRows {
    rows: Vec<OrderRow>,
    version: String,
    metadata_stamp: String,
    built_at: Instant,
}

impl CompactRows {
    fn reusable(&self, metadata_stamp: &str, refresh: bool, requested_at: Instant) -> bool {
        self.metadata_stamp == metadata_stamp && (!refresh || self.built_at >= requested_at)
    }
}

type RowCacheEntry = Arc<Mutex<Option<CompactRows>>>;
type RowCache = Mutex<BTreeMap<String, (Instant, RowCacheEntry)>>;
static ROW_CACHE: OnceLock<RowCache> = OnceLock::new();

async fn row_cache_entry(key: String) -> RowCacheEntry {
    let mut entries = ROW_CACHE.get_or_init(Default::default).lock().await;
    if let Some((touched, entry)) = entries.get_mut(&key) {
        *touched = Instant::now();
        return entry.clone();
    }
    if entries.len() >= 32 {
        let oldest = entries
            .iter()
            .filter(|(_, (_, entry))| Arc::strong_count(entry) == 1)
            .min_by_key(|(_, (touched, _))| *touched)
            .map(|(key, _)| key.clone());
        if let Some(key) = oldest {
            entries.remove(&key);
        }
    }
    let entry = Arc::new(Mutex::new(None));
    if entries.len() < 32 {
        entries.insert(key, (Instant::now(), entry.clone()));
    }
    entry
}

fn row(map: &ProductionMapDefinition, position: usize) -> OrderRow {
    let code = if !map.code.trim().is_empty() {
        map.code.trim()
    } else if !map.order_number.trim().is_empty() {
        map.order_number.trim()
    } else {
        map.id.strip_prefix("zakaz-").unwrap_or(&map.id)
    };
    let product = map
        .nodes
        .iter()
        .filter(|n| n.kind == crate::core::production_map::ProductionMapNodeKind::End)
        .map(|n| n.title.trim())
        .find(|v| !v.is_empty() && *v != map.title.trim())
        .unwrap_or("");
    let title = if map.title.trim().is_empty() {
        product
    } else {
        map.title.trim()
    };
    OrderRow {
        id: map.id.clone(),
        code: code.to_string(),
        order_number: map.order_number.clone(),
        title: title.to_string(),
        customer_name: map.customer_name.trim().to_string(),
        image_id: map.image_id.clone(),
        position,
    }
}

fn offset(cursor: &str, version: &str) -> Result<usize, ApiError> {
    if cursor.is_empty() {
        return Ok(0);
    }
    let parsed = URL_SAFE_NO_PAD
        .decode(cursor)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<(String, usize)>(&bytes).ok())
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "preparation_orders_cursor"))?;
    if parsed.0 != version {
        return Err(api_error(
            StatusCode::CONFLICT,
            "preparation_orders_changed",
        ));
    }
    Ok(parsed.1)
}

include!("orders_parts/read_list.rs");

include!("orders_parts/socket_loop.rs");

#[cfg(test)]
#[path = "orders_tests.rs"]
mod tests;
