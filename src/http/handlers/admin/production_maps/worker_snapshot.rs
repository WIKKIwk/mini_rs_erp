use super::*;
use crate::core::production_map::ProductionMapLiveSnapshot;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(super) async fn scope_token(
    state: &AppState,
    principal: &Principal,
) -> Result<(Vec<String>, String), AdminError> {
    let mut assigned = state.admin.principal_assigned_apparatus(principal).await;
    assigned.retain(|id| queue_state::is_canonical_apparatus_id(id));
    assigned.sort();
    assigned.dedup();
    if assigned.is_empty() {
        return Err(forbidden());
    }
    let mut digest = Sha256::new();
    digest.update(format!("{:?}\0{}\0", principal.role, principal.ref_));
    for apparatus in &assigned {
        digest.update(apparatus.as_bytes());
        digest.update([0]);
    }
    let fingerprint = format!("{:x}", digest.finalize());
    Ok((assigned, fingerprint[..16].to_string()))
}

fn select_orders<T: Clone>(
    values: &BTreeMap<String, T>,
    orders: &BTreeSet<String>,
) -> BTreeMap<String, T> {
    values
        .iter()
        .filter(|(id, _)| orders.contains(*id))
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect()
}

pub(super) fn project(
    snapshot: &ProductionMapLiveSnapshot,
    assigned: &[String],
    history_order_ids: impl IntoIterator<Item = String>,
) -> ProductionMapLiveSnapshot {
    let own = |apparatus: &str| queue_state::apparatus_matches_assigned(apparatus, assigned);
    let mut orders = history_order_ids.into_iter().collect::<BTreeSet<_>>();
    for (apparatus, ids) in snapshot
        .sequences
        .iter()
        .chain(snapshot.visible_order_ids.iter())
    {
        if own(apparatus) {
            orders.extend(ids.iter().cloned());
        }
    }
    for (apparatus, controls) in &snapshot.queue_action_controls {
        if own(apparatus) {
            orders.extend(controls.keys().cloned());
        }
    }
    for (apparatus, frozen) in &snapshot.frozen_orders_by_apparatus {
        if own(apparatus) {
            orders.extend(frozen.iter().map(|order| order.order_id.clone()));
        }
    }
    macro_rules! apparatus_field {
        ($field:ident) => {
            snapshot
                .$field
                .iter()
                .filter(|(apparatus, _)| own(apparatus))
                .map(|(apparatus, value)| (apparatus.clone(), value.clone()))
                .collect()
        };
    }
    ProductionMapLiveSnapshot {
        maps: snapshot
            .maps
            .iter()
            .filter(|saved| orders.contains(&saved.map.id))
            .cloned()
            .collect(),
        sequences: apparatus_field!(sequences),
        sequence_versions: apparatus_field!(sequence_versions),
        sequence_revisions: apparatus_field!(sequence_revisions),
        visible_order_ids: apparatus_field!(visible_order_ids),
        frozen_orders_by_apparatus: apparatus_field!(frozen_orders_by_apparatus),
        // Preserve cross-machine state for these orders: the sheet displays
        // the whole route, and upstream/downstream changes affect permission.
        queue_states: snapshot
            .queue_states
            .iter()
            .map(|(apparatus, states)| (apparatus.clone(), select_orders(states, &orders)))
            .collect(),
        queue_action_controls: snapshot
            .queue_action_controls
            .iter()
            .map(|(apparatus, controls)| (apparatus.clone(), select_orders(controls, &orders)))
            .collect(),
        stage_states: select_orders(&snapshot.stage_states, &orders),
        order_statuses: select_orders(&snapshot.order_statuses, &orders),
        order_controls: select_orders(&snapshot.order_controls, &orders),
        queue_policies: snapshot.queue_policies.clone(),
    }
}

pub(super) async fn project_for_principal(
    state: &AppState,
    principal: &Principal,
    snapshot: Arc<ProductionMapLiveSnapshot>,
    history_order_ids: impl IntoIterator<Item = String>,
) -> Result<(Arc<ProductionMapLiveSnapshot>, String), AdminError> {
    let (assigned, scope) = scope_token(state, principal).await?;
    Ok((
        Arc::new(project(&snapshot, &assigned, history_order_ids)),
        scope,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_projection_preserves_route_and_history_but_removes_other_orders() {
        let apparatus = "apparatus:default:bosma_9";
        let next = "apparatus:default:asset-008";
        let snapshot = ProductionMapLiveSnapshot {
            maps: vec![],
            sequences: BTreeMap::from([
                (apparatus.into(), vec!["own".into()]),
                (next.into(), vec!["other".into()]),
            ]),
            sequence_versions: BTreeMap::new(),
            sequence_revisions: BTreeMap::new(),
            visible_order_ids: BTreeMap::new(),
            queue_policies: vec![],
            queue_action_controls: BTreeMap::new(),
            order_statuses: BTreeMap::new(),
            order_controls: BTreeMap::new(),
            frozen_orders_by_apparatus: BTreeMap::new(),
            queue_states: BTreeMap::from([(
                next.into(),
                BTreeMap::from([
                    ("own".into(), "pending".into()),
                    ("other".into(), "pending".into()),
                    ("history".into(), "completed".into()),
                ]),
            )]),
            stage_states: BTreeMap::from([
                (
                    "own".into(),
                    BTreeMap::from([("stage".into(), "pending".into())]),
                ),
                ("other".into(), BTreeMap::new()),
            ]),
        };
        let projected = project(&snapshot, &[apparatus.into()], ["history".into()]);
        assert_eq!(projected.sequences.len(), 1);
        assert_eq!(projected.queue_states[next].len(), 2);
        assert_eq!(projected.queue_states[next]["own"], "pending");
        assert_eq!(projected.queue_states[next]["history"], "completed");
        assert!(!projected.stage_states.contains_key("other"));
    }
}
