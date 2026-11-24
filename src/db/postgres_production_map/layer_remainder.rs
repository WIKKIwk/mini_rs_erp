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

#[path = "layer_remainder_opening_wip.rs"]
mod opening;

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
            OrderRunStatus::Active
                | OrderRunStatus::Paused
                | OrderRunStatus::RollDetached
                | OrderRunStatus::Completed
        )
}

fn removed_wip_source_owned(session: &OrderRunSession, batch: &OrderProgressBatch) -> bool {
    session.status == OrderRunStatus::RollDetached
        && batch.wip_status == OrderProgressBatchWipStatus::Waiting
        && session
            .payload_json
            .get("roll_removed_from_apparatus")
            .and_then(Value::as_bool)
            == Some(true)
        && batch
            .payload_json
            .get("roll_removed_from_apparatus")
            .and_then(Value::as_bool)
            == Some(true)
        && session
            .payload_json
            .get("input_progress_batch_id")
            .and_then(Value::as_str)
            == Some(batch.batch_id.as_str())
        && batch.current_apparatus == session.apparatus
        && batch.used_by_session_id.is_empty()
        && batch.used_by_apparatus.is_empty()
        && batch.processed_by_session_id.is_empty()
        && batch.processed_by_apparatus.is_empty()
}

async fn validate_removed_wip_source_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
    batch: &OrderProgressBatch,
) -> Result<(), ProductionMapError> {
    if batch.wip_status != OrderProgressBatchWipStatus::Waiting {
        return Ok(());
    }
    if !removed_wip_source_owned(session, batch) {
        return Err(ProductionMapError::ProgressBatchCorrectionConflict);
    }
    let map: Value = sqlx::query_scalar("SELECT map_json FROM mini_production_maps WHERE id=$1")
        .bind(&session.order_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(database_error)?;
    let map: ProductionMapDefinition =
        serde_json::from_value(map).map_err(|_| ProductionMapError::StoreFailed)?;
    let route = crate::core::production_map::wip_route::resolve_wip_input_route(&map, batch)?;
    if !route
        .consumer_apparatus_ids
        .iter()
        .any(|id| id == &session.apparatus)
        || !crate::core::production_map::chain::stage_node_ids_match_for_map(
            &map,
            &route.stage_node_id,
            &session.stage_node_id,
        )
    {
        return Err(ProductionMapError::WipRouteChanged);
    }
    // The order lock also serializes claims. Refuse a stale removed-roll
    // pointer when a different execution still records that physical input.
    for other in stage_execution::sessions_tx(tx, &session.order_id).await? {
        if other.session_id == session.session_id
            || !matches!(
                other.status,
                OrderRunStatus::Active | OrderRunStatus::Paused | OrderRunStatus::RollDetached
            )
        {
            continue;
        }
        if other
            .payload_json
            .get("input_progress_batch_id")
            .and_then(Value::as_str)
            == Some(batch.batch_id.as_str())
            || order_run_input_links_from_payload(&other.payload_json)
                .map_err(|_| ProductionMapError::StoreFailed)?
                .iter()
                .any(|link| {
                    link.input_batch_id == batch.batch_id
                        && link.status == OrderRunInputStatus::InUse
                })
        {
            return Err(ProductionMapError::ProgressBatchCorrectionConflict);
        }
    }
    Ok(())
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
        OrderProgressBatchWipStatus::Waiting => removed_wip_source_owned(s, b),
        OrderProgressBatchWipStatus::Peremotka => false,
    };
    if !linked || !owned || b.order_id != s.order_id {
        return Err(ProductionMapError::ProgressBatchCorrectionConflict);
    }
    let (net, meter, tare) = layer_remainder_wip_basis(b).unwrap_or((0.0, 0.0, 0.0));
    Ok(LayerRemainderSource {
        source_kind: "progress_batch".into(),
        source_id: b.batch_id.clone(),
        is_current_roll: s.payload_json["input_wip_source_kind"] != "opening_wip"
            && s.payload_json["input_progress_batch_id"].as_str() == Some(&b.batch_id),
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

/// Older executions may omit start_resources after an additional intake or a
/// new WIP Start. Recover only audited stock still mounted at this apparatus,
/// and only for its latest execution; available and historical stocks are not inputs.
async fn mounted_raw_source_ids(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
) -> Result<Vec<String>, ProductionMapError> {
    let latest: Option<String> = sqlx::query_scalar(
        "SELECT session_id FROM mini_order_run_sessions
         WHERE order_id=$1 AND canonical_apparatus_id=$2
         ORDER BY started_at DESC,session_id DESC LIMIT 1",
    )
    .bind(&session.order_id)
    .bind(&session.apparatus)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?;
    if latest.as_deref() != Some(session.session_id.as_str()) {
        return Ok(Vec::new());
    }
    sqlx::query_scalar(
        "SELECT DISTINCT s.id FROM mini_raw_material_stock s
         JOIN mini_raw_material_assignments r ON lower(r.barcode)=lower(s.barcode)
         JOIN mini_inventory_placements p ON p.asset_kind='raw_material' AND p.asset_ref=s.id
         JOIN mini_inventory_locations l ON l.id=p.physical_location_id
         JOIN mini_factory_location_apparatus_links a ON a.location_id=l.factory_location_id
         WHERE r.order_id=$1 AND r.canonical_apparatus_id=$2
           AND s.reserved_order_id=$1 AND s.status='in_use' AND s.uom='kg'
           AND l.kind='state' AND l.active AND a.apparatus_id=$2
           AND EXISTS(SELECT 1 FROM mini_raw_material_events e
             WHERE lower(e.barcode)=lower(s.barcode) AND e.order_id=$1
               AND e.canonical_apparatus_id=$2 AND e.event_type='usage_started')
         ORDER BY s.id",
    )
    .bind(&session.order_id)
    .bind(&session.apparatus)
    .fetch_all(&mut **tx)
    .await
    .map_err(database_error)
}

async fn raw_source(
    tx: &mut Transaction<'_, Postgres>,
    s: &OrderRunSession,
    r: &RawRow,
) -> Result<LayerRemainderSource, ProductionMapError> {
    let recorded = ProductionQrSessionResources::recorded(s).is_some_and(|resources| {
        resources.raw_materials.iter().any(|m| {
            m.stock_id.as_deref() == Some(r.id.as_str()) && m.barcode.eq_ignore_ascii_case(&r.barcode)
        })
    });
    if !recorded && !mounted_raw_source_ids(tx, s).await?.contains(&r.id) {
        return Err(ProductionMapError::RawMaterialStockUnavailable);
    }
    if r.reserved_order_id != s.order_id
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
    let is_current_roll: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM mini_raw_material_assignments
         WHERE lower(barcode)=lower($1) AND order_id=$2 AND canonical_apparatus_id=$3
           AND payload_json->>'lamination_input_stock_id'=$4)",
    )
    .bind(&r.barcode).bind(&s.order_id).bind(&s.apparatus).bind(&r.id)
    .fetch_one(&mut **tx).await.map_err(database_error)?;
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
        is_current_roll,
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
        sessions.sort_by(|a, b| {
            (b.started_at_unix, &b.session_id).cmp(&(a.started_at_unix, &a.session_id))
        });
        // Astatka belongs to the current machine execution. Old sessions are
        // audit history and cannot supply stock to this submission.
        let sources = match sessions
            .iter()
            .find(|session| session.apparatus == apparatus)
            .filter(|session| eligible_session(session, apparatus))
        {
            Some(session) => session_sources_tx(&mut tx, session).await?,
            None => Vec::new(),
        };
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
        let record =
            record_layer_remainder_tx(&mut tx, input, actor.clone(), &mut session, false).await?;
        lifecycle::refresh_production_order_lifecycle_tx(
            &mut tx,
            &record.order_id,
            &actor,
            &record.operation_id,
            "layer_remainder_recorded",
        )
        .await?;
        tx.commit().await.map_err(database_error)?;
        Ok(record)
    }
}

pub(super) async fn source_already_returned(
    tx: &mut Transaction<'_, Postgres>,
    kind: &str,
    id: &str,
) -> Result<bool, ProductionMapError> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_layer_remainder_operations WHERE source_kind=$1 AND source_id=$2)").bind(kind).bind(id).fetch_one(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)
}

include!("layer_remainder_parts/raw_event.rs");

include!("layer_remainder_parts/session_sources_tx.rs");
