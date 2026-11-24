//! Shared, bounded live projections of the exact HTTP admin windows.
//! Domain ordering, visibility and action permissions stay in `order_page`.
use super::*;
use crate::http::json_research;
use crate::http::ws::{Message, WebSocket};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const MAX_FEEDS: usize = 64;
const MAX_HISTORY: usize = 8;
const HISTORY_BYTES: usize = 512 * 1024;
const HISTORY_TTL: Duration = Duration::from_secs(90);
const MAX_DELTA_BYTES: usize = 256 * 1024;
const MAX_WINDOW_BYTES: usize = 8 * 1024 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const RETRY_READ_DELAY: Duration = Duration::from_millis(100);

async fn read_before_deadline<T>(
    deadline: tokio::time::Instant,
    read: impl std::future::Future<Output = Result<T, AdminError>>,
) -> Result<T, AdminError> {
    tokio::time::timeout_at(deadline, read)
        .await
        .map_err(|_| conflict("order_page_changed"))?
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct WindowRequest {
    id: String,
    view: String,
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    search: String,
    length: usize,
    #[serde(default)]
    version: String,
    #[serde(default)]
    projection: String,
}

pub(super) fn requests(raw: &str) -> Result<Vec<WindowRequest>, AdminError> {
    if raw.len() > 4096 {
        return Err(bad_request("invalid_page_live_windows"));
    }
    let mut windows: Vec<WindowRequest> =
        serde_json::from_str(raw).map_err(|_| bad_request("invalid_page_live_windows"))?;
    let mut ids = std::collections::BTreeSet::new();
    if windows.is_empty() || windows.len() > 2 {
        return Err(bad_request("invalid_page_live_windows"));
    }
    for window in &mut windows {
        window.search = window.search.trim().to_lowercase();
        if window.id.is_empty()
            || window.id.len() > 256
            || !ids.insert(window.id.clone())
            || window.length > 10_000
            || window.search.chars().count() > 120
            || window.version.len() > 128
            || !matches!(
                window.view.as_str(),
                "sequence" | "orders" | "move" | "closed" | "audit"
            )
            || window.apparatus.len() > 160
            || window.target.len() > 160
            || !super::order_page::valid_list_projection(&window.view, &window.projection)
        {
            return Err(bad_request("invalid_page_live_windows"));
        }
    }
    Ok(windows)
}

impl WindowRequest {
    fn key(&self, principal: &Principal) -> String {
        // Session tokens and client-assigned IDs are never cache keys. The
        // principal boundary also isolates training projections.
        let mut key = json!([
            principal.role,
            principal.ref_,
            self.view,
            self.apparatus,
            self.target,
            self.search,
            self.length
        ]);
        if !self.projection.is_empty() {
            key.as_array_mut().unwrap().push(self.projection.clone().into());
        }
        key.to_string()
    }

    fn query(&self, cursor: String) -> super::order_page::OrderPageQuery {
        super::order_page::OrderPageQuery {
            view: self.view.clone(),
            apparatus: self.apparatus.clone(),
            target: self.target.clone(),
            search: self.search.clone(),
            cursor,
            projection: self.projection.clone(),
            limit: Some(20),
            ..Default::default()
        }
    }
}

struct Window {
    pages: Vec<Value>,
    version: String,
    epoch: String,
    revision: u64,
    bytes: usize,
    created: Instant,
    // A broadcast prepares one wire frame per baseline/client window ID.
    // Cache capacity is bounded even when clients choose arbitrary IDs.
    prepared: Mutex<BTreeMap<(String, String), Arc<str>>>,
    prepared_bytes: AtomicUsize,
}

impl Window {
    fn retained_bytes(&self) -> usize {
        self.bytes + self.prepared_bytes.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
struct Feed {
    current: Option<Arc<Window>>,
    history: VecDeque<Arc<Window>>,
}

#[derive(Default)]
pub(crate) struct PageLiveHub {
    feeds: Mutex<BTreeMap<String, Arc<Mutex<Feed>>>>,
    notifications: Mutex<Option<(u64, Value, Arc<str>)>>,
}

impl PageLiveHub {
    async fn notifications(&self, state: &AppState) -> Result<Arc<str>, AdminError> {
        let mut cached = self.notifications.lock().await;
        let deadline = tokio::time::Instant::now() + READ_TIMEOUT;
        for _ in 0..3 {
            let revision = state.production_maps.snapshot_revision();
            if let Some((observed, _, frame)) = cached.as_ref() {
                if *observed == revision {
                    return Ok(frame.clone());
                }
            }
            let requests = read_before_deadline(deadline, async {
                state
                    .production_maps
                    .completion_requests(200)
                    .await
                    .map_err(production_map_error)
            })
            .await?;
            if revision != state.production_maps.snapshot_revision() {
                continue;
            }
            let data = json!(requests);
            if let Some((observed, previous, frame)) = cached.as_mut() {
                if *previous == data {
                    *observed = revision;
                    return Ok(frame.clone());
                }
            }
            let frame: Arc<str> = json_research::value_to_string(&json!({"ok":true,"type":"notifications_snapshot",
                "epoch":state.production_maps.snapshot_epoch(),"rev":revision,
                "completion_requests":data}))
            .into();
            *cached = Some((revision, data, frame.clone()));
            return Ok(frame);
        }
        Err(conflict("order_page_changed"))
    }

    async fn subscribe(&self, key: String) -> Arc<Mutex<Feed>> {
        let mut feeds = self.feeds.lock().await;
        if let Some(feed) = feeds.get(&key) {
            return feed.clone();
        }
        if feeds.len() >= MAX_FEEDS {
            let idle = feeds
                .iter()
                .find(|(_, feed)| Arc::strong_count(feed) == 1)
                .map(|(key, _)| key.clone());
            if let Some(key) = idle {
                feeds.remove(&key);
            }
        }
        let feed = Arc::new(Mutex::new(Feed::default()));
        if feeds.len() < MAX_FEEDS {
            feeds.insert(key, feed.clone());
        }
        feed
    }
}

impl Feed {
    async fn current(
        &mut self,
        state: &AppState,
        principal: &Principal,
        request: &WindowRequest,
    ) -> Result<Arc<Window>, AdminError> {
        let revision = state.production_maps.snapshot_revision();
        if let Some(current) = &self.current {
            if current.revision == revision {
                return Ok(current.clone());
            }
        }
        // Domain projections can retry internally while writes continue. The
        // outer revision retry bound alone cannot bound that nested wait.
        let next = Arc::new(
            read_before_deadline(
                tokio::time::Instant::now() + READ_TIMEOUT,
                read_window(state, principal, request),
            )
            .await?,
        );
        if let Some(previous) = self.current.replace(next.clone()) {
            self.history.push_back(previous);
        }
        while self.history.len() > MAX_HISTORY
            || self
                .history
                .iter()
                .map(|item| item.retained_bytes())
                .sum::<usize>()
                > HISTORY_BYTES
            || self
                .history
                .front()
                .is_some_and(|item| item.created.elapsed() > HISTORY_TTL)
        {
            self.history.pop_front();
        }
        Ok(next)
    }

    fn version(&self, version: &str) -> Option<Arc<Window>> {
        self.current
            .iter()
            .chain(self.history.iter())
            .find(|window| window.version == version && window.created.elapsed() <= HISTORY_TTL)
            .cloned()
    }
}

async fn read_window(
    state: &AppState,
    principal: &Principal,
    request: &WindowRequest,
) -> Result<Window, AdminError> {
    // A write during pagination must retry the whole staged window. No mixed
    // revisions reach clients, including closed/pending/audit views.
    for attempt in 0..3 {
        let mut pages = Vec::new();
        let mut cursor = String::new();
        let mut bytes = 0;
        loop {
            let page =
                match super::order_page::read_page(state, principal, request.query(cursor)).await {
                    Ok(Some(page)) => page,
                    Ok(None) => return Err(conflict("order_page_changed")),
                    Err(error) if error.0 == StatusCode::CONFLICT && attempt < 2 => break,
                    Err(error) => return Err(error),
                };
            if pages
                .first()
                .is_some_and(|first: &Value| first["version"] != page["version"])
            {
                break;
            }
            bytes += json_research::to_vec(&page)
                .map_err(|_| bad_request("invalid_page_live_state"))?
                .len();
            if bytes > MAX_WINDOW_BYTES {
                return Err(bad_request("page_live_window_too_large"));
            }
            let next = page["next_cursor"].as_str().map(str::to_owned);
            pages.push(page);
            if next.is_none() || pages.len() * 20 >= request.length.max(20) {
                let first = &pages[0];
                let revision = first["rev"]
                    .as_u64()
                    .ok_or_else(|| conflict("order_page_changed"))?;
                if revision != state.production_maps.snapshot_revision() {
                    break;
                }
                return Ok(Window {
                    version: first["version"].as_str().unwrap_or_default().into(),
                    epoch: first["epoch"].as_str().unwrap_or_default().into(),
                    revision,
                    pages,
                    bytes,
                    created: Instant::now(),
                    prepared: Mutex::new(BTreeMap::new()),
                    prepared_bytes: AtomicUsize::new(0),
                });
            }
            cursor = next.unwrap();
        }
    }
    Err(conflict("order_page_changed"))
}

fn metadata(key: &str) -> bool {
    matches!(key, "version" | "epoch" | "rev" | "next_cursor")
}

fn content_equal(before: &Window, after: &Window) -> bool {
    before.pages.len() == after.pages.len()
        && before.pages.iter().zip(&after.pages).all(|(a, b)| {
            match (a.as_object(), b.as_object()) {
                (Some(a), Some(b)) => a
                    .iter()
                    .filter(|(key, _)| !metadata(key))
                    .eq(b.iter().filter(|(key, _)| !metadata(key))),
                _ => false,
            }
        })
}

/// Sparse operations retain explicit null values (unlike JSON merge-patch).
/// Different-length arrays are replaced atomically; equal-length arrays can
/// update a single row/property without retransmitting maps or programs.
fn diff(before: &Value, after: &Value, path: &mut Vec<Value>, ops: &mut Vec<Value>) {
    if before == after {
        return;
    }
    match (before, after) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().filter(|key| !b.contains_key(*key)) {
                path.push(json!(key));
                ops.push(json!({"op":"remove", "path":path}));
                path.pop();
            }
            for (key, value) in b {
                path.push(json!(key));
                if let Some(previous) = a.get(key) {
                    diff(previous, value, path, ops);
                } else {
                    ops.push(json!({"op":"set", "path":path, "value":value}));
                }
                path.pop();
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (index, (previous, value)) in a.iter().zip(b).enumerate() {
                path.push(json!(index));
                diff(previous, value, path, ops);
                path.pop();
            }
        }
        _ => ops.push(json!({"op":"set", "path":path, "value":after})),
    }
}

async fn prepared(before: &Window, after: &Window, id: &str) -> Arc<str> {
    let key = (before.version.clone(), id.to_string());
    let mut cached = after.prepared.lock().await;
    if let Some(frame) = cached.get(&key) {
        return frame.clone();
    }
    let mut payload = json!({
        "ok":true, "window":id, "epoch":after.epoch,
        "base_version":before.version, "version":after.version, "rev":after.revision,
    });
    if content_equal(before, after) {
        payload["type"] = "page_window_checkpoint".into();
        payload["next_cursors"] = json!(
            after
                .pages
                .iter()
                .map(|page| &page["next_cursor"])
                .collect::<Vec<_>>()
        );
    } else {
        payload["type"] = "page_window_delta".into();
        payload["page_count"] = after.pages.len().into();
        let changes = after
            .pages
            .iter()
            .enumerate()
            .filter_map(|(index, page)| {
                let mut ops = Vec::new();
                if let Some(previous) = before.pages.get(index) {
                    diff(previous, page, &mut Vec::new(), &mut ops);
                } else {
                    ops.push(json!({"op":"set", "path":[], "value":page}));
                }
                (!ops.is_empty()).then(|| json!({"index":index,"ops":ops}))
            })
            .collect::<Vec<_>>();
        payload["changes"] = json!(changes);
    }
    let mut frame = json_research::value_to_string(&payload);
    if frame.len() > MAX_DELTA_BYTES {
        frame = json_research::value_to_string(&json!({"ok":true,"type":"page_window_resync","window":id,
            "epoch":after.epoch,"rev":after.revision,"version":after.version}));
    }
    let frame: Arc<str> = frame.into();
    if cached.len() < 8
        && cached.values().map(|value| value.len()).sum::<usize>() + frame.len() <= MAX_DELTA_BYTES
    {
        cached.insert(key, frame.clone());
        after
            .prepared_bytes
            .fetch_add(frame.len(), Ordering::Relaxed);
    }
    frame
}

async fn send(socket: &mut WebSocket, frame: &str) -> bool {
    tokio::time::timeout(
        Duration::from_secs(15),
        socket.send(Message::Text(frame.to_owned().into())),
    )
    .await
    .is_ok_and(|result| result.is_ok())
}

struct Subscription {
    request: WindowRequest,
    feed: Arc<Mutex<Feed>>,
    baseline: Option<Arc<Window>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UpdateStatus {
    Complete,
    Pending,
    Closed,
}

include!("page_live_parts/sockets.rs");

#[cfg(test)]
#[path = "page_live_tests.rs"]
mod tests;
