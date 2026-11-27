use super::*;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgListener;
use std::{
    collections::{BTreeSet, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;

const CHANNEL: &str = "mini_preparation_formula_live";
const HISTORY_LIMIT: usize = 1024;
const DELTA_LIMIT: usize = 64;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct FormulaCursor {
    pub epoch: String,
    pub revision: u64,
}

#[derive(Clone)]
pub(crate) struct FormulaLive(Arc<FormulaLiveInner>);

struct FormulaLiveInner {
    history: Mutex<History>,
    wake: watch::Sender<()>,
}

struct History {
    ready: bool,
    cursor: FormulaCursor,
    changes: VecDeque<(String, String)>,
}

impl History {
    fn new(ready: bool) -> Self {
        Self {
            ready,
            cursor: FormulaCursor {
                epoch: format!("{:032x}", rand::random::<u128>()),
                revision: 0,
            },
            changes: VecDeque::new(),
        }
    }

    fn push(&mut self, changes: BTreeSet<(String, String)>) {
        for change in changes {
            if self.changes.len() == HISTORY_LIMIT {
                self.changes.pop_front();
            }
            self.changes.push_back(change);
            self.cursor.revision += 1;
        }
    }

    fn since(&self, owner: &str, base: Option<&FormulaCursor>) -> Option<FormulaUpdate> {
        if !self.ready {
            return None;
        }
        let oldest = self.cursor.revision - self.changes.len() as u64;
        let products = base
            .filter(|base| {
                base.epoch == self.cursor.epoch
                    && (oldest..=self.cursor.revision).contains(&base.revision)
            })
            .and_then(|base| {
                let products: BTreeSet<_> = self
                    .changes
                    .iter()
                    .skip((base.revision - oldest) as usize)
                    .filter(|(actor, _)| actor == owner)
                    .map(|(_, product)| product.clone())
                    .collect();
                (products.len() <= DELTA_LIMIT).then(|| products.into_iter().collect())
            });
        Some(FormulaUpdate {
            cursor: self.cursor.clone(),
            products,
        })
    }
}

pub(crate) struct FormulaUpdate {
    pub cursor: FormulaCursor,
    // None requires a fresh snapshot. An empty list advances only the cursor.
    pub products: Option<Vec<String>>,
}

impl FormulaLive {
    pub(crate) fn start(pool: PgPool) -> Self {
        let live = Self(Arc::new(FormulaLiveInner {
            history: Mutex::new(History::new(false)),
            wake: watch::channel(()).0,
        }));
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let live = live.clone();
            runtime.spawn(async move {
                while !pool.is_closed() {
                    let result = live.listen(&pool).await;
                    live.reset(false);
                    if let Err(error) = result {
                        if !pool.is_closed() {
                            tracing::warn!(%error, "preparation formula listener disconnected");
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                    }
                }
            });
        }
        live
    }

    pub(crate) fn cursor(&self) -> FormulaCursor {
        self.0.history.lock().unwrap().cursor.clone()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<()> {
        self.0.wake.subscribe()
    }

    pub(crate) fn since(&self, owner: &str, base: Option<&FormulaCursor>) -> Option<FormulaUpdate> {
        self.0.history.lock().unwrap().since(owner, base)
    }

    fn reset(&self, ready: bool) {
        *self.0.history.lock().unwrap() = History::new(ready);
        self.0.wake.send_replace(());
    }

    async fn listen(&self, pool: &PgPool) -> Result<(), sqlx::Error> {
        let mut listener = PgListener::connect_with(pool).await?;
        listener.listen(CHANNEL).await?;
        self.reset(true);
        loop {
            // try_recv reports a gap even when SQLx can reconnect automatically.
            let Some(first) = listener.try_recv().await? else {
                return Ok(());
            };
            let deadline = tokio::time::Instant::now() + Duration::from_millis(10);
            let mut batch = BTreeSet::new();
            let mut notification = first;
            let mut reset = false;
            for index in 0..128 {
                let payload = notification.payload();
                if payload == "reset" {
                    reset = true;
                } else if let Ok(change) = serde_json::from_str::<(String, String)>(payload) {
                    batch.insert(change);
                }
                if index == 127 {
                    break;
                }
                match tokio::time::timeout_at(deadline, listener.try_recv()).await {
                    Ok(Ok(Some(next))) => notification = next,
                    Ok(Ok(None)) => return Ok(()),
                    Ok(Err(error)) => return Err(error),
                    Err(_) => break,
                }
            }
            {
                let mut history = self.0.history.lock().unwrap();
                if reset {
                    *history = History::new(true);
                }
                history.push(batch);
            }
            self.0.wake.send_replace(());
        }
    }
}

impl PostgresPreparationStore {
    pub(crate) async fn formula_library_rows(
        &self,
        owner: &str,
        products: Option<&[String]>,
    ) -> Result<Value, PreparationError> {
        let mut tx = self.pool.begin().await?;
        // Sets, counts and recipes must come from the same committed view.
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let sets = Self::formula_set_rows(&mut *tx, owner, products, true).await?;
        let recipes = Self::formula_catalog_rows(&mut *tx, owner, products, true).await?;
        tx.commit().await?;
        Ok(json!({"sets": sets, "formulas": recipes}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resumes_only_the_owners_changed_products_and_resets_after_a_gap() {
        let mut history = History::new(true);
        let base = history.cursor.clone();
        history.push(BTreeSet::from([
            ("one".into(), "A".into()),
            ("two".into(), "B".into()),
        ]));
        history.push(BTreeSet::from([("one".into(), "A".into())]));
        let one = history.since("one", Some(&base)).unwrap();
        assert_eq!(one.products, Some(vec!["A".into()]));
        assert_eq!(one.cursor.revision, 3);
        assert_eq!(
            history.since("two", Some(&one.cursor)).unwrap().products,
            Some(vec![])
        );
        history.push(
            (0..HISTORY_LIMIT)
                .map(|i| ("two".into(), i.to_string()))
                .collect(),
        );
        assert_eq!(history.since("one", Some(&base)).unwrap().products, None);
        assert_eq!(
            History::new(true)
                .since("one", Some(&one.cursor))
                .unwrap()
                .products,
            None
        );
    }
}

#[cfg(test)]
#[path = "formula_live_postgres_tests.rs"]
mod postgres_tests;
