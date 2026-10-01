use std::collections::{BTreeMap, BTreeSet};

use super::super::*;

pub(super) struct BatchLineage {
    pub batch_ids: BTreeSet<String>,
    pub descendant_ids: BTreeSet<String>,
    pub frontier_ids: BTreeSet<String>,
    pub session_ids: BTreeSet<String>,
    pub edges: Vec<ProductionQrLineageEdge>,
    pub complete: bool,
}

fn session_input_ids(session: &OrderRunSession) -> Result<BTreeSet<String>, ()> {
    let mut ids = order_run_input_links_from_payload(&session.payload_json)?
        .into_iter()
        .map(|link| link.input_batch_id.trim().to_string())
        .collect::<BTreeSet<_>>();
    if ids.is_empty()
        && let Some(id) = session
            .payload_json
            .get("input_progress_batch_id")
            .and_then(|v| v.as_str())
        && !id.trim().is_empty()
    {
        ids.insert(id.trim().to_string());
    }
    Ok(ids)
}

fn expand(
    seeds: &BTreeSet<String>,
    links: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    let mut ids = seeds.clone();
    let mut pending = seeds.iter().cloned().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        if let Some(neighbors) = links.get(&id) {
            for neighbor in neighbors {
                if ids.insert(neighbor.clone()) {
                    pending.push(neighbor.clone());
                }
            }
        }
    }
    ids
}

/// Follow outputs from the scanned roll, then their inputs. Never follow
/// outputs from an ancestor: that would include unrelated split siblings.
pub(super) fn batch_lineage(
    scanned: &OrderProgressBatch,
    batches: &[OrderProgressBatch],
    sessions: &[OrderRunSession],
) -> BatchLineage {
    let order_id = scanned.order_id.trim();
    let batches_by_id = batches
        .iter()
        .filter(|batch| batch.order_id.trim() == order_id)
        .map(|batch| (batch.batch_id.trim(), batch))
        .collect::<BTreeMap<_, _>>();
    let sessions_by_id = sessions
        .iter()
        .filter(|session| session.order_id.trim() == order_id)
        .map(|session| (session.session_id.trim(), session))
        .collect::<BTreeMap<_, _>>();
    let session_inputs = sessions_by_id
        .iter()
        .map(|(id, session)| (*id, session_input_ids(session)))
        .collect::<BTreeMap<_, _>>();
    let mut parents = BTreeMap::<String, BTreeSet<String>>::new();
    let mut children = BTreeMap::<String, BTreeSet<String>>::new();
    let mut incomplete_ids = BTreeSet::<String>::new();
    for batch in batches_by_id.values() {
        let id = batch.batch_id.trim();
        let producer = sessions_by_id.get(batch.session_id.trim());
        let sources = progress_batch_input_links_from_payload(&batch.payload_json);
        let mut input_ids = BTreeSet::<String>::new();
        match sources {
            Ok(links) if !links.is_empty() => {
                // Per-output source links are more precise than the last active
                // parent or the session-wide merge list (especially Rezka frames).
                input_ids.extend(
                    links
                        .iter()
                        .map(|link| link.input_batch_id.trim().to_string()),
                );
            }
            sources => {
                if sources.is_err() {
                    incomplete_ids.insert(id.to_string());
                }
                if !batch.parent_batch_id.trim().is_empty() {
                    input_ids.insert(batch.parent_batch_id.trim().to_string());
                } else if !batch
                    .payload_json
                    .get("recovered_sibling_lineage")
                    .is_some_and(|v| v == true)
                    && !batch
                        .payload_json
                        .get(SOURCE_INPUT_LINKS_PAYLOAD_FIELD)
                        .is_some()
                {
                    match session_inputs.get(batch.session_id.trim()) {
                        Some(Ok(ids)) if ids.len() == 1 => input_ids.extend(ids.iter().cloned()),
                        Some(Ok(ids)) if ids.len() > 1 => {
                            incomplete_ids.insert(id.to_string());
                        }
                        Some(Err(())) => {
                            incomplete_ids.insert(id.to_string());
                        }
                        _ => {}
                    }
                }
            }
        }
        if producer.is_none() {
            incomplete_ids.insert(id.to_string());
        }
        if input_ids.is_empty() {
            let proven_root = producer.is_some_and(|session| {
                session_inputs
                    .get(session.session_id.trim())
                    .is_some_and(|ids| ids.as_ref().is_ok_and(BTreeSet::is_empty))
                    && (session
                        .payload_json
                        .get("input_progress_batch_id")
                        .is_some_and(|v| v.as_str().is_some())
                        || session
                            .payload_json
                            .get(INPUT_LINEAGE_PAYLOAD_FIELD)
                            .is_some())
            });
            if !proven_root {
                incomplete_ids.insert(id.to_string());
            }
        }
        for input_id in input_ids {
            if input_id == id || !batches_by_id.contains_key(input_id.as_str()) {
                incomplete_ids.insert(id.to_string());
                continue;
            }
            parents
                .entry(id.to_string())
                .or_default()
                .insert(input_id.clone());
            children.entry(input_id).or_default().insert(id.to_string());
        }
    }
    let descendant_ids = expand(
        &BTreeSet::from([scanned.batch_id.trim().to_string()]),
        &children,
    );
    let batch_ids = expand(&descendant_ids, &parents);
    let frontier_ids = descendant_ids
        .iter()
        .filter(|id| {
            children.get(*id).is_none_or(BTreeSet::is_empty)
                && batches_by_id.get(id.as_str()).is_some_and(|batch| {
                    batch.wip_status != OrderProgressBatchWipStatus::Processed
                        || batch.processed_by_apparatus.starts_with("warehouse:")
                })
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    let edges = parents
        .iter()
        .filter(|(child, _)| batch_ids.contains(*child))
        .flat_map(|(child, inputs)| {
            inputs
                .iter()
                .filter(|parent| batch_ids.contains(*parent))
                .map(|parent| ProductionQrLineageEdge {
                    parent_batch_id: parent.clone(),
                    child_batch_id: child.clone(),
                })
        })
        .collect::<Vec<_>>();
    let mut session_ids = BTreeSet::<String>::new();
    let mut complete = batch_ids.is_disjoint(&incomplete_ids);
    for id in &batch_ids {
        let Some(batch) = batches_by_id.get(id.as_str()) else {
            complete = false;
            continue;
        };
        if !batch.session_id.trim().is_empty() {
            session_ids.insert(batch.session_id.trim().to_string());
        }
        if descendant_ids.contains(id) {
            for consumer_id in [&batch.used_by_session_id, &batch.processed_by_session_id] {
                if !consumer_id.trim().is_empty() {
                    if batch.processed_by_apparatus.starts_with("warehouse:")
                        && consumer_id == &batch.processed_by_session_id
                    {
                        continue;
                    }
                    if sessions_by_id.contains_key(consumer_id.trim()) {
                        session_ids.insert(consumer_id.trim().to_string());
                    } else {
                        complete = false;
                    }
                }
            }
            // A consumed QR with no proven output is an unfinished legacy trace,
            // unless the consumption is the explicit warehouse receipt boundary.
            if batch.wip_status == OrderProgressBatchWipStatus::Processed
                && children.get(id).is_none_or(BTreeSet::is_empty)
                && !batch.processed_by_apparatus.starts_with("warehouse:")
            {
                complete = false;
            }
        }
    }
    for (id, inputs) in &session_inputs {
        if inputs
            .as_ref()
            .is_ok_and(|inputs| !inputs.is_disjoint(&descendant_ids))
        {
            session_ids.insert((*id).to_string());
        }
    }
    for id in &session_ids {
        if session_inputs.get(id.as_str()).is_some_and(Result::is_err)
            || batches_by_id.values().any(|batch| {
                batch.session_id.trim() == id && incomplete_ids.contains(batch.batch_id.trim())
            })
        {
            complete = false;
        }
    }
    // Cycles are corrupt provenance; do not choose an arbitrary current batch.
    let mut degrees = batch_ids
        .iter()
        .map(|id| (id.clone(), parents.get(id).map_or(0, BTreeSet::len)))
        .collect::<BTreeMap<_, _>>();
    let mut ready = degrees
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let mut visited = 0;
    while let Some(id) = ready.pop() {
        visited += 1;
        if let Some(outputs) = children.get(&id) {
            for output in outputs {
                if let Some(degree) = degrees.get_mut(output) {
                    *degree -= 1;
                    if *degree == 0 {
                        ready.push(output.clone());
                    }
                }
            }
        }
    }
    complete &= visited == batch_ids.len();
    BatchLineage {
        batch_ids,
        descendant_ids,
        frontier_ids,
        session_ids,
        edges,
        complete,
    }
}

/// Session totals/output lists may span several sibling rolls. Keep occurrence
/// identity and verified input provenance; measured roll quantities live on batches.
pub(super) fn scope_session_payload(session: &mut OrderRunSession, batch_ids: &BTreeSet<String>) {
    let mut payload = serde_json::Map::new();
    for key in [
        "stage_node_id",
        "stage_work_protocol",
        "stage_work_report",
        "started_by",
    ] {
        if let Some(value) = session.payload_json.get(key) {
            payload.insert(key.to_string(), value.clone());
        }
    }
    if session
        .payload_json
        .get("input_progress_batch_id")
        .and_then(|v| v.as_str())
        .is_some_and(|id| batch_ids.contains(id.trim()))
    {
        for key in [
            "input_progress_batch_id",
            "input_progress_qr_payload",
            "input_progress_apparatus",
            "input_wip_source_kind",
        ] {
            if let Some(value) = session.payload_json.get(key) {
                payload.insert(key.to_string(), value.clone());
            }
        }
    }
    if let Ok(mut links) = order_run_input_links_from_payload(&session.payload_json) {
        links.retain(|link| batch_ids.contains(link.input_batch_id.trim()));
        payload.insert(
            INPUT_LINEAGE_PAYLOAD_FIELD.to_string(),
            serde_json::json!(links),
        );
    }
    session.payload_json = serde_json::Value::Object(payload);
}
