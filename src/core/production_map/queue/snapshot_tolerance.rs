//! Fail-soft helpers for the production-map live snapshot.
//!
//! The live stream is a read path consumed by every operator screen. A single
//! corrupt row (deleted apparatus, edited map, torn session payload) must only
//! hide that one order's controls — never fail the snapshot for everyone.
//! Write paths keep their own strict validation and stay fail-closed.

use super::super::service_progress_support::rezka_output_kadr_counts;
use super::super::types::{
    OrderRunInputLink, OrderRunSession, ProductionMapDefinition, RezkaActivePartialRoll,
    order_run_input_links_from_payload, rezka_active_partial_rolls_from_payload,
    rezka_merge_state_is_consistent,
};
use super::super::*;

pub(super) fn warn_skipped_snapshot_apparatus(storage_key: &str) {
    tracing::warn!(
        apparatus = %storage_key,
        "skipping live snapshot apparatus without canonical configuration"
    );
}

pub(super) fn warn_skipped_snapshot_order(order_id: &str, storage_key: &str, reason: &str) {
    tracing::warn!(
        order_id = %order_id,
        apparatus = %storage_key,
        reason = %reason,
        "skipping live snapshot order control"
    );
}

/// Session lineage for one snapshot control entry.
///
/// Returns `None` when the stored session payload is unreadable or internally
/// inconsistent; the caller then skips only this order. A missing session is
/// normal and yields empty lineage.
pub(super) fn snapshot_session_lineage(
    active_session: Option<&OrderRunSession>,
    is_rezka: bool,
    order_id: &str,
    storage_key: &str,
) -> Option<(Vec<OrderRunInputLink>, Vec<RezkaActivePartialRoll>)> {
    let Some(session) = active_session else {
        return Some((Vec::new(), Vec::new()));
    };
    let input_lineage = match order_run_input_links_from_payload(&session.payload_json) {
        Ok(links) => links,
        Err(_) => {
            warn_skipped_snapshot_order(order_id, storage_key, "unreadable session lineage");
            return None;
        }
    };
    let active_partial_rolls = if is_rezka {
        match rezka_active_partial_rolls_from_payload(&session.payload_json) {
            Ok(rolls) => rolls,
            Err(_) => {
                warn_skipped_snapshot_order(order_id, storage_key, "unreadable partial rolls");
                return None;
            }
        }
    } else {
        Vec::new()
    };
    if is_rezka && !rezka_merge_state_is_consistent(&input_lineage, &active_partial_rolls) {
        warn_skipped_snapshot_order(order_id, storage_key, "inconsistent merge state");
        return None;
    }
    Some((input_lineage, active_partial_rolls))
}

/// Rezka frame-group counts for one snapshot control entry.
///
/// Returns `None` when the stored map no longer carries usable frame data;
/// the caller then skips only this order.
pub(super) fn snapshot_rezka_output_kadr_counts(
    order_map: &ProductionMapDefinition,
    storage_key: &str,
    stage_node_id: &str,
    input_contained_kadr_count: Option<usize>,
    order_id: &str,
) -> Option<Vec<i64>> {
    let frame_groups = match rezka_output_kadr_counts(
        order_map,
        storage_key,
        stage_node_id,
        input_contained_kadr_count,
    ) {
        Ok(groups) => groups,
        Err(error) => {
            tracing::warn!(
                ?error,
                order_id = %order_id,
                apparatus = %storage_key,
                "skipping live snapshot control with unreadable frame groups"
            );
            return None;
        }
    };
    let mut counts = Vec::with_capacity(frame_groups.len());
    for value in frame_groups {
        match i64::try_from(value) {
            Ok(count) => counts.push(count),
            Err(error) => {
                tracing::warn!(
                    ?error,
                    order_id = %order_id,
                    apparatus = %storage_key,
                    "skipping live snapshot control with oversized frame groups"
                );
                return None;
            }
        }
    }
    Some(counts)
}
