// Presentation is optional after a committed write. Bound it so snapshot
// churn cannot delay the acknowledgement or invite a replay of the mutation.
const QUEUE_ACTION_CONTROL_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);

pub(super) async fn queue_action_response_control(
    state: &AppState,
    principal: &Principal,
    headers: &HeaderMap,
    apparatus: &str,
    order_id: &str,
) -> Option<serde_json::Value> {
    tokio::time::timeout(QUEUE_ACTION_CONTROL_TIMEOUT, async {
        let scope = super::order_scan_bootstrap::authorization_scope(state, principal, apparatus)
            .await
            .ok()?;
        let (snapshot, revision) = state
            .production_maps
            .worker_snapshot_shared_with_revision(&[apparatus.to_string()], &[order_id.to_string()])
            .await
            .ok()?;
        let control = snapshot
            .queue_action_controls
            .get(apparatus)?
            .get(order_id)?;
        let current = authorize_any_capability(
            state,
            headers,
            &[
                Capability::AdminAccess,
                Capability::ProductionMapManage,
                Capability::ApparatusQueueManage,
            ],
        )
        .await
        .ok()?;
        let current_scope =
            super::order_scan_bootstrap::authorization_scope(state, &current, apparatus)
                .await
                .ok()?;
        if current_scope != scope || state.production_maps.snapshot_revision() != revision {
            return None;
        }
        Some(serde_json::json!({
            "apparatus": apparatus,
            "order_id": order_id,
            "rev": revision,
            "epoch": state.production_maps.snapshot_epoch(),
            "scope": scope,
            "control": control,
            "queue_state": control.state.as_str(),
            "stage_states": snapshot.stage_states.get(order_id).cloned().unwrap_or_default(),
            "order_control": snapshot.order_controls.get(order_id)
                .map(|record| record.state.as_str()).unwrap_or("active"),
        }))
    })
    .await
    .ok()
    .flatten()
}
