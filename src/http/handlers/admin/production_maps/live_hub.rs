use super::live_delta::{LiveView, snapshot_patch_for_protocol};
use super::live_feed::{LiveHistory, PreparedDelta};
use crate::core::production_map::ProductionMapLiveSnapshot;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

const MAX_FEEDS: usize = 128;
const MAX_SHARED_PROJECTIONS: usize = 64;
const MAX_SHARED_PATCHES: usize = 32;
const MAX_CACHED_PATCH_BYTES: usize = 64 * 1024;
const IDLE_RETENTION: Duration = Duration::from_secs(90);

#[derive(Default)]
pub(crate) struct StateLiveHub {
    feeds: Mutex<BTreeMap<String, Arc<StateFeed>>>,
    projections: Mutex<VecDeque<ProjectionEntry>>,
    patches: Mutex<VecDeque<PatchEntry>>,
}

#[derive(Default)]
pub(super) struct StateFeed {
    pub data: tokio::sync::Mutex<StateFeedData>,
}

pub(super) struct StateFeedData {
    pub view: Option<LiveView>,
    pub history: LiveHistory,
    pub used: Instant,
    pub compact: bool,
}

impl Default for StateFeedData {
    fn default() -> Self {
        Self {
            view: None,
            history: LiveHistory::default(),
            used: Instant::now(),
            compact: false,
        }
    }
}

struct ProjectionEntry {
    source: Weak<ProductionMapLiveSnapshot>,
    key: String,
    projected: Arc<ProductionMapLiveSnapshot>,
}

struct PatchEntry {
    before: Weak<ProductionMapLiveSnapshot>,
    after: Weak<ProductionMapLiveSnapshot>,
    patch: Arc<str>,
    compact: bool,
}

impl StateLiveHub {
    /// Actor identity is mandatory: completed history, decisions and training
    /// overlays are personal even when two workers operate the same apparatus.
    #[cfg(test)]
    pub(super) fn feed(&self, key: String) -> Arc<StateFeed> {
        self.feed_for_protocol(key, false)
    }

    pub(super) fn feed_for_protocol(&self, key: String, compact: bool) -> Arc<StateFeed> {
        // Replay history is a protocol contract as well as an actor scope.
        let key = format!("{}:{key}", u8::from(compact));
        let mut feeds = self.feeds.lock().expect("live feed registry poisoned");
        if let Some(feed) = feeds.get(&key) {
            return feed.clone();
        }
        feeds.retain(|_, feed| {
            Arc::strong_count(feed) > 1
                || feed
                    .data
                    .try_lock()
                    .map_or(true, |data| data.used.elapsed() <= IDLE_RETENTION)
        });
        if feeds.len() >= MAX_FEEDS {
            if let Some(key) = feeds
                .iter()
                .filter(|(_, feed)| Arc::strong_count(feed) == 1)
                .filter_map(|(key, feed)| {
                    feed.data
                        .try_lock()
                        .ok()
                        .map(|data| (key.clone(), data.used))
                })
                .min_by_key(|(_, used)| *used)
                .map(|(key, _)| key)
            {
                feeds.remove(&key);
            }
        }
        let feed = Arc::new(StateFeed {
            data: tokio::sync::Mutex::new(StateFeedData {
                compact,
                ..StateFeedData::default()
            }),
        });
        // Uncached overflow remains correct and bounded per live connection.
        if feeds.len() < MAX_FEEDS {
            feeds.insert(key, feed.clone());
        }
        feed
    }

    /// Share the list projection across different actors only when the source
    /// snapshot, authorized apparatuses, history membership and window match.
    /// Training overlays use a distinct source Arc and cannot cross this key.
    pub(super) fn project(
        &self,
        source: &Arc<ProductionMapLiveSnapshot>,
        key: String,
        build: impl FnOnce() -> ProductionMapLiveSnapshot,
    ) -> Arc<ProductionMapLiveSnapshot> {
        let mut entries = self.projections.lock().expect("projection cache poisoned");
        if let Some(entry) = entries.iter().find(|entry| {
            entry.key == key
                && entry
                    .source
                    .upgrade()
                    .is_some_and(|previous| Arc::ptr_eq(&previous, source))
        }) {
            return entry.projected.clone();
        }
        let projected = Arc::new(build());
        entries.retain(|entry| entry.source.strong_count() > 0);
        if entries.len() >= MAX_SHARED_PROJECTIONS {
            entries.pop_front();
        }
        entries.push_back(ProjectionEntry {
            source: Arc::downgrade(source),
            key,
            projected: projected.clone(),
        });
        projected
    }

    /// The expensive graph/list comparison and serialization is performed once
    /// for an identical before/after list pair, including across actor feeds.
    #[cfg(test)]
    pub(super) fn snapshot_patch(
        &self,
        before: &Arc<ProductionMapLiveSnapshot>,
        after: &Arc<ProductionMapLiveSnapshot>,
    ) -> Arc<str> {
        self.snapshot_patch_for_protocol(before, after, false)
    }

    pub(super) fn snapshot_patch_for_protocol(
        &self,
        before: &Arc<ProductionMapLiveSnapshot>,
        after: &Arc<ProductionMapLiveSnapshot>,
        compact: bool,
    ) -> Arc<str> {
        let mut entries = self.patches.lock().expect("patch cache poisoned");
        if let Some(entry) = entries.iter().find(|entry| {
            entry.compact == compact
                && entry
                    .before
                    .upgrade()
                    .is_some_and(|value| Arc::ptr_eq(&value, before))
                && entry
                    .after
                    .upgrade()
                    .is_some_and(|value| Arc::ptr_eq(&value, after))
        }) {
            return entry.patch.clone();
        }
        let patch: Arc<str> =
            serde_json::to_string(&snapshot_patch_for_protocol(before, after, compact))
                .expect("snapshot patch serialization")
                .into();
        if patch.len() > MAX_CACHED_PATCH_BYTES {
            return patch;
        }
        entries.retain(|entry| entry.before.strong_count() > 0 && entry.after.strong_count() > 0);
        if entries.len() >= MAX_SHARED_PATCHES {
            entries.pop_front();
        }
        entries.push_back(PatchEntry {
            before: Arc::downgrade(before),
            after: Arc::downgrade(after),
            patch: patch.clone(),
            compact,
        });
        patch
    }
}

pub(super) fn join_patch(common: &Arc<str>, personal: &Map<String, Value>) -> Arc<str> {
    if personal.is_empty() {
        return common.clone();
    }
    let personal = serde_json::to_string(personal).expect("personal patch serialization");
    if common.as_ref() == "{}" {
        return personal.into();
    }
    // Both fragments originate from serde_json, never from a client string.
    format!("{},{}", &common[..common.len() - 1], &personal[1..]).into()
}

impl StateFeedData {
    pub(super) fn advance_unchanged(&mut self, revision: u64) {
        let view = self.view.as_mut().expect("initialized view");
        self.history.push(PreparedDelta::for_protocol(
            &view.epoch,
            &view.scope,
            view.revision,
            revision,
            Arc::from("{}"),
            self.compact,
        ));
        view.revision = revision;
        self.used = Instant::now();
    }

    pub(super) fn replace(&mut self, next: LiveView, patch: Option<Arc<str>>) {
        self.used = Instant::now();
        if let Some(previous) = &self.view {
            if previous.epoch != next.epoch || previous.scope != next.scope || patch.is_none() {
                self.history.clear();
            } else if previous.revision != next.revision {
                self.history.push(PreparedDelta::for_protocol(
                    &next.epoch,
                    &next.scope,
                    previous.revision,
                    next.revision,
                    patch.expect("checked patch"),
                    self.compact,
                ));
            }
        }
        self.view = Some(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn different_actors_never_share_private_feeds() {
        let hub = StateLiveHub::default();
        let a = hub.feed("epoch/worker-a/scope".into());
        let repeated = hub.feed("epoch/worker-a/scope".into());
        let b = hub.feed("epoch/worker-b/scope".into());
        assert!(Arc::ptr_eq(&a, &repeated));
        assert!(!Arc::ptr_eq(&a, &b));
    }

    #[tokio::test]
    async fn wire_versions_never_share_replay_or_prepared_patch_caches() {
        let hub = StateLiveHub::default();
        let legacy = hub.feed_for_protocol("same-actor/same-scope".into(), false);
        let compact = hub.feed_for_protocol("same-actor/same-scope".into(), true);
        assert!(!Arc::ptr_eq(&legacy, &compact));
        assert!(!legacy.data.lock().await.compact);
        assert!(compact.data.lock().await.compact);
        let mut before = ProductionMapLiveSnapshot::default();
        before.sequences.insert(
            "press".into(),
            (0..1_000).map(|i| format!("order-{i:06}")).collect(),
        );
        let mut after = before.clone();
        after.sequences.get_mut("press").unwrap().rotate_right(1);
        let (before, after) = (Arc::new(before), Arc::new(after));
        let v1 = hub.snapshot_patch_for_protocol(&before, &after, false);
        let v2 = hub.snapshot_patch_for_protocol(&before, &after, true);
        assert!(!Arc::ptr_eq(&v1, &v2));
        assert!(Arc::ptr_eq(
            &v2,
            &hub.snapshot_patch_for_protocol(&before, &after, true)
        ));
        let v1: Value = serde_json::from_str(&v1).unwrap();
        let v2: Value = serde_json::from_str(&v2).unwrap();
        assert_eq!(
            v1["sequences"]["upsert"]["press"].as_array().unwrap().len(),
            1_000
        );
        assert!(v1["sequences"].get("edit").is_none());
        assert!(v2["sequences"]["upsert"].as_object().unwrap().is_empty());
        assert_eq!(v2["sequences"]["edit"]["press"]["len"], 1_000);
    }

    #[test]
    fn equivalent_authorized_lists_share_projection_but_other_windows_do_not() {
        let hub = StateLiveHub::default();
        let source = Arc::new(ProductionMapLiveSnapshot::default());
        let calls = AtomicUsize::new(0);
        let build = || {
            calls.fetch_add(1, Ordering::SeqCst);
            ProductionMapLiveSnapshot::default()
        };
        let a = hub.project(&source, "apparatus-a/10/history-a".into(), build);
        let b = hub.project(&source, "apparatus-a/10/history-a".into(), build);
        let private = hub.project(&source, "apparatus-a/10/history-b".into(), build);
        let other = hub.project(&source, "apparatus-b/10/history-a".into(), build);
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!Arc::ptr_eq(&a, &private));
        assert!(!Arc::ptr_eq(&a, &other));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn concurrent_workers_build_the_same_authorized_projection_once() {
        let hub = StateLiveHub::default();
        let source = Arc::new(ProductionMapLiveSnapshot::default());
        let calls = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let tasks = (0..32)
                .map(|_| {
                    scope.spawn(|| {
                        hub.project(&source, "same-list".into(), || {
                            calls.fetch_add(1, Ordering::SeqCst);
                            ProductionMapLiveSnapshot::default()
                        })
                    })
                })
                .collect::<Vec<_>>();
            let projected = tasks
                .into_iter()
                .map(|task| task.join().unwrap())
                .collect::<Vec<_>>();
            assert!(
                projected
                    .iter()
                    .all(|value| Arc::ptr_eq(value, &projected[0]))
            );
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn inactive_feed_cache_is_bounded_and_does_not_evict_active_readers() {
        let hub = StateLiveHub::default();
        let active = hub.feed("active".into());
        for index in 0..MAX_FEEDS * 3 {
            hub.feed(format!("inactive-{index}"));
        }
        assert!(hub.feeds.lock().unwrap().len() <= MAX_FEEDS);
        assert!(Arc::ptr_eq(&active, &hub.feed("active".into())));
    }

    #[test]
    fn common_patch_encoding_is_shared_without_personal_data() {
        let hub = StateLiveHub::default();
        let before = Arc::new(ProductionMapLiveSnapshot::default());
        let mut after = ProductionMapLiveSnapshot::default();
        after.sequences.insert("a".into(), vec!["order".into()]);
        let after = Arc::new(after);
        let a = hub.snapshot_patch(&before, &after);
        let b = hub.snapshot_patch(&before, &after);
        assert!(Arc::ptr_eq(&a, &b));
        let personal =
            Map::from_iter([("completed_orders".into(), serde_json::json!(["private"]))]);
        let joined: Value = serde_json::from_str(&join_patch(&a, &personal)).unwrap();
        assert_eq!(joined["completed_orders"], serde_json::json!(["private"]));
        assert!(
            serde_json::from_str::<Value>(&a)
                .unwrap()
                .get("completed_orders")
                .is_none()
        );
    }
}
