use super::*;
use crate::core::gscale::{epc::GscaleEpcGenerator, ports::EpcSource};
use crate::core::production_map::layer_remainder::{
    json_number, layer_remainder_meter, layer_remainder_wip_basis,
};
use crate::core::production_map::{
    LayerRemainderInput, LayerRemainderRawStock, LayerRemainderRecord, LayerRemainderSource,
    OrderProgressBatchWipStatus, OrderRunInputSourceKind, OrderRunInputStatus, OrderRunStatus,
    ProgressBatchInputLink, order_run_input_links_from_payload, write_order_run_input_links,
    write_progress_batch_input_links,
};
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, insert_raw_material_event_tx,
};
use serde_json::{Value, json};

fn database_error(error: sqlx::Error) -> ProductionMapError {
    tracing::error!(%error, "lamination layer remainder storage failed");
    ProductionMapError::StoreFailed
}
use sqlx::{Postgres, Transaction};

#[derive(sqlx::FromRow)]
struct RawRow {
    id: String,
    warehouse: String,
    item_code: String,
    item_name: String,
    barcode: String,
    qty: f64,
    width_mm: Option<f64>,
    micron: Option<f64>,
    length_m: Option<f64>,
    uom: String,
    status: String,
    reserved_order_id: String,
    source_receipt_id: String,
    payload_json: Value,
    revision: i64,
}

async fn raw_row(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<Option<RawRow>, ProductionMapError> {
    sqlx::query_as(
        "SELECT id,warehouse,item_code,item_name,barcode,qty::float8 AS qty,
        width_mm::float8 AS width_mm,micron::float8 AS micron,length_m::float8 AS length_m,
        uom,status,reserved_order_id,source_receipt_id,payload_json,
        (extract(epoch FROM updated_at)*1000000)::bigint AS revision
        FROM mini_raw_material_stock WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)
}

fn eligible_session(s: &OrderRunSession, apparatus: &str) -> bool {
    s.apparatus == apparatus
        && matches!(
            s.status,
            OrderRunStatus::Paused | OrderRunStatus::RollDetached | OrderRunStatus::Completed
        )
}

fn wip_source(
    s: &OrderRunSession,
    b: &OrderProgressBatch,
) -> Result<LayerRemainderSource, ProductionMapError> {
    let linked = order_run_input_links_from_payload(&s.payload_json)
        .map_err(|_| ProductionMapError::StoreFailed)?
        .iter()
        .any(|l| {
            l.source_kind == OrderRunInputSourceKind::ProgressBatch
                && l.input_batch_id == b.batch_id
        })
        || s.payload_json
            .get("input_progress_batch_id")
            .and_then(Value::as_str)
            == Some(b.batch_id.as_str());
    let owned = match b.wip_status {
        OrderProgressBatchWipStatus::InUse => {
            b.used_by_session_id == s.session_id && b.used_by_apparatus == s.apparatus
        }
        OrderProgressBatchWipStatus::Processed => {
            b.processed_by_session_id == s.session_id && b.processed_by_apparatus == s.apparatus
        }
        _ => false,
    };
    if !linked || !owned || b.order_id != s.order_id {
        return Err(ProductionMapError::ProgressBatchCorrectionConflict);
    }
    let (net, meter, tare) = layer_remainder_wip_basis(b).unwrap_or((0.0, 0.0, 0.0));
    Ok(LayerRemainderSource {
        source_kind: "progress_batch".into(),
        source_id: b.batch_id.clone(),
        session_id: s.session_id.clone(),
        revision: b.revision,
        qr_payload: b.qr_payload.clone(),
        item_code: b.label_item_code.clone(),
        item_name: b.label_item_name.clone(),
        source_net_kg: net,
        source_meter: meter,
        tare_kg: tare,
        producer_name: b.executor_name.clone(),
    })
}

async fn raw_source(
    tx: &mut Transaction<'_, Postgres>,
    s: &OrderRunSession,
    r: &RawRow,
) -> Result<LayerRemainderSource, ProductionMapError> {
    if s.status != OrderRunStatus::Completed {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let resources = ProductionQrSessionResources::recorded(s)
        .ok_or(ProductionMapError::RawMaterialStockUnavailable)?;
    if !resources.raw_materials.iter().any(|m| {
        m.stock_id.as_deref() == Some(r.id.as_str()) && m.barcode.eq_ignore_ascii_case(&r.barcode)
    }) || r.reserved_order_id != s.order_id
        || !matches!(r.status.as_str(), "in_use" | "consumed")
        || r.uom != "kg"
        || r.payload_json
            .get("inventory_transfer_id")
            .and_then(Value::as_str)
            .is_some_and(|v| !v.is_empty())
    {
        return Err(ProductionMapError::RawMaterialStockUnavailable);
    }
    let assigned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_raw_material_assignments WHERE lower(barcode)=lower($1) AND order_id=$2 AND canonical_apparatus_id=$3)")
        .bind(&r.barcode).bind(&s.order_id).bind(&s.apparatus).fetch_one(&mut **tx).await.map_err(database_error)?;
    if !assigned {
        return Err(ProductionMapError::RawMaterialStockUnavailable);
    }
    let placed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_inventory_placements p JOIN mini_inventory_locations l ON l.id=p.physical_location_id JOIN mini_factory_location_apparatus_links a ON a.location_id=l.factory_location_id WHERE p.asset_kind='raw_material' AND p.asset_ref=$1 AND l.kind='state' AND l.active AND a.apparatus_id=$2)")
        .bind(&r.id).bind(&s.apparatus).fetch_one(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
    if !placed {
        return Err(ProductionMapError::RawMaterialStockUnavailable);
    }
    let meter = r.length_m.unwrap_or(0.0);
    let tare = r
        .payload_json
        .get("tare_kg")
        .and_then(json_number)
        .unwrap_or(0.0);
    if !tare.is_finite() || tare < 0.0 {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    let producer: Option<String> = sqlx::query_scalar("SELECT owner_display_name FROM mini_raw_material_events WHERE lower(barcode)=lower($1) AND owner_ref<>'' ORDER BY id DESC LIMIT 1")
        .bind(&r.barcode).fetch_optional(&mut **tx).await.map_err(database_error)?;
    Ok(LayerRemainderSource {
        source_kind: "raw_material".into(),
        source_id: r.id.clone(),
        session_id: s.session_id.clone(),
        revision: u64::try_from(r.revision).map_err(|_| ProductionMapError::StoreFailed)?,
        qr_payload: r.barcode.clone(),
        item_code: r.item_code.clone(),
        item_name: r.item_name.clone(),
        source_net_kg: r.qty,
        source_meter: meter,
        tare_kg: tare,
        producer_name: producer.unwrap_or_else(|| {
            r.payload_json
                .pointer("/layer_remainder/producer_name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        }),
    })
}

impl PostgresProductionMapStore {
    pub(super) async fn layer_remainder_sources(
        &self,
        apparatus: &str,
        order_id: &str,
    ) -> Result<Vec<LayerRemainderSource>, ProductionMapError> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        // A snapshot is serialized with the same order/apparatus/source locks
        // as creation; revision witnesses are still rechecked at POST time.
        transaction_locks::lock_order_and_apparatuses_tx(&mut tx, order_id, &[apparatus]).await?;
        let mut sessions = stage_execution::sessions_tx(&mut tx, order_id).await?;
        sessions.sort_by(|a, b| b.started_at_unix.cmp(&a.started_at_unix));
        let mut sources = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for s in sessions.iter().filter(|s| eligible_session(s, apparatus)) {
            let mut ids: Vec<String> = order_run_input_links_from_payload(&s.payload_json)
                .map_err(|_| ProductionMapError::StoreFailed)?
                .into_iter()
                .filter(|l| l.source_kind == OrderRunInputSourceKind::ProgressBatch)
                .map(|l| l.input_batch_id)
                .collect();
            if let Some(id) = s
                .payload_json
                .get("input_progress_batch_id")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
            {
                ids.push(id.to_string());
            }
            for id in ids {
                if !seen.insert(("progress_batch", id.clone())) {
                    continue;
                }
                if !source_already_returned(&mut tx, "progress_batch", &id).await? {
                    if let Some(b) = order_query_helpers::load_progress_batch(&mut *tx, &id).await?
                    {
                        match wip_source(s, &b) {
                            Ok(source) => sources.push(source),
                            Err(ProductionMapError::StoreFailed) => {
                                return Err(ProductionMapError::StoreFailed);
                            }
                            Err(_) => {}
                        }
                    }
                }
            }
            if let Some(resources) = ProductionQrSessionResources::recorded(s) {
                for m in resources.raw_materials {
                    let Some(id) = m.stock_id else {
                        continue;
                    };
                    if !seen.insert(("raw_material", id.clone()))
                        || source_already_returned(&mut tx, "raw_material", &id).await?
                    {
                        continue;
                    }
                    if let Some(r) = raw_row(&mut tx, &id).await? {
                        match raw_source(&mut tx, s, &r).await {
                            Ok(source) => sources.push(source),
                            Err(ProductionMapError::StoreFailed) => {
                                return Err(ProductionMapError::StoreFailed);
                            }
                            Err(_) => {}
                        }
                    }
                }
            }
        }
        tx.commit().await.map_err(database_error)?;
        Ok(sources)
    }

    pub(super) async fn layer_remainder_history(
        &self,
        apparatus: &str,
        order_id: &str,
    ) -> Result<Vec<LayerRemainderRecord>, ProductionMapError> {
        let rows: Vec<Value> = sqlx::query_scalar("SELECT response_json FROM mini_layer_remainder_operations WHERE apparatus=$1 AND order_id=$2 ORDER BY created_at DESC,operation_id DESC LIMIT 100")
            .bind(apparatus).bind(order_id).fetch_all(&self.pool).await.map_err(database_error)?;
        rows.into_iter()
            .map(|v| serde_json::from_value(v).map_err(|_| ProductionMapError::StoreFailed))
            .collect()
    }

    pub(super) async fn layer_remainder_by_id(
        &self,
        id: &str,
    ) -> Result<Option<LayerRemainderRecord>, ProductionMapError> {
        let row: Option<Value> = sqlx::query_scalar(
            "SELECT response_json FROM mini_layer_remainder_operations WHERE operation_id=$1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;
        row.map(|v| serde_json::from_value(v).map_err(|_| ProductionMapError::StoreFailed))
            .transpose()
    }

    pub(super) async fn record_layer_remainder(
        &self,
        input: LayerRemainderInput,
        actor: QueueActionActor,
    ) -> Result<LayerRemainderRecord, ProductionMapError> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        transaction_locks::lock_order_and_apparatuses_tx(
            &mut tx,
            &input.order_id,
            &[&input.apparatus],
        )
        .await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!(
                "layer-remainder:{}:{}",
                actor.ref_, input.request_id
            ))
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        let request = json!(input);
        let replay: Option<(Value,Value)> = sqlx::query_as("SELECT request_json,response_json FROM mini_layer_remainder_operations WHERE recorded_by_ref=$1 AND request_id=$2")
            .bind(&actor.ref_).bind(&input.request_id).fetch_optional(&mut *tx).await.map_err(database_error)?;
        if let Some((old, response)) = replay {
            if old != request {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
            return serde_json::from_value(response).map_err(|_| ProductionMapError::StoreFailed);
        }
        let frozen: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_order_control_states WHERE order_id=$1 AND state IN ('frozen','freeze_requested'))")
            .bind(&input.order_id).fetch_one(&mut *tx).await.map_err(database_error)?;
        if frozen {
            return Err(ProductionMapError::OrderFrozen);
        }
        sqlx::query(
            "SELECT session_id FROM mini_order_run_sessions WHERE session_id=$1 FOR UPDATE",
        )
        .bind(&input.session_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(database_error)?;
        let mut session = stage_execution::sessions_tx(&mut tx, &input.order_id)
            .await?
            .into_iter()
            .find(|s| s.session_id == input.session_id)
            .ok_or(ProductionMapError::OrderNotStarted)?;
        if !eligible_session(&session, &input.apparatus) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        if source_already_returned(&mut tx, &input.source_kind, &input.source_id).await? {
            return Err(ProductionMapError::ProgressBatchCorrectionConflict);
        }
        let operation_id = format!("layer-remainder:{:032x}", rand::random::<u128>());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let (source, progress_batch, raw_material_stock) = if input.source_kind == "progress_batch"
        {
            sqlx::query("SELECT batch_id FROM mini_progress_batches WHERE batch_id=$1 FOR UPDATE")
                .bind(&input.source_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(database_error)?;
            let mut original = order_query_helpers::load_progress_batch(&mut *tx, &input.source_id)
                .await?
                .ok_or(ProductionMapError::ProgressBatchNotFound)?;
            let source = wip_source(&session, &original)?;
            if source.revision != input.expected_revision {
                return Err(ProductionMapError::ProgressBatchCorrectionConflict);
            }
            let (net, meter) = layer_remainder_meter(
                source.source_net_kg,
                source.source_meter,
                input.remaining_net_kg,
            )?;
            let mut child = original.clone();
            child.batch_id = crate::core::production_map::progress_batch_id(
                &original.apparatus,
                &original.order_id,
                original.action,
            );
            child.qr_payload = crate::core::production_map::progress_qr_payload(&child.batch_id);
            child.revision = 1;
            child.parent_batch_id = original.batch_id.clone();
            child.wip_status = OrderProgressBatchWipStatus::Waiting;
            child.current_apparatus = input.apparatus.clone();
            child.current_location = input.apparatus.clone();
            child.next_apparatus = input.apparatus.clone();
            child.used_by_session_id.clear();
            child.used_by_apparatus.clear();
            child.processed_by_session_id.clear();
            child.processed_by_apparatus.clear();
            child.produced_qty = if child.uom == "kg" {
                net + source.tare_kg
            } else {
                meter
            };
            child.finished_goods_kg = Some(net + source.tare_kg);
            child.finished_goods_meter = Some(meter);
            child.payload_json["origin"] = json!("layer_remainder");
            child.payload_json["gross_qty"] = json!(net + source.tare_kg);
            child.payload_json["net_qty"] = json!(net);
            child.payload_json["layer_remainder"] = json!({"operation_id":operation_id,"source_batch_id":original.batch_id,"root_batch_id":original.payload_json.pointer("/layer_remainder/root_batch_id").and_then(Value::as_str).unwrap_or(&original.batch_id),"recorded_by":actor,"session_id":input.session_id,"source_net_kg":source.source_net_kg,"source_meter":source.source_meter,"meter_estimated":true});
            child
                .payload_json
                .as_object_mut()
                .ok_or(ProductionMapError::StoreFailed)?
                .remove("wip_route_binding");
            child
                .payload_json
                .as_object_mut()
                .unwrap()
                .remove("wip_in_use_at_unix");
            child
                .payload_json
                .as_object_mut()
                .unwrap()
                .remove("wip_processed_at_unix");
            write_progress_batch_input_links(
                &mut child.payload_json,
                &[ProgressBatchInputLink {
                    input_batch_id: original.batch_id.clone(),
                    input_qr_payload: original.qr_payload.clone(),
                    source_apparatus: original.apparatus.clone(),
                    source_kind: OrderRunInputSourceKind::ProgressBatch,
                    sequence_no: 1,
                }],
            );
            child.refresh_status_detail();
            original.wip_status = OrderProgressBatchWipStatus::Processed;
            original.used_by_session_id.clear();
            original.used_by_apparatus.clear();
            original.processed_by_session_id = session.session_id.clone();
            original.processed_by_apparatus = session.apparatus.clone();
            original.payload_json["layer_remainder_operation_id"] = json!(operation_id);
            original.payload_json["wip_processed_at_unix"] = json!(now);
            original.refresh_status_detail();
            progress_helpers::put_order_progress_batch_tx(&mut tx, &original).await?;
            progress_helpers::put_order_progress_batch_tx(&mut tx, &child).await?;
            let mut links = order_run_input_links_from_payload(&session.payload_json)
                .map_err(|_| ProductionMapError::StoreFailed)?;
            for link in &mut links {
                if link.input_batch_id == original.batch_id {
                    link.status = OrderRunInputStatus::Processed;
                    link.processed_at_unix = Some(now);
                }
            }
            write_order_run_input_links(&mut session.payload_json, &links);
            if session
                .payload_json
                .get("input_progress_batch_id")
                .and_then(Value::as_str)
                == Some(original.batch_id.as_str())
            {
                session.payload_json["layer_remainder_detached_input_batch_id"] =
                    json!(original.batch_id);
                for field in [
                    "input_progress_batch_id",
                    "input_progress_qr_payload",
                    "input_progress_apparatus",
                ] {
                    session.payload_json[field] = json!("");
                }
            }
            session.updated_at_unix = now;
            progress_helpers::put_order_run_session_tx(&mut tx, &session).await?;
            (source, Some(child), None)
        } else {
            let r = raw_row(&mut tx, &input.source_id)
                .await?
                .ok_or(ProductionMapError::RawMaterialStockUnavailable)?;
            let source = raw_source(&mut tx, &session, &r).await?;
            if source.revision != input.expected_revision {
                return Err(ProductionMapError::RawMaterialStockUnavailable);
            }
            let (net, meter) = layer_remainder_meter(
                source.source_net_kg,
                source.source_meter,
                input.remaining_net_kg,
            )?;
            let barcode = GscaleEpcGenerator::new().next_epc();
            let mut payload = r.payload_json.clone();
            if let Some((role,owner_ref,name)) = sqlx::query_as::<_,(String,String,String)>("SELECT owner_role,owner_ref,owner_display_name FROM mini_raw_material_events WHERE lower(barcode)=lower($1) AND owner_ref<>'' ORDER BY id DESC LIMIT 1")
                .bind(&r.barcode).fetch_optional(&mut *tx).await.map_err(|_|ProductionMapError::StoreFailed)? {
                payload["owner_role"]=json!(role);payload["owner_ref"]=json!(owner_ref);payload["owner_display_name"]=json!(name);
            }
            let object = payload
                .as_object_mut()
                .ok_or(ProductionMapError::StoreFailed)?;
            for field in [
                "consumed_order_id",
                "in_use_order_id",
                "inventory_transfer_id",
                "raw_material_split_id",
            ] {
                object.remove(field);
            }
            payload["origin"] = json!("layer_remainder");
            payload["net_qty"] = json!(net);
            payload["gross_qty"] = json!(net + source.tare_kg);
            payload["tare_kg"] = json!(source.tare_kg);
            payload["length_m"] = json!(meter);
            payload["layer_remainder"] = json!({"operation_id":operation_id,"parent_stock_id":r.id,"root_stock_id":r.payload_json.pointer("/layer_remainder/root_stock_id").and_then(Value::as_str).unwrap_or(&r.id),"recorded_by":actor,"producer_name":source.producer_name,"session_id":input.session_id,"source_net_kg":source.source_net_kg,"source_meter":source.source_meter,"meter_estimated":true});
            let child = LayerRemainderRawStock {
                id: format!("raw:{}", barcode.to_lowercase()),
                warehouse: r.warehouse.clone(),
                item_code: r.item_code.clone(),
                item_name: r.item_name.clone(),
                barcode,
                qty: net,
                width_mm: r.width_mm,
                micron: r.micron,
                length_m: Some(meter),
                uom: r.uom.clone(),
                status: "available".into(),
                source_receipt_id: r.source_receipt_id.clone(),
                payload_json: payload,
            };
            // An in-use source still has its full balance. Post the same full
            // consumption once, then return the residual as a positive move.
            if r.status == "in_use" {
                insert_raw_material_event_tx(
                    &mut tx,
                    raw_event(
                        &r,
                        &child,
                        &input,
                        &actor,
                        &operation_id,
                        "consumption_posted",
                        -r.qty,
                        "in_use",
                        "consumed",
                    ),
                )
                .await
                .map_err(database_error)?;
            }
            sqlx::query("UPDATE mini_raw_material_stock SET status='consumed',payload_json=payload_json || jsonb_build_object('consumed_order_id',$2::text,'layer_remainder_operation_id',$3::text),updated_at=now() WHERE id=$1")
                .bind(&r.id).bind(&input.order_id).bind(&operation_id).execute(&mut *tx).await.map_err(database_error)?;
            sqlx::query("INSERT INTO mini_raw_material_stock (id,warehouse,item_code,item_name,barcode,qty,width_mm,micron,length_m,uom,status,reserved_order_id,source_receipt_id,payload_json) VALUES ($1,$2,$3,$4,$5,($6::float8)::numeric(18,6),($7::float8)::numeric(18,6),($8::float8)::numeric(18,6),($9::float8)::numeric(18,6),$10,'available','',$11,$12)")
                .bind(&child.id).bind(&child.warehouse).bind(&child.item_code).bind(&child.item_name).bind(&child.barcode).bind(child.qty).bind(child.width_mm).bind(child.micron).bind(child.length_m).bind(&child.uom).bind(&child.source_receipt_id).bind(&child.payload_json)
                .execute(&mut *tx).await.map_err(database_error)?;
            insert_raw_material_event_tx(
                &mut tx,
                raw_event(
                    &r,
                    &child,
                    &input,
                    &actor,
                    &operation_id,
                    "layer_remainder_returned",
                    net,
                    "",
                    "available",
                ),
            )
            .await
            .map_err(database_error)?;
            // The physical roll stays at this machine with the same scoped
            // order assignment; the operator can scan it again without a
            // fabricated supplier issue or another warehouse handoff.
            let assignment = sqlx::query("INSERT INTO mini_raw_material_assignments (barcode,order_id,apparatus,canonical_apparatus_id,item_code,item_group,payload_json,updated_at) SELECT $1,order_id,apparatus,canonical_apparatus_id,item_code,item_group,payload_json || jsonb_build_object('barcode',$1::text,'layer_remainder_operation_id',$4::text),now() FROM mini_raw_material_assignments WHERE lower(barcode)=lower($2) AND order_id=$3")
                .bind(&child.barcode).bind(&r.barcode).bind(&input.order_id).bind(&operation_id).execute(&mut *tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
            if assignment.rows_affected() != 1 {
                return Err(ProductionMapError::RawMaterialStockUnavailable);
            }
            let placement = sqlx::query("INSERT INTO mini_inventory_placements (asset_kind,asset_ref,physical_location_id,version,updated_by_role,updated_by_ref,updated_by_name) SELECT 'raw_material',$1,physical_location_id,1,$3,$4,$5 FROM mini_inventory_placements WHERE asset_kind='raw_material' AND asset_ref=$2")
                .bind(&child.id).bind(&r.id).bind(&actor.role).bind(&actor.ref_).bind(&actor.display_name).execute(&mut *tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
            if placement.rows_affected() != 1 {
                return Err(ProductionMapError::RawMaterialStockUnavailable);
            }

            (source, None, Some(child))
        };
        let (net, meter) = layer_remainder_meter(
            source.source_net_kg,
            source.source_meter,
            input.remaining_net_kg,
        )?;
        let qr = progress_batch
            .as_ref()
            .map(|b| b.qr_payload.clone())
            .or_else(|| raw_material_stock.as_ref().map(|s| s.barcode.clone()))
            .ok_or(ProductionMapError::StoreFailed)?;
        let record = LayerRemainderRecord {
            request_id: input.request_id.clone(),
            item_name: source.item_name.clone(),
            operation_id,
            apparatus: input.apparatus.clone(),
            order_id: input.order_id.clone(),
            session_id: input.session_id.clone(),
            source_kind: input.source_kind.clone(),
            source_id: input.source_id.clone(),
            qr_payload: qr,
            source_net_kg: source.source_net_kg,
            source_meter: source.source_meter,
            tare_kg: source.tare_kg,
            producer_name: source.producer_name,
            remaining_net_kg: net,
            remaining_meter: meter,
            recorded_by: actor.clone(),
            created_at_unix: now,
            progress_batch,
            raw_material_stock,
        };
        sqlx::query("INSERT INTO mini_layer_remainder_operations (operation_id,apparatus,order_id,session_id,source_kind,source_id,recorded_by_ref,request_id,request_json,response_json) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(&record.operation_id).bind(&input.apparatus).bind(&input.order_id).bind(&input.session_id).bind(&input.source_kind).bind(&input.source_id).bind(&actor.ref_).bind(&input.request_id).bind(request).bind(json!(record))
            .execute(&mut *tx).await.map_err(database_error)?;
        lifecycle::refresh_production_order_lifecycle_tx(
            &mut tx,
            &input.order_id,
            &actor,
            &record.operation_id,
            "layer_remainder_recorded",
        )
        .await?;
        tx.commit().await.map_err(database_error)?;
        Ok(record)
    }
}

async fn source_already_returned(
    tx: &mut Transaction<'_, Postgres>,
    kind: &str,
    id: &str,
) -> Result<bool, ProductionMapError> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_layer_remainder_operations WHERE source_kind=$1 AND source_id=$2)").bind(kind).bind(id).fetch_one(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)
}

fn raw_event(
    r: &RawRow,
    child: &LayerRemainderRawStock,
    input: &LayerRemainderInput,
    actor: &QueueActionActor,
    id: &str,
    kind: &str,
    delta: f64,
    before: &str,
    after: &str,
) -> RawMaterialEventDraft {
    let returned = kind == "layer_remainder_returned";
    RawMaterialEventDraft {
        idempotency_key: if returned {
            format!("layer-remainder-return:{id}")
        } else {
            format!(
                "consumption_posted:{}:{}:{}",
                r.barcode.to_ascii_uppercase(),
                input.order_id,
                input.apparatus
            )
        },
        event_type: kind.into(),
        warehouse: r.warehouse.clone(),
        barcode: if returned {
            child.barcode.clone()
        } else {
            r.barcode.clone()
        },
        item_code: r.item_code.clone(),
        item_name: r.item_name.clone(),
        qty_delta: delta,
        uom: r.uom.clone(),
        stock_status_before: (!before.is_empty()).then(|| before.to_string()),
        stock_status_after: Some(after.into()),
        order_id: Some(input.order_id.clone()),
        apparatus: Some(input.apparatus.clone()),
        actor_role: actor.role.clone(),
        actor_ref: actor.ref_.clone(),
        actor_display_name: actor.display_name.clone(),
        owner_role: child
            .payload_json
            .get("owner_role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        owner_ref: child
            .payload_json
            .get("owner_ref")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        owner_display_name: child
            .payload_json
            .pointer("/layer_remainder/producer_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        source_type: if returned {
            "layer_remainder"
        } else {
            "consumption"
        }
        .into(),
        source_id: if returned { id } else { &input.order_id }.into(),
        source_line_ref: Some(r.barcode.clone()),
        correlation_id: Some(id.into()),
        payload_json: json!({"parent_stock_id":r.id,"stock_id":child.id,"session_id":input.session_id,"source_receipt_id":r.source_receipt_id,"origin":"layer_remainder"}),
    }
}
