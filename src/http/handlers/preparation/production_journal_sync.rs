use super::*;
use crate::db::postgres_preparation::MAX_JOURNAL_WINDOW;
use crate::db::postgres_preparation::paddons::BalanceFilter;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{Duration, Instant, sleep_until};

const WAIT: Duration = Duration::from_secs(7);

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum JournalView {
    Orders,
    Batches,
    Rolls,
    Paddons,
}

impl JournalView {
    fn rows_key(&self) -> &'static str {
        match self {
            Self::Orders => "orders",
            Self::Batches => "batches",
            Self::Rolls => "rolls",
            Self::Paddons => "paddons",
        }
    }

    fn valid_status(&self, status: &str) -> bool {
        match self {
            Self::Paddons => matches!(status, "all" | "available" | "empty"),
            _ => matches!(status, "all" | "active" | "completed"),
        }
    }
}

#[derive(Deserialize)]
pub struct JournalSyncQuery {
    view: JournalView,
    q: String,
    status: String,
    length: usize,
    baseline: Option<JournalCheckpoint>,
    #[serde(default)]
    watch: bool,
}

#[derive(Deserialize, Serialize)]
struct JournalCheckpoint {
    version: String,
    hashes: Vec<String>,
}

fn fingerprint(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(&Sha256::digest(bytes)[..16])
}

impl JournalCheckpoint {
    fn new(rows: &[Value], next_offset: &Value) -> Self {
        let hashes = rows
            .iter()
            .map(|row| fingerprint(row.to_string().as_bytes()))
            .collect::<Vec<_>>();
        let version = fingerprint(&serde_json::to_vec(&(&hashes, next_offset)).unwrap());
        Self { version, hashes }
    }

    fn valid(&self, length: usize) -> bool {
        let valid_hash = |hash: &str| {
            hash.len() == 22
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        };
        self.hashes.len() <= length
            && valid_hash(&self.version)
            && self.hashes.iter().all(|hash| valid_hash(hash))
    }
}

fn invalid_query() -> ApiError {
    error(PreparationError::Invalid(
        "Jurnal sinxronlash so‘rovi noto‘g‘ri",
    ))
}

pub(super) fn with_checkpoint(mut page: Value, key: &str) -> Value {
    if let Some(rows) = page[key].as_array() {
        page["journal_sync"] = json!(JournalCheckpoint::new(rows, &page["next_offset"]));
    }
    page
}

async fn read_window(
    state: &AppState,
    actor: &Principal,
    query: &JournalSyncQuery,
) -> Result<Value, ApiError> {
    let store = store(state)?;
    let JournalSyncQuery {
        view,
        q,
        status,
        length,
        ..
    } = query;
    match view {
        JournalView::Orders => {
            store
                .production_journal_orders(&actor.ref_, q, status, 0, *length)
                .await
        }
        JournalView::Batches => {
            store
                .production_journal_batches(&actor.ref_, q, status, 0, *length)
                .await
        }
        JournalView::Rolls => {
            store
                .production_journal_rolls(&actor.ref_, q, status, 0, *length)
                .await
        }
        JournalView::Paddons => {
            let filter = match status.as_str() {
                "all" => BalanceFilter::All,
                "available" => BalanceFilter::Available,
                "empty" => BalanceFilter::Empty,
                _ => return Err(invalid_query()),
            };
            store
                .preparation_journal_paddons(actor, q.clone(), filter, *length as u32)
                .await
        }
    }
    .map_err(error)
}

fn sync_frame(page: &Value, rows_key: &str, baseline: Option<&JournalCheckpoint>) -> Value {
    let rows = page[rows_key].as_array().expect("journal list rows");
    let checkpoint = JournalCheckpoint::new(rows, &page["next_offset"]);
    if baseline.is_some_and(|before| {
        before.version == checkpoint.version && before.hashes == checkpoint.hashes
    }) {
        return json!({ "type": "unchanged", "version": checkpoint.version });
    }
    let known = baseline
        .map(|before| {
            before
                .hashes
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let upserts = checkpoint
        .hashes
        .iter()
        .zip(rows)
        .filter(|(hash, _)| !known.contains(hash.as_str()))
        .collect::<BTreeMap<_, _>>();
    json!({
        "type": if baseline.is_some() { "delta" } else { "snapshot" },
        "base_version": baseline.map(|before| before.version.as_str()),
        "journal_sync": checkpoint,
        "upserts": upserts,
        "has_more": page["has_more"],
        "next_offset": page["next_offset"],
    })
}

pub async fn sync_production_journal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(query): Json<JournalSyncQuery>,
) -> Result<([(axum::http::HeaderName, &'static str); 1], Json<Value>), ApiError> {
    let actor = authorize(&state, &headers).await?;
    if !(1..=MAX_JOURNAL_WINDOW).contains(&query.length)
        || query.q.chars().count() > 200
        || !query.view.valid_status(&query.status)
        || query
            .baseline
            .as_ref()
            .is_some_and(|before| !before.valid(query.length))
    {
        return Err(invalid_query());
    }
    let mut inventory = state.warehouse_events.subscribe_inventory();
    let mut journal = state.warehouse_events.subscribe_preparation_journal();
    let deadline = Instant::now() + WAIT;
    let mut current = actor;
    let frame = loop {
        let page = read_window(&state, &current, &query).await?;
        let frame = sync_frame(&page, query.view.rows_key(), query.baseline.as_ref());
        if !query.watch || frame["type"] != "unchanged" {
            break frame;
        }

        let changed = loop {
            tokio::select! {
                _ = sleep_until(deadline) => break false,
                event = inventory.recv() => break !matches!(event, Err(RecvError::Closed)),
                event = journal.recv() => match event {
                    Ok(owner) if owner != current.ref_ => continue,
                    Err(RecvError::Closed) => break false,
                    _ => break true,
                }
            }
        };
        if !changed {
            break frame;
        }
        // The next read observes all commits made before it. Discard pending
        // signals for those commits while keeping notifications during the read.
        inventory = inventory.resubscribe();
        journal = journal.resubscribe();
        let next = authorize(&state, &headers).await?;
        if next.ref_ != current.ref_ {
            return Err(error(PreparationError::Forbidden));
        }
        current = next;
    };
    Ok((
        [(axum::http::header::CACHE_CONTROL, "private, no-store")],
        Json(frame),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_delta_keeps_the_ordered_window_and_only_sends_new_content() {
        let first = json!({"order_id": "one", "roll_count": 1});
        let second = json!({"order_id": "two", "roll_count": 2});
        let third = json!({"order_id": "three", "roll_count": 3});
        let before = json!({"orders": [&first, &second], "has_more": true, "next_offset": 2});
        let checkpoint =
            JournalCheckpoint::new(before["orders"].as_array().unwrap(), &before["next_offset"]);
        assert_eq!(
            sync_frame(&before, "orders", Some(&checkpoint)),
            json!({"type":"unchanged", "version": checkpoint.version})
        );

        let after = json!({"orders": [&second, &third], "has_more": false, "next_offset": null});
        let delta = sync_frame(&after, "orders", Some(&checkpoint));
        assert_eq!(delta["type"], "delta");
        assert_eq!(delta["base_version"], checkpoint.version);
        assert_eq!(delta["journal_sync"]["hashes"][0], checkpoint.hashes[1]);
        assert_ne!(delta["journal_sync"]["hashes"][0], checkpoint.hashes[0]);
        assert_eq!(delta["upserts"].as_object().unwrap().len(), 1);
        let new_hash = delta["journal_sync"]["hashes"][1].as_str().unwrap();
        assert_eq!(delta["upserts"][new_hash], third);
        assert_eq!(delta["has_more"], false);
        assert!(delta["next_offset"].is_null());

        let snapshot = sync_frame(&after, "orders", None);
        assert_eq!(snapshot["type"], "snapshot");
        assert_eq!(snapshot["upserts"].as_object().unwrap().len(), 2);
    }
}
