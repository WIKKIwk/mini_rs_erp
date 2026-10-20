//! Small, bounded replay log shared by authorized live views.
//!
//! A cursor is accepted only when an observed transition starts at that exact
//! revision. Revisions skipped while building a view are never assumed to have
//! equivalent data. Prepared wire bytes are reused like Centrifuge's channel
//! publication cache; callers remain responsible for authorization and scope.
use axum::extract::ws::Utf8Bytes;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const HISTORY_ENTRIES: usize = 64;
const HISTORY_BYTES: usize = 256 * 1024;
const HISTORY_TTL: Duration = Duration::from_secs(90);
const PREPARED_VARIANTS: usize = 2;

pub(super) struct PreparedDelta {
    pub base_revision: u64,
    pub revision: u64,
    pub empty: bool,
    pub created: Instant,
    epoch_json: String,
    scope_json: String,
    patch: Arc<str>,
    prepared: Mutex<BTreeMap<u64, Utf8Bytes>>,
}

impl PreparedDelta {
    pub fn new(
        epoch: &str,
        scope: &str,
        base_revision: u64,
        revision: u64,
        patch: Arc<str>,
    ) -> Self {
        Self {
            base_revision,
            revision,
            empty: patch.as_ref() == "{}",
            created: Instant::now(),
            epoch_json: serde_json::to_string(epoch).expect("string serialization"),
            scope_json: serde_json::to_string(scope).expect("string serialization"),
            patch,
            prepared: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn bytes(&self) -> usize {
        // Bound the encoded-variant cache as part of retained history, too.
        (self.patch.len() + self.epoch_json.len() + self.scope_json.len() + 160)
            * (PREPARED_VARIANTS + 1)
    }

    pub fn frame(&self, base_revision: u64) -> Utf8Bytes {
        let mut prepared = self.prepared.lock().expect("publication cache poisoned");
        if let Some(frame) = prepared.get(&base_revision) {
            return frame.clone();
        }
        let frame: Utf8Bytes = format!(
            "{{\"ok\":true,\"type\":\"state_delta\",\"epoch\":{},\"scope\":{},\"base_rev\":{},\"rev\":{},\"patch\":{}}}",
            self.epoch_json, self.scope_json, base_revision, self.revision, self.patch,
        )
        .into();
        if prepared.len() < PREPARED_VARIANTS {
            prepared.insert(base_revision, frame.clone());
        }
        frame
    }
}

#[derive(Default)]
pub(super) struct LiveHistory {
    entries: VecDeque<Arc<PreparedDelta>>,
    bytes: usize,
}

impl LiveHistory {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    pub fn push(&mut self, delta: PreparedDelta) {
        self.bytes += delta.bytes();
        self.entries.push_back(Arc::new(delta));
        self.prune();
    }

    fn prune(&mut self) {
        while self.entries.len() > HISTORY_ENTRIES
            || self.bytes > HISTORY_BYTES
            || self
                .entries
                .front()
                .is_some_and(|entry| entry.created.elapsed() > HISTORY_TTL)
        {
            let Some(entry) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= entry.bytes();
        }
    }

    /// Returns the entire proven chain or no chain. Partial recovery would
    /// silently combine fields from different authoritative views.
    pub fn recover(&mut self, from: u64, to: u64) -> Option<Vec<Arc<PreparedDelta>>> {
        self.prune();
        if from == to {
            return Some(Vec::new());
        }
        if from > to {
            return None;
        }
        let mut cursor = from;
        let mut result = Vec::new();
        for entry in &self.entries {
            if entry.revision <= cursor {
                continue;
            }
            if entry.base_revision != cursor || entry.revision > to {
                return None;
            }
            cursor = entry.revision;
            result.push(entry.clone());
            if cursor == to {
                return Some(result);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn delta(base: u64, revision: u64, patch: &str) -> PreparedDelta {
        PreparedDelta::new("epoch", "scope", base, revision, Arc::from(patch))
    }

    #[test]
    fn replay_requires_exact_observed_cursor_and_complete_chain() {
        let mut history = LiveHistory::default();
        history.push(delta(1, 3, "{}"));
        history.push(delta(
            3,
            4,
            r#"{"order_controls":{"upsert":{},"remove":["a"]}}"#,
        ));
        assert_eq!(history.recover(1, 4).unwrap().len(), 2);
        assert!(history.recover(2, 4).is_none());
        assert!(history.recover(0, 4).is_none());
        assert!(history.recover(1, 5).is_none());
        assert!(history.recover(5, 4).is_none());
        assert!(history.recover(4, 4).unwrap().is_empty());
    }

    #[test]
    fn prepared_delta_can_skip_proven_empty_steps_without_changing_payload() {
        let delta = delta(3, 4, r#"{"order_statuses":{"remove":["a"],"upsert":{}}}"#);
        let first = delta.frame(1);
        let repeated = delta.frame(1);
        let value: serde_json::Value = serde_json::from_str(first.as_str()).unwrap();
        assert_eq!(value["base_rev"], 1);
        assert_eq!(value["rev"], 4);
        assert_eq!(value["patch"]["order_statuses"]["remove"], json!(["a"]));
        assert_eq!(first.as_str().as_ptr(), repeated.as_str().as_ptr());
    }

    #[test]
    fn slow_reader_history_is_bounded_and_requires_resync_after_eviction() {
        let mut history = LiveHistory::default();
        for revision in 1..=200 {
            history.push(delta(revision - 1, revision, "{}"));
        }
        assert!(history.entries.len() <= HISTORY_ENTRIES);
        assert!(history.bytes <= HISTORY_BYTES);
        assert!(history.recover(0, 200).is_none());
        assert!(history.recover(199, 200).is_some());
        history.clear();
        assert!(history.recover(199, 200).is_none());
    }

    #[test]
    fn expired_history_and_large_payloads_do_not_remain_cached() {
        let mut history = LiveHistory::default();
        let mut expired = delta(1, 2, "{}");
        expired.created = Instant::now() - HISTORY_TTL * 2;
        history.push(expired);
        assert!(history.recover(1, 2).is_none());
        let huge = format!(r#"{{"data":"{}"}}"#, "x".repeat(HISTORY_BYTES));
        history.push(delta(2, 3, &huge));
        assert!(history.entries.is_empty());
        assert_eq!(history.bytes, 0);
    }
}
