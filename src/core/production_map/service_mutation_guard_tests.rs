use super::*;
use std::time::{Duration, Instant};
use tokio::sync::Barrier;

fn apparatus(value: &str) -> ResourceKey {
    ("apparatus", value.into())
}

fn wip(value: &str) -> ResourceKey {
    ("wip", value.into())
}

#[tokio::test]
async fn independent_apparatuses_overlap_even_when_using_the_same_group() {
    let locks = ProductionMutationLocks::default();
    let first = locks.scoped(vec![apparatus("cut-1"), wip("roll-1")]).await;
    let second = tokio::time::timeout(
        Duration::from_millis(100),
        locks.scoped(vec![apparatus("cut-2"), wip("roll-2")]),
    )
    .await
    .expect("another apparatus with another roll must not wait");
    drop((first, second));
    assert!(
        locks.resources.lock().unwrap().is_empty(),
        "finished rolls must leave no historical lock registry"
    );
}

#[tokio::test]
async fn same_apparatus_or_same_roll_cannot_overlap() {
    let locks = ProductionMutationLocks::default();
    let first = locks.scoped(vec![apparatus("cut-1"), wip("roll-1")]).await;
    assert!(
        tokio::time::timeout(
            Duration::from_millis(20),
            locks.scoped(vec![apparatus("cut-1"), wip("roll-2")])
        )
        .await
        .is_err()
    );
    assert!(
        tokio::time::timeout(
            Duration::from_millis(20),
            locks.scoped(vec![apparatus("cut-2"), wip("roll-1")])
        )
        .await
        .is_err()
    );
    drop(first);
    assert!(
        locks.resources.lock().unwrap().is_empty(),
        "cancelled waiters also retire their leases"
    );
    locks.scoped(vec![apparatus("cut-2"), wip("roll-1")]).await;
    assert!(locks.resources.lock().unwrap().is_empty());
}

#[tokio::test]
async fn broad_mutations_exclude_every_scoped_action() {
    let locks = ProductionMutationLocks::default();
    let scoped = locks.scoped(vec![apparatus("cut-1")]).await;
    assert!(
        tokio::time::timeout(
            Duration::from_millis(20),
            locks.barrier.clone().write_owned()
        )
        .await
        .is_err()
    );
    drop(scoped);
    let broad = locks.barrier.clone().write_owned().await;
    assert!(
        tokio::time::timeout(
            Duration::from_millis(20),
            locks.scoped(vec![apparatus("cut-2")])
        )
        .await
        .is_err()
    );
    drop(broad);
    locks.scoped(vec![apparatus("cut-2")]).await;
}

#[tokio::test]
async fn opposite_resource_order_and_waiter_cancellation_do_not_split_locks() {
    let locks = Arc::new(ProductionMutationLocks::default());
    let first = locks
        .scoped(vec![apparatus("cut-1"), wip("roll-1"), wip("roll-2")])
        .await;
    let waiter_locks = locks.clone();
    let waiting = tokio::spawn(async move {
        waiter_locks
            .scoped(vec![wip("roll-2"), wip("roll-1"), apparatus("cut-2")])
            .await
    });
    tokio::task::yield_now().await;
    assert!(
        tokio::time::timeout(
            Duration::from_millis(20),
            locks.scoped(vec![wip("roll-1"), apparatus("cut-3")])
        )
        .await
        .is_err()
    );
    waiting.abort();
    assert!(matches!(waiting.await, Err(error) if error.is_cancelled()));
    drop(first);
    assert!(locks.resources.lock().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "manual timing comparison; no throughput claim for SQL or the printer"]
async fn benchmark_independent_apparatus_guards() {
    const APPARATUSES: usize = 8;
    const WORK_MS: u64 = 40;
    let legacy = Arc::new(Mutex::new(()));
    let start = Arc::new(Barrier::new(APPARATUSES));
    let began = Instant::now();
    let mut workers = Vec::new();
    for _ in 0..APPARATUSES {
        let legacy = legacy.clone();
        let start = start.clone();
        workers.push(tokio::spawn(async move {
            start.wait().await;
            let _guard = legacy.lock().await;
            tokio::time::sleep(Duration::from_millis(WORK_MS)).await;
        }));
    }
    for worker in workers {
        worker.await.unwrap();
    }
    let legacy_ms = began.elapsed().as_secs_f64() * 1000.0;

    let scoped = Arc::new(ProductionMutationLocks::default());
    let start = Arc::new(Barrier::new(APPARATUSES));
    let began = Instant::now();
    let mut workers = Vec::new();
    for index in 0..APPARATUSES {
        let scoped = scoped.clone();
        let start = start.clone();
        workers.push(tokio::spawn(async move {
            start.wait().await;
            let _guard = scoped
                .scoped(vec![
                    apparatus(&format!("cut-{index}")),
                    wip(&format!("roll-{index}")),
                ])
                .await;
            tokio::time::sleep(Duration::from_millis(WORK_MS)).await;
        }));
    }
    for worker in workers {
        worker.await.unwrap();
    }
    let scoped_ms = began.elapsed().as_secs_f64() * 1000.0;
    assert!(
        scoped_ms < legacy_ms / 2.0,
        "independent physical machines must overlap"
    );
    assert!(scoped.resources.lock().unwrap().is_empty());
    println!(
        "{}",
        serde_json::json!({
            "benchmark": "independent_apparatus_guards", "apparatuses": APPARATUSES,
            "work_ms_per_apparatus": WORK_MS, "legacy_global_mutex_ms": legacy_ms,
            "scoped_resource_guards_ms": scoped_ms, "speedup": legacy_ms / scoped_ms,
        })
    );
}
