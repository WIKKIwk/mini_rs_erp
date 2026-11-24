//! Commit-only invalidation for production readers on other server processes.
//! PostgreSQL notifications are hints, not a durable event log: LISTEN startup
//! and every reconnect invalidate all cached views to cover an offline gap.
use crate::core::apparatus_standard::CanonicalApparatusService;
use crate::core::production_map::{ProductionMapLiveDelta, ProductionMapService};
use serde::Deserialize;
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use std::time::Duration;

const CHANNEL: &str = "mini_production_live";
const COALESCE: Duration = Duration::from_millis(10);
const MAX_BATCH: usize = 128;

pub(crate) fn start_listener(
    pool: PgPool,
    service: ProductionMapService,
    apparatus: CanonicalApparatusService,
) -> Option<tokio::task::JoinHandle<()>> {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return None;
    };
    Some(runtime.spawn(async move {
        loop {
            if pool.is_closed() {
                return;
            }
            if let Err(error) = listen_once(&pool, &service, &apparatus).await {
                if pool.is_closed() {
                    return;
                }
                tracing::warn!(%error, "production postgres listener disconnected");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }))
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PreflightChange {
    kind: String,
    order_id: String,
    apparatus: String,
    #[serde(default)]
    event_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SequenceChange {
    kind: String,
    event_id: String,
    apparatus: String,
    base_revision: i64,
    revision: i64,
    ops: Vec<serde_json::Value>,
    version: String,
}

impl SequenceChange {
    fn valid(&self) -> bool {
        self.kind == "sequence"
            && self.event_id.len() == 64
            && self.event_id.bytes().all(|b| b.is_ascii_hexdigit())
            && crate::core::production_map::queue_state::is_canonical_apparatus_id(&self.apparatus)
            && self.revision > 0
            && self.base_revision == self.revision - 1
            && self.version.len() == 64
            && self.version.bytes().all(|b| b.is_ascii_hexdigit())
            && self.ops.len() == 1
            && self.ops[0].get("type").and_then(|v| v.as_str()) == Some("move")
            && self.ops[0]
                .get("id")
                .and_then(|v| v.as_str())
                .is_some_and(|id| !id.is_empty())
    }

    fn publish(&self, service: &ProductionMapService) {
        service.notify_committed_event_once(&self.event_id, || {
            service.notify_live_delta(ProductionMapLiveDelta {
                epoch: service.snapshot_epoch().to_string(),
                apparatus: self.apparatus.clone(),
                base_revision: self.base_revision,
                revision: self.revision,
                ops: self.ops.clone(),
                version: self.version.clone(),
            });
        });
    }
}

#[derive(Default)]
struct Pending {
    generic: bool,
    apparatus: bool,
    preflight: Vec<PreflightChange>,
    sequence: Vec<SequenceChange>,
}

impl Pending {
    fn add(&mut self, payload: &str) {
        if payload == "apparatus" {
            self.apparatus = true;
            self.generic = true;
            return;
        }
        if let Ok(change) = serde_json::from_str::<SequenceChange>(payload) {
            if change.valid() {
                if !self.sequence.contains(&change) {
                    self.sequence.push(change);
                }
                return;
            }
        }
        match serde_json::from_str::<PreflightChange>(payload) {
            Ok(change)
                if change.kind == "preflight"
                    && !change.order_id.is_empty()
                    && crate::core::production_map::queue_state::is_canonical_apparatus_id(
                        &change.apparatus,
                    )
                    && change.event_id.as_ref().is_none_or(|id| {
                        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
                    }) =>
            {
                if !self.preflight.contains(&change) {
                    self.preflight.push(change);
                }
            }
            _ => self.generic = true,
        }
    }

    async fn publish(self, service: &ProductionMapService, apparatus: &CanonicalApparatusService) {
        if self.apparatus {
            apparatus.invalidate_runtime_caches().await;
        }
        // Preserve each apparatus delta in commit order. Claiming the same
        // committed ID locally or through LISTEN produces the identical event.
        for change in self.sequence {
            change.publish(service);
        }
        let changes = self
            .preflight
            .iter()
            .map(|change| {
                (
                    change.event_id.as_deref(),
                    change.order_id.as_str(),
                    change.apparatus.as_str(),
                )
            })
            .collect::<Vec<_>>();
        service.notify_print_preflight_batch(&changes, self.generic);
    }
}

async fn invalidate_after_gap(
    service: &ProductionMapService,
    apparatus: &CanonicalApparatusService,
) {
    apparatus.invalidate_runtime_caches().await;
    service.notify_live();
}

async fn listen_once(
    pool: &PgPool,
    service: &ProductionMapService,
    apparatus: &CanonicalApparatusService,
) -> Result<(), sqlx::Error> {
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen(CHANNEL).await?;
    invalidate_after_gap(service, apparatus).await;
    loop {
        // recv() would hide an automatic reconnection and therefore a missed
        // notification gap. try_recv() explicitly reports that boundary.
        let Some(notification) = listener.try_recv().await? else {
            invalidate_after_gap(service, apparatus).await;
            continue;
        };
        let mut pending = Pending::default();
        pending.add(notification.payload());
        let deadline = tokio::time::Instant::now() + COALESCE;
        for _ in 1..MAX_BATCH {
            match tokio::time::timeout_at(deadline, listener.try_recv()).await {
                Ok(Ok(Some(notification))) => pending.add(notification.payload()),
                Ok(Ok(None)) => {
                    pending.generic = true;
                    pending.apparatus = true;
                    break;
                }
                Ok(Err(error)) => return Err(error),
                Err(_) => break,
            }
        }
        pending.publish(service, apparatus).await;
    }
}

#[cfg(test)]
#[path = "realtime_tests.rs"]
mod tests;
