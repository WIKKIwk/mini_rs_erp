impl ProductionMapService {
    /// Resolve live apparatus configuration by immutable canonical identity.
    /// Missing or invalid configuration returns `StoreFailed`; callers cannot
    /// continue with a title/name-derived fallback.
    pub(crate) async fn resolve_canonical_apparatus(
        &self,
        apparatus_id: &crate::core::apparatus_standard::ApparatusId,
    ) -> Result<
        std::sync::Arc<crate::core::apparatus_standard::RuntimeApparatusConfiguration>,
        ProductionMapError,
    > {
        let configuration = self
            .apparatus_resolver
            .resolve(apparatus_id)
            .await?
            .ok_or(ProductionMapError::StoreFailed)?;
        if configuration.runtime.apparatus_id != *apparatus_id
            || !configuration.has_coherent_source()
            || !configuration.is_active()
        {
            return Err(ProductionMapError::StoreFailed);
        }
        Ok(configuration)
    }

    pub(crate) async fn resolve_canonical_apparatus_text(
        &self,
        value: &str,
    ) -> Result<
        std::sync::Arc<crate::core::apparatus_standard::RuntimeApparatusConfiguration>,
        ProductionMapError,
    > {
        let id = crate::core::apparatus_standard::ApparatusId::new(value.trim().to_string())
            .map_err(|_| ProductionMapError::StoreFailed)?;
        self.resolve_canonical_apparatus(&id).await
    }

    pub(crate) async fn active_canonical_apparatuses(
        &self,
    ) -> Result<
        Vec<std::sync::Arc<crate::core::apparatus_standard::RuntimeApparatusConfiguration>>,
        ProductionMapError,
    > {
        let mut configurations = Vec::new();
        for configuration in self.apparatus_resolver.list().await? {
            if !configuration.has_coherent_source() {
                return Err(ProductionMapError::StoreFailed);
            }
            if !configuration.is_active() {
                continue;
            }
            configurations.push(configuration);
        }
        Ok(configurations)
    }

    /// Snapshot-only variant of [`Self::active_canonical_apparatuses`].
    ///
    /// The live stream must keep serving every healthy apparatus even while one
    /// configuration is mid-migration or incoherent. Write paths keep using the
    /// strict variant above and stay fail-closed.
    pub(crate) async fn snapshot_canonical_apparatuses(
        &self,
    ) -> Vec<std::sync::Arc<crate::core::apparatus_standard::RuntimeApparatusConfiguration>> {
        let configurations = match self.apparatus_resolver.list().await {
            Ok(configurations) => configurations,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "live snapshot continues without canonical apparatuses"
                );
                return Vec::new();
            }
        };
        configurations
            .into_iter()
            .filter(|configuration| {
                if !configuration.has_coherent_source() {
                    tracing::warn!(
                        apparatus = %configuration.runtime.apparatus_id.as_str(),
                        "skipping incoherent apparatus in live snapshot"
                    );
                    return false;
                }
                configuration.is_active()
            })
            .collect()
    }

    pub(crate) async fn queue_action_guard(&self) -> OwnedRwLockWriteGuard<()> {
        self.queue_action_locks.barrier.clone().write_owned().await
    }

    pub(crate) async fn apparatus_action_guard(
        &self,
        apparatus: &str,
    ) -> ScopedProductionMutationGuard {
        self.queue_action_locks
            .scoped(vec![("apparatus", apparatus.trim().to_string())])
            .await
    }

    pub(crate) async fn apparatus_preflight_guard(
        &self,
        apparatus: &str,
        idempotency_key: &str,
    ) -> ScopedProductionMutationGuard {
        self.queue_action_locks
            .scoped(vec![
                ("apparatus", apparatus.trim().to_string()),
                (
                    "print-preflight-idempotency",
                    idempotency_key.trim().to_string(),
                ),
            ])
            .await
    }

    pub(crate) async fn wip_action_guard(&self, batch_id: &str) -> ScopedProductionMutationGuard {
        self.queue_action_locks
            .scoped(vec![("wip", batch_id.trim().to_string())])
            .await
    }

    pub(crate) async fn queue_progress_action_guard(
        &self,
        apparatus: &str,
        action: queue_state::ApparatusQueueAction,
        progress: &QueueProgressInput,
        material_barcodes: &[String],
        qolip_codes: &[String],
    ) -> Result<QueueProgressMutationGuard, ProductionMapError> {
        if action == queue_state::ApparatusQueueAction::Freeze
            || progress.freeze_with_issue
            || !progress.freeze_request_id.trim().is_empty()
        {
            return Ok(QueueProgressMutationGuard::Exclusive {
                _guard: self.queue_action_guard().await,
            });
        }
        let mut keys = vec![("apparatus", apparatus.trim().to_string())];
        let batch_id = progress.progress_batch_id.trim();
        let qr_payload = progress.qr_payload.trim();
        if !batch_id.is_empty() {
            keys.push(("wip", batch_id.to_string()));
        } else if !qr_payload.is_empty() {
            // Resolve QR-only clients to the same identity as clients supplying
            // batch_id. This read never grants ownership: prepare and commit
            // still revalidate the input after acquiring the guard.
            if let Some(batch) = self.store.progress_batch_by_qr(qr_payload).await? {
                keys.push(("wip", batch.batch_id));
            } else if let Some(batch) = self.store.opening_wip_batch("", qr_payload).await? {
                keys.push(("wip", batch.batch.batch_id));
            } else {
                keys.push(("wip-qr", qr_payload.to_ascii_lowercase()));
            }
        }
        keys.extend(
            material_barcodes
                .iter()
                .filter(|code| !code.trim().is_empty())
                .map(|code| ("material", code.trim().to_ascii_lowercase())),
        );
        keys.extend(
            qolip_codes
                .iter()
                .filter(|code| !code.trim().is_empty())
                .map(|code| ("qolip", code.trim().to_ascii_lowercase())),
        );
        Ok(QueueProgressMutationGuard::Scoped {
            _guard: self.queue_action_locks.scoped(keys).await,
        })
    }

    pub fn subscribe_live(&self) -> broadcast::Receiver<ProductionMapLiveEvent> {
        self.live_notify.subscribe()
    }

    pub fn notify_live(&self) {
        self.snapshot_cache.revision.fetch_add(1, Ordering::AcqRel);
        let _ = self.live_notify.send(ProductionMapLiveEvent::Invalidate);
    }

    pub fn notify_live_delta(&self, delta: ProductionMapLiveDelta) {
        self.snapshot_cache.revision.fetch_add(1, Ordering::AcqRel);
        let _ = self
            .live_notify
            .send(ProductionMapLiveEvent::Delta(std::sync::Arc::new(delta)));
    }

    pub fn snapshot_epoch(&self) -> &str {
        &self.snapshot_cache.epoch
    }

    pub fn snapshot_revision(&self) -> u64 {
        self.snapshot_cache.revision.load(Ordering::Acquire)
    }

    pub(crate) fn notify_print_preflight(&self, order_id: &str, apparatus: &str) {
        let revision = self.snapshot_cache.revision.fetch_add(1, Ordering::AcqRel) + 1;
        let _ = self
            .live_notify
            .send(ProductionMapLiveEvent::PrintPreflight {
                revision,
                order_id: order_id.to_string(),
                apparatus: apparatus.to_string(),
            });
    }

    /// The ID is generated by persistence, included in the committed PG
    /// notification, and returned only after commit. Either publisher may win;
    /// claiming and sending are synchronous under one short mutex so a
    /// cancelled request cannot leave a claimed-but-unpublished commit.
    pub(crate) fn notify_print_preflight_once(
        &self,
        event_id: Option<&str>,
        order_id: &str,
        apparatus: &str,
    ) -> bool {
        let Some(event_id) = event_id else {
            self.notify_print_preflight(order_id, apparatus);
            return true;
        };
        self.notify_committed_event_once(event_id, || {
            self.notify_print_preflight(order_id, apparatus);
        })
    }

    /// A bounded exact-ID registry shared by local mutation publishers and the
    /// database listener. Eviction permits an extra invalidation, never loss.
    pub(crate) fn notify_committed_event_once(
        &self,
        event_id: &str,
        publish: impl FnOnce(),
    ) -> bool {
        self.publish_committed_events(&[Some(event_id)], |fresh| {
            if fresh.is_empty() {
                return false;
            }
            publish();
            true
        })
    }

    /// Deduplicate before deciding whether a coalesced batch is scoped.
    /// Already-published local commits must not turn an unrelated fresh
    /// preflight event into a generic invalidation.
    pub(crate) fn notify_print_preflight_batch(
        &self,
        changes: &[(Option<&str>, &str, &str)],
        generic: bool,
    ) {
        let ids = changes.iter().map(|change| change.0).collect::<Vec<_>>();
        self.publish_committed_events(&ids, |fresh| {
            let target = fresh.first().map(|index| changes[*index]);
            if generic
                || target.is_some_and(|target| {
                    fresh.iter().any(|index| {
                        let change = changes[*index];
                        change.1 != target.1 || change.2 != target.2
                    })
                })
            {
                self.notify_live();
            } else if let Some((_, order, apparatus)) = target {
                self.notify_print_preflight(order, apparatus);
            }
        });
    }

    fn publish_committed_events<T>(
        &self,
        ids: &[Option<&str>],
        publish: impl FnOnce(&[usize]) -> T,
    ) -> T {
        let mut events = self
            .snapshot_cache
            .committed_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let fresh = ids
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                if id.is_none_or(|id| events.remember(id)) {
                    Some(index)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        // Keep the guard through publication. No await or user I/O is allowed
        // in these internal callbacks.
        publish(&fresh)
    }

    pub async fn queue_events_replay(
        &self,
        apparatus: &str,
        from_rev: i64,
        to_rev: i64,
    ) -> Result<Vec<serde_json::Value>, ProductionMapError> {
        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        self.store
            .queue_events_replay(canonical.runtime.apparatus_id.as_str(), from_rev, to_rev)
            .await
    }

    pub async fn live_snapshot(&self) -> Result<ProductionMapLiveSnapshot, ProductionMapError> {
        Ok(self.live_snapshot_shared().await?.as_ref().clone())
    }

    /// Shared snapshot together with the revision it was built from.
    ///
    /// The revision is a monotonic counter bumped by every [`Self::notify_live`]
    /// call. Live-stream payloads expose it as `rev` so clients can detect a
    /// missed update and resync instead of sitting on stale state.
    pub async fn live_snapshot_shared_with_revision(
        &self,
    ) -> Result<(std::sync::Arc<ProductionMapLiveSnapshot>, u64), ProductionMapError> {
        loop {
            let revision = self.snapshot_cache.revision.load(Ordering::Acquire);
            {
                let cached = self.snapshot_cache.snapshot.read().await;
                if let Some(entry) = cached.as_ref()
                    && entry.revision == revision
                {
                    return Ok((entry.snapshot.clone(), entry.revision));
                }
            }

            let _rebuild_guard = self.snapshot_cache.rebuild_lock.lock().await;
            let revision = self.snapshot_cache.revision.load(Ordering::Acquire);
            {
                let cached = self.snapshot_cache.snapshot.read().await;
                if let Some(entry) = cached.as_ref()
                    && entry.revision == revision
                {
                    return Ok((entry.snapshot.clone(), entry.revision));
                }
            }

            let snapshot = self.build_production_snapshot().await?;
            let latest_revision = self.snapshot_cache.revision.load(Ordering::Acquire);
            if latest_revision != revision {
                continue;
            }

            let snapshot = std::sync::Arc::new(snapshot);
            *self.snapshot_cache.snapshot.write().await = Some(CachedProductionSnapshot {
                revision,
                snapshot: snapshot.clone(),
            });
            return Ok((snapshot, revision));
        }
    }

    pub async fn live_snapshot_shared(
        &self,
    ) -> Result<std::sync::Arc<ProductionMapLiveSnapshot>, ProductionMapError> {
        Ok(self.live_snapshot_shared_with_revision().await?.0)
    }

    async fn build_production_snapshot(
        &self,
    ) -> Result<ProductionMapLiveSnapshot, ProductionMapError> {
        let previous = self
            .snapshot_cache
            .snapshot
            .read()
            .await
            .as_ref()
            .map(|entry| entry.snapshot.clone());
        self.build_production_snapshot_in_scope(None, previous.as_deref())
            .await
    }
}
