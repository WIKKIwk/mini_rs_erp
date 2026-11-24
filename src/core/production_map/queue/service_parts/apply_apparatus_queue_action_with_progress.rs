impl ProductionMapService {
    pub async fn apply_apparatus_queue_action_with_progress(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        assigned_apparatus: &[String],
        actor: QueueActionActor,
        progress: QueueProgressInput,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        let _guard = self
            .queue_progress_action_guard(apparatus, action, &progress, &[], &[])
            .await?;
        self.enforce_qolip_start_boundary(apparatus, order_id, action, None)
            .await?;
        let prepared = self
            .prepare_apparatus_queue_action_with_progress(
                apparatus,
                order_id,
                action,
                assigned_apparatus,
                actor,
                progress,
            )
            .await?;
        self.commit_prepared_queue_action(prepared).await
    }

    pub async fn qolip_validation_required_for_action(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
    ) -> Result<bool, ProductionMapError> {
        if !matches!(
            action,
            queue_state::ApparatusQueueAction::Start | queue_state::ApparatusQueueAction::Resume
        ) {
            return Ok(false);
        }
        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        if !apparatus::requires_qolip_scan(&canonical) {
            return Ok(false);
        }
        if action == queue_state::ApparatusQueueAction::Start {
            return Ok(true);
        }
        Ok(self
            .store
            .active_order_run_session(canonical.runtime.apparatus_id.as_str(), order_id)
            .await?
            .is_some_and(|session| session.qolip_reacquisition_required()))
    }

    /// Core queue callers must not enter a Qolip-protected start or reacquisition
    /// without the trusted handler validation bound to the same canonical
    /// apparatus and order. Untrusted core callers therefore remain
    /// fail-closed.
    pub(crate) async fn enforce_qolip_start_boundary(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        qolip_validation: Option<&TrustedQolipStartValidation>,
    ) -> Result<(), ProductionMapError> {
        if !self
            .qolip_validation_required_for_action(apparatus, order_id, action)
            .await?
        {
            return Ok(());
        }
        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        if apparatus::requires_qolip_scan(&canonical)
            && !qolip_validation.is_some_and(|validation| {
                validation.matches(&canonical.runtime.apparatus_id, order_id)
            })
        {
            return Err(ProductionMapError::QolipCodeMismatch);
        }
        Ok(())
    }
}

include!("../execution.rs");
