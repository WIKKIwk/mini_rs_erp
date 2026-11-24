pub(super) async fn session_sources_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
) -> Result<Vec<LayerRemainderSource>, ProductionMapError> {
    let strict = session.status != OrderRunStatus::Completed;
    let links = order_run_input_links_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let primary = session
        .payload_json
        .get("input_progress_batch_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let mut ids: Vec<String> = if strict {
        links
            .iter()
            .filter(|link| {
                link.source_kind == OrderRunInputSourceKind::ProgressBatch
                    && link.status == OrderRunInputStatus::InUse
            })
            .map(|link| link.input_batch_id.clone())
            .collect()
    } else {
        primary
            .map(str::to_string)
            .into_iter()
            .chain(
                primary
                    .is_none()
                    .then(|| {
                        links
                            .iter()
                            .filter(|link| {
                                link.source_kind == OrderRunInputSourceKind::ProgressBatch
                            })
                            .max_by_key(|link| link.sequence_no)
                            .map(|link| link.input_batch_id.clone())
                    })
                    .flatten(),
            )
            .collect()
    };
    if session
        .payload_json
        .get("input_wip_source_kind")
        .and_then(Value::as_str)
        != Some("opening_wip")
    {
        if let Some(id) = session
            .payload_json
            .get("input_progress_batch_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            ids.push(id.to_string());
        }
    }
    ids.sort();
    ids.dedup();
    let mut sources = Vec::new();
    sources.extend(opening::session_sources_tx(tx, session).await?);
    for id in ids {
        if source_already_returned(tx, "progress_batch", &id).await? {
            continue;
        }
        sqlx::query("SELECT batch_id FROM mini_progress_batches WHERE batch_id=$1 FOR UPDATE")
            .bind(&id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(database_error)?;
        let batch = order_query_helpers::load_progress_batch(&mut **tx, &id).await?;
        let Some(batch) = batch else {
            if strict {
                return Err(ProductionMapError::ProgressBatchNotFound);
            }
            continue;
        };
        if strict
            && batch.wip_status != OrderProgressBatchWipStatus::InUse
            && !removed_wip_source_owned(session, &batch)
        {
            return Err(ProductionMapError::ProgressBatchCorrectionConflict);
        }
        validate_removed_wip_source_tx(tx, session, &batch).await?;
        match wip_source(session, &batch) {
            Ok(source) => sources.push(source),
            Err(error) if strict || matches!(error, ProductionMapError::StoreFailed) => {
                return Err(error);
            }
            Err(_) => {}
        }
    }
    let mut raw_ids = mounted_raw_source_ids(tx, session).await?;
    if let Some(resources) = ProductionQrSessionResources::recorded(session) {
        for material in resources.raw_materials {
            let Some(id) = material.stock_id else {
                if strict {
                    return Err(ProductionMapError::RawMaterialStockUnavailable);
                }
                continue;
            };
            raw_ids.push(id);
        }
    }
    raw_ids.sort();
    raw_ids.dedup();
    for id in raw_ids {
        if source_already_returned(tx, "raw_material", &id).await? {
            continue;
        }
        let Some(row) = raw_row(tx, &id).await? else {
            if strict {
                return Err(ProductionMapError::RawMaterialStockUnavailable);
            }
            continue;
        };
        match raw_source(tx, session, &row).await {
            Ok(source) => sources.push(source),
            Err(error) if strict || matches!(error, ProductionMapError::StoreFailed) => {
                return Err(error);
            }
            Err(_) => {}
        }
    }
    Ok(sources)
}
