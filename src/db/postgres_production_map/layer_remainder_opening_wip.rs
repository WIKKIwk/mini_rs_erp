use super::*;
use crate::core::production_map::layer_remainder::layer_remainder_opening_basis;
use crate::core::production_map::{
    OpeningWipBatchRecord, OpeningWipBatchStatus, OpeningWipIntakeStatus, OpeningWipQuantityBasis,
    ProductionMapService,
};

async fn source_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
    record: &OpeningWipBatchRecord,
    revision: u64,
) -> Result<LayerRemainderSource, ProductionMapError> {
    let batch = &record.batch;
    let primary = session.payload_json["input_wip_source_kind"] == "opening_wip"
        && session.payload_json["input_progress_batch_id"].as_str() == Some(&batch.batch_id);
    let linked = primary
        || order_run_input_links_from_payload(&session.payload_json)
            .map_err(|_| ProductionMapError::StoreFailed)?
            .iter()
            .any(|link| {
                link.source_kind == OrderRunInputSourceKind::OpeningWip
                    && link.input_batch_id == batch.batch_id
            });
    let owned = match batch.wip_status {
        OpeningWipBatchStatus::InUse => {
            batch.used_by_session_id == session.session_id
                && batch.used_by_apparatus == session.apparatus
        }
        OpeningWipBatchStatus::Processed => {
            batch.processed_by_session_id == session.session_id
                && batch.processed_by_apparatus == session.apparatus
        }
        OpeningWipBatchStatus::Waiting => {
            primary && session.payload_json["roll_removed_from_apparatus"] == true
        }
        OpeningWipBatchStatus::Void => false,
    };
    if !linked
        || !owned
        || batch.order_id != session.order_id
        || record.intake.order_id != session.order_id
        || record.intake.status != OpeningWipIntakeStatus::Confirmed
    {
        return Err(ProductionMapError::ProgressBatchCorrectionConflict);
    }
    let map: Value = sqlx::query_scalar("SELECT map_json FROM mini_production_maps WHERE id=$1")
        .bind(&session.order_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(database_error)?;
    let map: ProductionMapDefinition =
        serde_json::from_value(map).map_err(|_| ProductionMapError::StoreFailed)?;
    if ProductionMapService::opening_wip_target_stage(
        &map,
        &record.intake,
        &session.apparatus,
        &session.stage_node_id,
    )
    .is_none()
    {
        return Err(ProductionMapError::WipRouteChanged);
    }
    if batch.wip_status == OpeningWipBatchStatus::Waiting {
        for other in stage_execution::sessions_tx(tx, &session.order_id).await? {
            if other.session_id == session.session_id
                || !matches!(
                    other.status,
                    OrderRunStatus::Active | OrderRunStatus::Paused | OrderRunStatus::RollDetached
                )
            {
                continue;
            }
            if (other.payload_json["input_wip_source_kind"] == "opening_wip"
                && other.payload_json["input_progress_batch_id"].as_str() == Some(&batch.batch_id))
                || order_run_input_links_from_payload(&other.payload_json)
                    .map_err(|_| ProductionMapError::StoreFailed)?
                    .iter()
                    .any(|link| {
                        link.source_kind == OrderRunInputSourceKind::OpeningWip
                            && link.input_batch_id == batch.batch_id
                            && link.status == OrderRunInputStatus::InUse
                    })
            {
                return Err(ProductionMapError::ProgressBatchCorrectionConflict);
            }
        }
    }
    let (net, meter, tare) = layer_remainder_opening_basis(batch).unwrap_or((0.0, 0.0, 0.0));
    Ok(LayerRemainderSource {
        source_kind: "opening_wip".into(),
        source_id: batch.batch_id.clone(),
        session_id: session.session_id.clone(),
        revision,
        qr_payload: batch.qr_payload.clone(),
        item_code: batch.label_item_code.clone(),
        item_name: batch.label_item_name.clone(),
        source_net_kg: net,
        source_meter: meter,
        tare_kg: tare,
        producer_name: record.intake.actor.display_name.clone(),
    })
}

pub(super) async fn session_sources_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
) -> Result<Vec<LayerRemainderSource>, ProductionMapError> {
    let strict = session.status != OrderRunStatus::Completed;
    let links = order_run_input_links_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let primary = (session.payload_json["input_wip_source_kind"] == "opening_wip")
        .then(|| session.payload_json["input_progress_batch_id"].as_str())
        .flatten()
        .filter(|id| !id.is_empty());
    let mut ids = links
        .iter()
        .filter(|link| {
            link.source_kind == OrderRunInputSourceKind::OpeningWip
                && (link.status == OrderRunInputStatus::InUse || (!strict && primary.is_none()))
        })
        .map(|link| link.input_batch_id.clone())
        .collect::<Vec<_>>();
    if !strict && primary.is_none() {
        ids = links
            .iter()
            .filter(|link| link.source_kind == OrderRunInputSourceKind::OpeningWip)
            .max_by_key(|link| link.sequence_no)
            .map(|link| link.input_batch_id.clone())
            .into_iter()
            .collect();
    }
    if let Some(id) = primary {
        ids.push(id.to_string());
    }
    ids.sort();
    ids.dedup();
    let mut sources = Vec::new();
    for id in ids {
        if source_already_returned(tx, "opening_wip", &id).await? {
            continue;
        }
        let Some((record, revision)) =
            opening_wip_helpers::load_opening_wip_batch_tx(tx, &id).await?
        else {
            if strict {
                return Err(ProductionMapError::ProgressBatchNotFound);
            }
            continue;
        };
        match source_tx(tx, session, &record, revision).await {
            Ok(source) => sources.push(source),
            Err(error) if strict || matches!(error, ProductionMapError::StoreFailed) => {
                return Err(error);
            }
            Err(_) => {}
        }
    }
    Ok(sources)
}

pub(super) async fn record_tx(
    tx: &mut Transaction<'_, Postgres>,
    input: &LayerRemainderInput,
    session: &mut OrderRunSession,
    shift_report: bool,
    now: i64,
) -> Result<(LayerRemainderSource, Option<OpeningWipBatchRecord>), ProductionMapError> {
    let (record, revision) = opening_wip_helpers::load_opening_wip_batch_tx(tx, &input.source_id)
        .await?
        .ok_or(ProductionMapError::ProgressBatchNotFound)?;
    let source = source_tx(tx, session, &record, revision).await?;
    if source.revision != input.expected_revision {
        return Err(ProductionMapError::ProgressBatchCorrectionConflict);
    }
    let (net, meter) = remainder_quantities(&source, input.remaining_net_kg, shift_report)?;
    let remainder = if net > 0.0 {
        let sequence: i32 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence_no),0)+1 FROM mini_opening_wip_batches WHERE intake_id=$1",
        ).bind(&record.intake.intake_id).fetch_one(&mut **tx).await.map_err(database_error)?;
        let mut child = record.batch.clone();
        child.parent_batch_id = record.batch.batch_id.clone();
        child.batch_id = format!("{}:roll-{sequence}", record.intake.intake_id);
        child.qr_payload = crate::core::production_map::progress_qr_payload(&child.batch_id);
        child.sequence_no = sequence;
        child.quantity_basis = OpeningWipQuantityBasis::Estimated;
        child.quantity = Some(meter);
        child.uom = "m".into();
        child.finished_goods_meter = Some(meter);
        child.finished_goods_kg = Some(net + source.tare_kg);
        child.diameter = None;
        child.wip_status = OpeningWipBatchStatus::Waiting;
        child.used_by_session_id.clear();
        child.used_by_apparatus.clear();
        child.processed_by_session_id.clear();
        child.processed_by_apparatus.clear();
        child.created_at_unix = now;
        child.updated_at_unix = now;
        opening_wip_helpers::insert_opening_wip_batch_tx(tx, &child).await?;
        let mut intake = record.intake.clone();
        // The immutable receipt contains the public passport. The private
        // creation fingerprint stays on its original intake, outside this JSON.
        intake.request_fingerprint.clear();
        Some(OpeningWipBatchRecord {
            intake,
            batch: child,
        })
    } else {
        None
    };
    sqlx::query(
        "UPDATE mini_opening_wip_batches SET wip_status='processed',
        used_by_session_id='',used_by_apparatus='',processed_by_session_id=$2,
        processed_by_apparatus=$3,updated_at=now() WHERE batch_id=$1",
    )
    .bind(&record.batch.batch_id)
    .bind(&session.session_id)
    .bind(&session.apparatus)
    .execute(&mut **tx)
    .await
    .map_err(database_error)?;
    let mut links = order_run_input_links_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    for link in &mut links {
        if link.source_kind == OrderRunInputSourceKind::OpeningWip
            && link.input_batch_id == record.batch.batch_id
        {
            link.status = OrderRunInputStatus::Processed;
            link.processed_at_unix = Some(now);
        }
    }
    write_order_run_input_links(&mut session.payload_json, &links);
    if session.payload_json["input_wip_source_kind"] == "opening_wip"
        && session.payload_json["input_progress_batch_id"].as_str() == Some(&record.batch.batch_id)
    {
        session.payload_json["layer_remainder_detached_input_batch_id"] =
            json!(record.batch.batch_id);
        for field in [
            "input_progress_batch_id",
            "input_progress_qr_payload",
            "input_progress_apparatus",
            "input_wip_source_kind",
        ] {
            session.payload_json[field] = json!("");
        }
    }
    session.updated_at_unix = now;
    progress_helpers::put_order_run_session_tx(tx, session).await?;
    Ok((source, remainder))
}
