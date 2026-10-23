//! Shared, bounded live projections of the exact HTTP admin windows.
//! Domain ordering, visibility and action permissions stay in `order_page`.
use super::*;
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
            let frame: Arc<str> = json!({"ok":true,"type":"notifications_snapshot",
                "epoch":state.production_maps.snapshot_epoch(),"rev":revision,
                "completion_requests":data})
            .to_string()
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
            bytes += serde_json::to_vec(&page)
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
    let mut frame = payload.to_string();
    if frame.len() > MAX_DELTA_BYTES {
        frame = json!({"ok":true,"type":"page_window_resync","window":id,
            "epoch":after.epoch,"rev":after.revision,"version":after.version})
        .to_string();
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

async fn update(
    state: &AppState,
    principal: &Principal,
    socket: &mut WebSocket,
    subscriptions: &mut [Subscription],
) -> UpdateStatus {
    let mut pending = false;
    for sub in subscriptions {
        let mut feed = sub.feed.lock().await;
        let current = match feed.current(state, principal, &sub.request).await {
            Ok(current) => current,
            Err(error)
                if error.0 == StatusCode::CONFLICT && error.1.0.error == "order_page_changed" =>
            {
                // A concurrent write invalidated staging. Keep the last sent
                // baseline and retry the whole window after writes settle.
                drop(feed);
                pending = true;
                continue;
            }
            Err(error) => {
                drop(feed);
                let _ = send(socket, &json!(error.1.0).to_string()).await;
                return UpdateStatus::Closed;
            }
        };
        let baseline = sub
            .baseline
            .clone()
            .or_else(|| feed.version(&sub.request.version));
        drop(feed);
        let initial = sub.baseline.is_none();
        if let Some(before) = baseline {
            if before.version != current.version {
                if !send(socket, &prepared(&before, &current, &sub.request.id).await).await {
                    return UpdateStatus::Closed;
                }
            }
            if initial
                && !send(
                    socket,
                    &json!({"ok":true,"type":"page_window_ready",
                "window":sub.request.id,"epoch":current.epoch,"rev":current.revision,
                "version":current.version,"resync":false})
                    .to_string(),
                )
                .await
            {
                return UpdateStatus::Closed;
            }
        } else {
            if !send(
                socket,
                &json!({"ok":true,"type":"page_window_ready",
                "window":sub.request.id,"epoch":current.epoch,"rev":current.revision,
                "version":current.version,"resync":sub.request.version != current.version})
                .to_string(),
            )
            .await
            {
                return UpdateStatus::Closed;
            }
        }
        sub.baseline = Some(current);
    }
    if pending {
        UpdateStatus::Pending
    } else {
        UpdateStatus::Complete
    }
}

pub(super) async fn socket(
    state: AppState,
    mut socket: WebSocket,
    principal: Principal,
    requests: Vec<WindowRequest>,
) {
    let mut events = state.production_maps.subscribe_live();
    let mut subscriptions = Vec::new();
    for request in requests {
        let feed = state
            .production_page_live
            .subscribe(request.key(&principal))
            .await;
        subscriptions.push(Subscription {
            request,
            feed,
            baseline: None,
        });
    }
    let status = update(&state, &principal, &mut socket, &mut subscriptions).await;
    if status == UpdateStatus::Closed {
        return;
    }
    let mut retry_pending = status == UpdateStatus::Pending;
    let mut retry = tokio::time::interval(RETRY_READ_DELAY);
    retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    retry.reset();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(25));
    heartbeat.tick().await;
    loop {
        tokio::select! {
            message = socket.recv() => match message {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => {}
            },
            event = events.recv() => {
                if matches!(event, Err(tokio::sync::broadcast::error::RecvError::Closed)) { break; }
                // Pending reads retry on the timer, even if this was the last
                // notification. Draining events here avoids a conflict loop.
                if retry_pending { continue; }
                // Reads after lag use a fresh canonical view and diff from the
                // subscriber's retained baseline, so no partial chain is applied.
                let status = update(&state, &principal, &mut socket, &mut subscriptions).await;
                if status == UpdateStatus::Closed { break; }
                retry_pending = status == UpdateStatus::Pending;
                if retry_pending { retry.reset(); }
            },
            _ = retry.tick(), if retry_pending => {
                let status = update(&state, &principal, &mut socket, &mut subscriptions).await;
                if status == UpdateStatus::Closed { break; }
                retry_pending = status == UpdateStatus::Pending;
                if retry_pending { retry.reset(); }
            },
            _ = heartbeat.tick() => {
                if super::completion::require_paged_live_capability(&state, &principal).await.is_err() {
                    let _ = send(&mut socket, &json!({"ok":false,"error":"forbidden"}).to_string()).await;
                    break;
                }
            }
        }
    }
}

pub(super) async fn notifications_socket(
    state: AppState,
    mut socket: WebSocket,
    principal: Principal,
    include_requests: bool,
) {
    let mut events = state.production_maps.subscribe_live();
    let mut previous: Option<Arc<str>> = None;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(25));
    heartbeat.tick().await;
    loop {
        let frame = if include_requests {
            match state.production_page_live.notifications(&state).await {
                Ok(frame) => frame,
                Err(error) => {
                    let _ = send(&mut socket, &json!(error.1.0).to_string()).await;
                    break;
                }
            }
        } else {
            // Preserve legacy role filtering: only Admin receives requests.
            previous.clone().unwrap_or_else(|| {
                json!({"ok":true,
                "type":"notifications_snapshot","epoch":state.production_maps.snapshot_epoch(),
                "rev":state.production_maps.snapshot_revision(),"completion_requests":[]})
                .to_string()
                .into()
            })
        };
        if previous
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, &frame))
            && !send(&mut socket, &frame).await
        {
            break;
        }
        previous = Some(frame);
        tokio::select! {
            event = events.recv() => {
                if matches!(event, Err(tokio::sync::broadcast::error::RecvError::Closed)) { break; }
            },
            message = socket.recv() => match message {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => {}
            },
            _ = heartbeat.tick() => {
                if super::completion::require_any_live_capability(&state, &principal).await.is_err() {
                    let _ = send(&mut socket, &json!({"ok":false,"error":"forbidden"}).to_string()).await;
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(value: Value, version: &str) -> Window {
        Window {
            pages: vec![value],
            version: version.into(),
            epoch: "epoch".into(),
            revision: 1,
            bytes: 1,
            created: Instant::now(),
            prepared: Mutex::new(BTreeMap::new()),
            prepared_bytes: AtomicUsize::new(0),
        }
    }

    #[test]
    fn sparse_colour_patch_keeps_maps_and_explicit_nulls() {
        let before = json!({"maps":[{"id":"a","program":"x".repeat(10000)}],
            "queue_states":{"press":{"a":"pending"}},"optional":"old","removed":true});
        let after = json!({"maps":[{"id":"a","program":"x".repeat(10000)}],
            "queue_states":{"press":{"a":"paused"}},"optional":null});
        let mut ops = Vec::new();
        diff(&before, &after, &mut Vec::new(), &mut ops);
        assert_eq!(ops.len(), 3);
        assert!(ops.iter().all(|op| op["path"][0] != "maps"));
        assert!(
            ops.iter()
                .any(|op| op["op"] == "set" && op["value"].is_null())
        );
        assert!(ops.iter().any(|op| op["op"] == "remove"));
        assert!(json!(ops).to_string().len() < 512);
    }

    #[tokio::test]
    async fn checkpoint_advances_cursors_without_repeating_content() {
        let a = window(
            json!({"maps":[],"version":"old","rev":1,"epoch":"epoch","next_cursor":"a"}),
            "old",
        );
        let b = window(
            json!({"maps":[],"version":"new","rev":2,"epoch":"epoch","next_cursor":"b"}),
            "new",
        );
        let first = prepared(&a, &b, "1").await;
        let second = prepared(&a, &b, "1").await;
        assert!(Arc::ptr_eq(&first, &second));
        let value: Value = serde_json::from_str(&first).unwrap();
        assert_eq!(value["type"], "page_window_checkpoint");
        assert_eq!(value["next_cursors"], json!(["b"]));
        assert!(value.get("changes").is_none());
    }

    #[test]
    fn subscriptions_are_bounded_and_ids_unique() {
        assert!(requests("[]").is_err());
        assert!(requests(r#"[{"id":"x","view":"orders","length":10001}]"#).is_err());
        assert!(
            requests(
                r#"[{"id":"x","view":"orders","length":20},{"id":"x","view":"closed","length":20}]"#
            )
            .is_err()
        );
        assert!(
            requests(r#"[{"id":"1","view":"sequence","apparatus":"apparatus:a","length":40}]"#)
                .is_ok()
        );
    }

    #[tokio::test]
    async fn oversized_patches_require_resync_and_cached_variants_are_bounded() {
        let a = window(json!({"maps":[]}), "old");
        let b = window(
            json!({"maps":[{"program":"x".repeat(MAX_DELTA_BYTES)}]}),
            "new",
        );
        let frame = prepared(&a, &b, "1").await;
        let message: Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(message["type"], "page_window_resync");
        assert!(frame.len() < 1024);
        for id in 0..100 {
            prepared(&a, &b, &id.to_string()).await;
        }
        let variants = b.prepared.lock().await;
        assert!(variants.len() <= 8);
        assert!(variants.values().map(|value| value.len()).sum::<usize>() <= MAX_DELTA_BYTES);
        assert_eq!(
            b.retained_bytes(),
            b.bytes + variants.values().map(|value| value.len()).sum::<usize>()
        );
    }

    #[tokio::test]
    async fn active_subscription_registry_and_expired_recovery_are_bounded() {
        let hub = PageLiveHub::default();
        let mut active = Vec::new();
        for index in 0..MAX_FEEDS + 16 {
            active.push(hub.subscribe(index.to_string()).await);
        }
        assert_eq!(hub.feeds.lock().await.len(), MAX_FEEDS);
        assert!(Arc::ptr_eq(&active[0], &hub.subscribe("0".into()).await));
        let mut expired = window(json!({}), "expired");
        expired.created = Instant::now() - HISTORY_TTL * 2;
        let feed = Feed {
            current: Some(Arc::new(expired)),
            history: VecDeque::new(),
        };
        assert!(feed.version("expired").is_none());
        assert!(feed.version("unknown").is_none());
        assert!(feed.version("").is_none());
    }

    #[tokio::test]
    async fn stalled_shared_projection_releases_its_lock_for_safe_retry() {
        let lock = Mutex::new(());
        let guard = lock.lock().await;
        let error = read_before_deadline(
            tokio::time::Instant::now() + Duration::from_millis(5),
            async move {
                let _guard = guard;
                std::future::pending::<Result<(), AdminError>>().await
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.0, StatusCode::CONFLICT);
        assert_eq!(json!(error.1.0)["error"], "order_page_changed");
        assert!(lock.try_lock().is_ok());
    }
}
