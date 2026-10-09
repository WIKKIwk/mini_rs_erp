use std::collections::BTreeMap;
use std::sync::{Arc, Mutex as RegistryMutex, Weak};

use tokio::sync::{Mutex, OwnedMutexGuard, OwnedRwLockReadGuard, RwLock};

type ResourceKey = (&'static str, String);
type Registry = Arc<RegistryMutex<BTreeMap<ResourceKey, Weak<Mutex<()>>>>>;

/// Multi-queue mutations take the exclusive barrier; ordinary worker actions
/// share it and serialize only their physical apparatus and scanned resources.
#[derive(Default)]
pub(super) struct ProductionMutationLocks {
    pub(super) barrier: Arc<RwLock<()>>,
    resources: Registry,
}

pub(crate) struct ScopedProductionMutationGuard {
    // Release mutexes before retiring their registry leases.
    _guards: Vec<OwnedMutexGuard<()>>,
    _leases: Vec<ResourceLease>,
    _barrier: OwnedRwLockReadGuard<()>,
}

struct ResourceLease {
    key: ResourceKey,
    lock: Arc<Mutex<()>>,
    registry: Registry,
}

impl Drop for ResourceLease {
    fn drop(&mut self) {
        let mut registry = self
            .registry
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Every owner and waiter holds a lease before awaiting its mutex.
        // Retire only the final lease, so a waiting action cannot be bypassed
        // by creating another mutex for the same physical resource.
        if Arc::strong_count(&self.lock) == 1
            && registry
                .get(&self.key)
                .is_some_and(|entry| entry.ptr_eq(&Arc::downgrade(&self.lock)))
        {
            registry.remove(&self.key);
        }
    }
}

impl ProductionMutationLocks {
    pub(super) async fn scoped(&self, mut keys: Vec<ResourceKey>) -> ScopedProductionMutationGuard {
        let barrier = self.barrier.clone().read_owned().await;
        keys.sort_unstable();
        keys.dedup();
        let leases = {
            let mut registry = self
                .resources
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            keys.into_iter()
                .map(|key| {
                    let lock = registry
                        .get(&key)
                        .and_then(Weak::upgrade)
                        .unwrap_or_else(|| {
                            let lock = Arc::new(Mutex::new(()));
                            registry.insert(key.clone(), Arc::downgrade(&lock));
                            lock
                        });
                    ResourceLease {
                        key,
                        lock,
                        registry: self.resources.clone(),
                    }
                })
                .collect::<Vec<_>>()
        };
        let mut guard = ScopedProductionMutationGuard {
            _guards: Vec::with_capacity(leases.len()),
            _leases: leases,
            _barrier: barrier,
        };
        // Deterministic ordering also handles requests sharing several rolls,
        // materials or Qolips without an in-process lock-order deadlock.
        for lease in &guard._leases {
            guard._guards.push(lease.lock.clone().lock_owned().await);
        }
        guard
    }
}

#[cfg(test)]
#[path = "service_mutation_guard_tests.rs"]
mod tests;
