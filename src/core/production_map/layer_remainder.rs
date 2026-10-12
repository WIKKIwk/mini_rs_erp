use super::*;
use crate::core::quantity::{
    erp_quantity_from_units, erp_quantity_to_units, positive_erp_quantity,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerRemainderInput {
    pub apparatus: String,
    pub order_id: String,
    pub source_kind: String,
    pub source_id: String,
    pub session_id: String,
    pub expected_revision: u64,
    pub remaining_net_kg: f64,
    pub request_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerRemainderSource {
    pub source_kind: String,
    pub source_id: String,
    pub session_id: String,
    pub revision: u64,
    pub qr_payload: String,
    pub item_code: String,
    pub item_name: String,
    pub source_net_kg: f64,
    pub source_meter: f64,
    pub tare_kg: f64,
    pub producer_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerRemainderRawStock {
    pub id: String,
    pub warehouse: String,
    pub item_code: String,
    pub item_name: String,
    pub barcode: String,
    pub qty: f64,
    pub width_mm: Option<f64>,
    pub micron: Option<f64>,
    pub length_m: Option<f64>,
    pub uom: String,
    pub status: String,
    pub source_receipt_id: String,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerRemainderRecord {
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub item_name: String,
    pub operation_id: String,
    pub apparatus: String,
    pub order_id: String,
    pub session_id: String,
    pub source_kind: String,
    pub source_id: String,
    pub qr_payload: String,
    pub source_net_kg: f64,
    pub source_meter: f64,
    pub tare_kg: f64,
    pub producer_name: String,
    pub remaining_net_kg: f64,
    pub remaining_meter: f64,
    pub recorded_by: QueueActionActor,
    pub created_at_unix: i64,
    pub progress_batch: Option<OrderProgressBatch>,
    pub raw_material_stock: Option<LayerRemainderRawStock>,
}

/// Round once at the inventory boundary, using integer micro-units and an
/// i128 intermediate so the ratio does not accumulate binary float drift.
pub(crate) fn layer_remainder_meter(
    net: f64,
    meter: f64,
    remaining: f64,
) -> Result<(f64, f64), ProductionMapError> {
    let units = |v: f64| {
        erp_quantity_to_units(v)
            .filter(|v| *v > 0)
            .ok_or(ProductionMapError::ProgressInputInvalid)
    };
    let net = units(net)?;
    let meter = units(meter)?;
    let remaining = units(remaining)?;
    if remaining > net {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    let length =
        (i128::from(meter) * i128::from(remaining) + i128::from(net) / 2) / i128::from(net);
    let length = i64::try_from(length).map_err(|_| ProductionMapError::ProgressInputInvalid)?;
    if length <= 0 {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    Ok((
        erp_quantity_from_units(remaining),
        erp_quantity_from_units(length),
    ))
}

pub(crate) fn layer_remainder_wip_basis(
    batch: &OrderProgressBatch,
) -> Result<(f64, f64, f64), ProductionMapError> {
    let tare = batch.bobina_kg.unwrap_or(0.0);
    if !tare.is_finite() || tare < 0.0 {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    let gross = batch
        .payload_json
        .get("gross_qty")
        .and_then(json_number)
        .or(batch.finished_goods_kg)
        .or_else(|| (batch.uom.trim().eq_ignore_ascii_case("kg")).then_some(batch.produced_qty))
        .ok_or(ProductionMapError::ProgressInputInvalid)?;
    let net =
        positive_erp_quantity(gross - tare).ok_or(ProductionMapError::ProgressInputInvalid)?;
    let meter = batch
        .finished_goods_meter
        .or_else(|| {
            matches!(batch.uom.trim(), "m" | "meter" | "metr").then_some(batch.produced_qty)
        })
        .and_then(positive_erp_quantity)
        .ok_or(ProductionMapError::ProgressInputInvalid)?;
    Ok((net, meter, tare))
}

pub(crate) fn json_number(value: &serde_json::Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.parse().ok())
}

impl ProductionMapService {
    async fn validate_layer_remainder_apparatus(
        &self,
        apparatus: &str,
        order_id: &str,
    ) -> Result<(), ProductionMapError> {
        if order_id.trim().is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let id = crate::core::apparatus_standard::ApparatusId::new(apparatus.trim().to_string())
            .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
        let canonical = self.resolve_canonical_apparatus(&id).await?;
        if canonical.runtime.execution_profile.operation
            != crate::core::apparatus_standard::ExecutionOperation::Laminate
            || !super::apparatus::uses_lamination_workflow(&canonical)
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        Ok(())
    }

    pub async fn layer_remainder_sources(
        &self,
        apparatus: &str,
        order_id: &str,
    ) -> Result<Vec<LayerRemainderSource>, ProductionMapError> {
        self.validate_layer_remainder_apparatus(apparatus, order_id)
            .await?;
        self.store
            .layer_remainder_sources(apparatus.trim(), order_id.trim())
            .await
    }

    pub async fn layer_remainder_history(
        &self,
        apparatus: &str,
        order_id: &str,
    ) -> Result<Vec<LayerRemainderRecord>, ProductionMapError> {
        self.validate_layer_remainder_apparatus(apparatus, order_id)
            .await?;
        self.store
            .layer_remainder_history(apparatus.trim(), order_id.trim())
            .await
    }

    pub async fn layer_remainder_by_id(
        &self,
        operation_id: &str,
    ) -> Result<Option<LayerRemainderRecord>, ProductionMapError> {
        self.store.layer_remainder_by_id(operation_id.trim()).await
    }

    pub async fn record_layer_remainder(
        &self,
        input: LayerRemainderInput,
        actor: QueueActionActor,
    ) -> Result<LayerRemainderRecord, ProductionMapError> {
        self.validate_layer_remainder_apparatus(&input.apparatus, &input.order_id)
            .await?;
        if !matches!(
            input.source_kind.as_str(),
            "progress_batch" | "raw_material"
        ) || input.source_id.trim().is_empty()
            || input.session_id.trim().is_empty()
            || !(8..=128).contains(&input.request_id.len())
            || input.expected_revision == 0
            || positive_erp_quantity(input.remaining_net_kg).is_none()
            || actor.ref_.trim().is_empty()
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let _guard = self.queue_action_guard().await;
        let record = self.store.record_layer_remainder(input, actor).await?;
        self.notify_live();
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn net_weight_ratio_and_rounding() {
        assert_eq!(
            layer_remainder_meter(50.0, 1500.0, 20.0).unwrap(),
            (20.0, 600.0)
        );
        assert_eq!(
            layer_remainder_meter(48.0, 1500.0, 18.0).unwrap(),
            (18.0, 562.5)
        );
        assert_eq!(
            layer_remainder_meter(3.0, 10.0, 1.0).unwrap(),
            (1.0, 3.333333)
        );
        assert_eq!(
            layer_remainder_meter(20.0, 600.0, 5.0).unwrap(),
            (5.0, 150.0)
        );
    }
    #[test]
    fn invalid_quantities_never_create_stock() {
        for remaining in [0.0, -1.0, 51.0, f64::NAN, f64::INFINITY, 0.0000001] {
            assert!(layer_remainder_meter(50.0, 1500.0, remaining).is_err());
        }
        assert!(layer_remainder_meter(50.0, 0.0, 20.0).is_err());
    }
}
