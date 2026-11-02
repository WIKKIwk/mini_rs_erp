use super::*;
use crate::core::quantity::{
    erp_quantity_from_units, erp_quantity_to_units, positive_erp_quantity,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaminationSettings {
    /// Compatibility alias for older clients. It no longer selects a report mode.
    pub layer_remainders_required: bool,
    pub print_wip_remainder_labels: bool,
    pub print_film_remainder_labels: bool,
}

impl Default for LaminationSettings {
    fn default() -> Self {
        Self {
            layer_remainders_required: true,
            print_wip_remainder_labels: true,
            print_film_remainder_labels: true,
        }
    }
}

impl LaminationSettings {
    pub fn prints_remainder(&self, source_kind: &str) -> bool {
        match source_kind {
            "progress_batch" => self.print_wip_remainder_labels,
            "raw_material" => self.print_film_remainder_labels,
            _ => false,
        }
    }

    pub fn apply(&mut self, patch: LaminationLabelSettingsPatch) {
        if let Some(value) = patch.print_wip_remainder_labels {
            self.print_wip_remainder_labels = value;
        }
        if let Some(value) = patch.print_film_remainder_labels {
            self.print_film_remainder_labels = value;
        }
        self.layer_remainders_required =
            self.print_wip_remainder_labels && self.print_film_remainder_labels;
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LaminationLabelSettingsPatch {
    pub print_wip_remainder_labels: Option<bool>,
    pub print_film_remainder_labels: Option<bool>,
}

/// The two existing report fields contain net kilograms, not roll counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaminationRemainderWeights {
    pub wip_net_kg: f64,
    pub film_net_kg: f64,
    pub expected_sources: Vec<LaminationRemainderSourceRevision>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaminationRemainderSourceRevision {
    pub source_kind: String,
    pub source_id: String,
    pub expected_revision: u64,
}

impl LaminationRemainderWeights {
    pub(crate) fn dispositions(
        &self,
        sources: &[LayerRemainderSource],
    ) -> Result<Vec<LaminatsiyaAstatkaLayerInput>, ProductionMapError> {
        let expected = self
            .expected_sources
            .iter()
            .map(|source| {
                (
                    (source.source_kind.as_str(), source.source_id.as_str()),
                    source.expected_revision,
                )
            })
            .collect::<BTreeMap<_, _>>();
        if expected.len() != self.expected_sources.len()
            || sources.len() != expected.len()
            || sources.iter().any(|source| {
                expected.get(&(source.source_kind.as_str(), source.source_id.as_str()))
                    != Some(&source.revision)
            })
        {
            return Err(ProductionMapError::ProgressBatchCorrectionConflict);
        }
        let mut layers = sources
            .iter()
            .map(|source| LaminatsiyaAstatkaLayerInput {
                source_kind: source.source_kind.clone(),
                source_id: source.source_id.clone(),
                expected_revision: source.revision,
                remaining_net_kg: 0.0,
            })
            .collect::<Vec<_>>();
        for (kind, kg) in [
            ("progress_batch", self.wip_net_kg),
            ("raw_material", self.film_net_kg),
        ] {
            if !kg.is_finite() || kg < 0.0 {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
            if kg == 0.0 {
                continue;
            }
            let candidates = sources
                .iter()
                .enumerate()
                .filter(|(_, source)| {
                    source.source_kind == kind
                        && source.source_net_kg > 0.0
                        && source.source_meter > 0.0
                })
                .collect::<Vec<_>>();
            let (index, source) = match candidates.as_slice() {
                [(index, source)] => (*index, *source),
                [] => return Err(ProductionMapError::LaminationRemainderSourceMissing),
                _ => return Err(ProductionMapError::LaminationRemainderSourceAmbiguous),
            };
            layer_remainder_meter(source.source_net_kg, source.source_meter, kg)?;
            layers[index].remaining_net_kg = kg;
        }
        Ok(layers)
    }
}

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
    pub async fn lamination_settings(&self) -> Result<LaminationSettings, ProductionMapError> {
        self.store.lamination_settings().await
    }

    pub async fn update_lamination_settings(
        &self,
        required: bool,
        actor: &QueueActionActor,
    ) -> Result<LaminationSettings, ProductionMapError> {
        self.update_lamination_label_settings(
            LaminationLabelSettingsPatch {
                print_wip_remainder_labels: Some(required),
                print_film_remainder_labels: Some(required),
            },
            actor,
        )
        .await
    }

    pub async fn update_lamination_label_settings(
        &self,
        patch: LaminationLabelSettingsPatch,
        actor: &QueueActionActor,
    ) -> Result<LaminationSettings, ProductionMapError> {
        if !matches!(actor.role.as_str(), "admin" | "brigader") || actor.ref_.trim().is_empty() {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let settings = self.store.update_lamination_settings(patch, actor).await?;
        self.notify_live();
        Ok(settings)
    }

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

    fn source(kind: &str, id: &str) -> LayerRemainderSource {
        LayerRemainderSource {
            source_kind: kind.into(),
            source_id: id.into(),
            session_id: "session".into(),
            revision: 7,
            qr_payload: format!("qr-{id}"),
            item_code: id.into(),
            item_name: id.into(),
            source_net_kg: 50.0,
            source_meter: 1500.0,
            tare_kg: 2.0,
            producer_name: "original owner".into(),
        }
    }

    fn weights(
        sources: &[LayerRemainderSource],
        wip: f64,
        film: f64,
    ) -> LaminationRemainderWeights {
        LaminationRemainderWeights {
            wip_net_kg: wip,
            film_net_kg: film,
            expected_sources: sources
                .iter()
                .map(|s| LaminationRemainderSourceRevision {
                    source_kind: s.source_kind.clone(),
                    source_id: s.source_id.clone(),
                    expected_revision: s.revision,
                })
                .collect(),
        }
    }

    #[test]
    fn two_kg_fields_bind_to_their_original_rolls_and_preserve_zero_answers() {
        let sources = [
            source("raw_material", "film"),
            source("progress_batch", "printed"),
        ];
        let layers = weights(&sources, 20.25, 0.0)
            .dispositions(&sources)
            .unwrap();
        assert_eq!(layers[0].remaining_net_kg, 0.0);
        assert_eq!(layers[1].source_id, "printed");
        assert_eq!(layers[1].remaining_net_kg, 20.25);
        assert_eq!(layers[1].expected_revision, 7);
        assert!(
            weights(&sources, 0.0, 0.0)
                .dispositions(&sources)
                .unwrap()
                .iter()
                .all(|layer| layer.remaining_net_kg == 0.0)
        );
        assert!(weights(&sources, 51.0, 0.0).dispositions(&sources).is_err());
        for kg in [-1.0, f64::NAN, f64::INFINITY, 0.0000001] {
            assert!(weights(&sources, kg, 0.0).dispositions(&sources).is_err());
        }
    }

    #[test]
    fn kg_fields_never_guess_missing_ambiguous_or_changed_sources() {
        let mut sources = vec![source("progress_batch", "printed")];
        assert_eq!(
            weights(&sources, 0.0, 1.0)
                .dispositions(&sources)
                .unwrap_err(),
            ProductionMapError::LaminationRemainderSourceMissing
        );
        let old = weights(&sources, 1.0, 0.0);
        sources[0].revision += 1;
        assert_eq!(
            old.dispositions(&sources).unwrap_err(),
            ProductionMapError::ProgressBatchCorrectionConflict
        );
        sources.push(source("progress_batch", "other"));
        assert_eq!(
            weights(&sources, 1.0, 0.0)
                .dispositions(&sources)
                .unwrap_err(),
            ProductionMapError::LaminationRemainderSourceAmbiguous
        );
        sources.truncate(1);
        sources[0].source_meter = 0.0;
        assert_eq!(
            weights(&sources, 1.0, 0.0)
                .dispositions(&sources)
                .unwrap_err(),
            ProductionMapError::LaminationRemainderSourceMissing
        );
        assert!(weights(&[], 0.0, 0.0).dispositions(&[]).unwrap().is_empty());
    }

    #[test]
    fn label_switches_are_independent_and_only_select_their_source_kind() {
        for wip in [false, true] {
            for film in [false, true] {
                let mut settings = LaminationSettings::default();
                settings.apply(LaminationLabelSettingsPatch {
                    print_wip_remainder_labels: Some(wip),
                    print_film_remainder_labels: None,
                });
                settings.apply(LaminationLabelSettingsPatch {
                    print_wip_remainder_labels: None,
                    print_film_remainder_labels: Some(film),
                });
                assert_eq!(settings.prints_remainder("progress_batch"), wip);
                assert_eq!(settings.prints_remainder("raw_material"), film);
                assert!(!settings.prints_remainder("unknown"));
            }
        }
    }
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
