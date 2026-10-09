use std::collections::BTreeMap;

use super::*;

use super::progress::unix_seconds;
use super::service_progress_support::normalize_self_consumed_wip_history;
use super::service_queue_support::*;

#[path = "qr_lineage.rs"]
mod qr_lineage;

impl ProductionMapService {
    pub async fn progress_qr_report(
        &self,
        progress_batch_id: &str,
        qr_payload: &str,
    ) -> Result<ProductionQrReport, ProductionMapError> {
        let mut scanned_batch = self
            .progress_batch_for_qr(progress_batch_id, qr_payload)
            .await?;
        let order_id = scanned_batch.order_id.trim().to_string();
        let (
            order,
            mut progress_batches,
            all_queue_states,
            mut logs_by_order,
            mut corrections,
            mut run_sessions,
            order_status,
        ) = tokio::try_join!(
            self.raw_map(&order_id),
            self.store.progress_batches_for_order(&order_id),
            self.store.apparatus_queue_states(),
            self.store
                .queue_action_logs_for_orders(std::slice::from_ref(&order_id)),
            self.store.progress_batch_corrections_for_order(&order_id),
            self.store.order_run_sessions_for_order(&order_id),
            self.order_status_detail(&order_id),
        )?;
        normalize_self_consumed_wip_history(&mut progress_batches);
        for batch in &mut progress_batches {
            batch.refresh_status_detail();
        }
        if !progress_batches
            .iter()
            .any(|batch| batch.batch_id == scanned_batch.batch_id)
        {
            progress_batches.push(scanned_batch.clone());
        }
        let lineage = qr_lineage::batch_lineage(&scanned_batch, &progress_batches, &run_sessions);
        for batch in &mut progress_batches {
            // The nominal parent can be the last mounted merge input. Only
            // verified per-output parents belong in this read-only projection.
            if !lineage.edges.iter().any(|edge| {
                edge.child_batch_id == batch.batch_id
                    && edge.parent_batch_id == batch.parent_batch_id
            }) {
                batch.parent_batch_id.clear();
            }
        }
        if let Some(batch) = progress_batches
            .iter()
            .find(|batch| batch.batch_id == scanned_batch.batch_id)
        {
            scanned_batch = batch.clone();
        }
        let current_batches = progress_batches
            .iter()
            .filter(|batch| lineage.frontier_ids.contains(batch.batch_id.trim()))
            .cloned()
            .collect::<Vec<_>>();
        let current_batch = if current_batches.len() == 1 {
            current_batches.first().cloned()
        } else {
            None
        };
        let is_stale = scanned_batch.wip_status == OrderProgressBatchWipStatus::Processed
            || lineage.descendant_ids.len() > 1;
        let stale_reason = if !is_stale {
            String::new()
        } else if scanned_batch.wip_status == OrderProgressBatchWipStatus::Processed {
            "processed_by_next_stage".to_string()
        } else {
            "superseded_by_new_qr".to_string()
        };
        progress_batches.retain(|batch| lineage.batch_ids.contains(batch.batch_id.trim()));
        corrections.retain(|entry| lineage.batch_ids.contains(entry.batch_id.trim()));
        run_sessions.retain(|session| lineage.session_ids.contains(session.session_id.trim()));
        let session_resources = run_sessions.iter().map(qr_lineage::session_resources).collect();
        let report_event_ids = run_sessions
            .iter()
            .filter_map(super::stage_execution::work_report)
            .map(|report| report.report_id)
            .collect::<std::collections::BTreeSet<_>>();
        let mut logs = logs_by_order.remove(&order_id).unwrap_or_default();
        logs.retain(|entry| {
            report_event_ids.contains(entry.event_id.trim())
                || entry.transfer.as_ref().is_some_and(|transfer| {
                    lineage
                        .batch_ids
                        .contains(transfer.progress_batch_id.trim())
                })
                || entry.freeze.as_ref().is_some_and(|freeze| {
                    lineage
                        .session_ids
                        .contains(freeze.target_session_id.trim())
                })
        });
        // Queue state is order-level context even on a scoped machine. Per-roll
        // status is carried by the batches/sessions, never inferred from it.
        let mut queue_states = queue_states_for_order(&all_queue_states, &order_id);
        queue_states.retain(|apparatus, _| {
            progress_batches
                .iter()
                .any(|batch| batch.apparatus == *apparatus)
                || run_sessions
                    .iter()
                    .any(|session| session.apparatus == *apparatus)
        });
        let roots = progress_batches
            .iter()
            .filter(|batch| {
                !lineage
                    .edges
                    .iter()
                    .any(|edge| edge.child_batch_id == batch.batch_id)
            })
            .collect::<Vec<_>>();
        let opened_by = if roots.len() == 1 {
            roots
                .first()
                .and_then(|batch| {
                    run_sessions
                        .iter()
                        .find(|session| session.session_id == batch.session_id)
                })
                .and_then(|session| {
                    let actor = serde_json::from_value::<QueueActionActor>(
                        session.payload_json.get("started_by")?.clone(),
                    )
                    .ok()?;
                    Some(ProductionQrOpenedBy {
                        actor_role: actor.role,
                        actor_ref: actor.ref_,
                        actor_display_name: actor.display_name,
                        opened_at_unix: session.started_at_unix,
                    })
                })
        } else {
            None
        };
        for session in &mut run_sessions {
            qr_lineage::scope_session_payload(session, &lineage.batch_ids);
        }
        let active_sessions = run_sessions
            .iter()
            .filter(|session| {
                matches!(
                    session.status,
                    OrderRunStatus::Active | OrderRunStatus::Paused | OrderRunStatus::RollDetached
                )
            })
            .cloned()
            .collect();
        Ok(ProductionQrReport {
            scanned_batch,
            current_batch,
            current_batches,
            history_scope: "batch_lineage".to_string(),
            lineage_complete: lineage.complete,
            lineage_edges: lineage.edges,
            session_resources,
            is_stale,
            stale_reason,
            order,
            order_status,
            queue_states,
            logs,
            corrections,
            progress_batches,
            run_sessions,
            active_sessions,
            opened_by,
        })
    }

    pub async fn receive_finished_goods(
        &self,
        progress_batch_id: &str,
        qr_payload: &str,
        warehouse: &str,
        actor: QueueActionActor,
    ) -> Result<FinishedGoodsReceipt, ProductionMapError> {
        let warehouse = warehouse.trim();
        if warehouse.is_empty() {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        if !matches!(
            actor.role.trim().to_ascii_lowercase().as_str(),
            "werka" | "omborchi"
        ) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let identity = self.progress_batch_for_qr(progress_batch_id, qr_payload).await?;
        let _guard = self.wip_action_guard(&identity.batch_id).await;
        let mut batch = self
            .progress_batch_for_qr(progress_batch_id, qr_payload)
            .await?;
        let order_map = self
            .raw_map(&batch.order_id)
            .await?
            .ok_or(ProductionMapError::MapNotFound)?;
        let stage_node_id = batch
            .payload_json
            .get("stage_node_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        let is_final_stage = if stage_node_id.is_empty() {
            chain::is_final_work_stage_station(&order_map, &batch.apparatus)
        } else {
            chain::is_final_work_stage_node(&order_map, stage_node_id)
        };
        if !batch.is_finished_goods_output()
            || !is_final_stage
            || batch.wip_status != OrderProgressBatchWipStatus::Waiting
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let item_code = order_map.product_code.trim();
        if item_code.is_empty() {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let item_name = if order_map.title.trim().is_empty() {
            batch.label_item_name.trim()
        } else {
            order_map.title.trim()
        };
        let now = unix_seconds();
        let (qty, uom) = finished_goods_qty_uom(&batch)?;
        let stock = finished_goods_stock_entry(
            &batch, warehouse, item_code, item_name, &actor, qty, uom, now,
        );
        mark_finished_goods_batch_received(&mut batch, &stock, warehouse, &actor, now);
        self.store
            .receive_finished_goods_batch(batch.clone(), stock.clone())
            .await?;
        let order_status = self.order_status_detail(&stock.order_id).await?;
        self.notify_live();
        Ok(FinishedGoodsReceipt {
            batch,
            stock,
            order_status,
        })
    }

    pub async fn wip_progress_batches(
        &self,
        query: WipProgressBatchQuery,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        let requested_status = query.status;
        let include_processed = query.include_processed;
        let requested_limit = query.limit;
        let requested_next_apparatus = query.next_apparatus.trim().to_string();
        let mut store_query = query;
        if !include_processed
            && requested_status.is_none_or(|status| status == OrderProgressBatchWipStatus::Waiting)
        {
            store_query.status = None;
            store_query.include_processed = true;
            store_query.limit = 500;
        }
        if !requested_next_apparatus.is_empty() {
            // Alternative topology is resolved with the order map below. Do
            // not make the store guess that a producer's first candidate is
            // the only valid canonical consumer.
            store_query.next_apparatus.clear();
            store_query.limit = 500;
        }
        let load_maps = !requested_next_apparatus.is_empty();
        let load_order_controls = !include_processed;
        let (mut batches, loaded_maps, order_controls) = tokio::try_join!(
            self.store.wip_progress_batches(store_query),
            async {
                if load_maps {
                    self.store.maps().await
                } else {
                    Ok(Vec::new())
                }
            },
            async {
                if load_order_controls {
                    self.store.order_control_states().await
                } else {
                    Ok(BTreeMap::new())
                }
            },
        )?;
        normalize_self_consumed_wip_history(&mut batches);
        let mut maps_by_id = maps_by_order_id(loaded_maps);
        // A detached final-stage roll may predate a map extension. Resolve its
        // missing destination before filtering, without rewriting WIP history.
        if batches.iter().any(progress_batch_needs_location_repair) {
            if !load_maps {
                maps_by_id = maps_by_order_id(self.store.maps().await?);
            }
            repair_wip_progress_batch_locations(&mut batches, &maps_by_id);
        }
        if !requested_next_apparatus.is_empty() {
            batches.retain(|batch| {
                maps_by_id
                    .get(batch.order_id.trim())
                    .is_some_and(|map| {
                        super::wip_route::resolve_wip_input_route(map, batch).is_ok_and(|route|
                            route.consumer_apparatus_ids.iter().any(|id| id == &requested_next_apparatus))
                    })
            });
        }
        if !include_processed {
            batches.retain(|batch| {
                requested_status.map_or(
                    batch.wip_status != OrderProgressBatchWipStatus::Processed,
                    |status| batch.wip_status == status,
                )
            });
            batches.retain(|batch| {
                order_controls
                    .get(batch.order_id.trim())
                    .is_none_or(|control| control.state != OrderControlState::Frozen)
            });
        }
        for batch in &mut batches {
            batch.refresh_status_detail();
        }
        batches.truncate(requested_limit.min(500));
        Ok(batches)
    }
}

fn maps_by_order_id(
    maps: Vec<ProductionMapDefinition>,
) -> BTreeMap<String, ProductionMapDefinition> {
    maps.into_iter()
        .map(|map| (map.id.trim().to_string(), map))
        .collect()
}
