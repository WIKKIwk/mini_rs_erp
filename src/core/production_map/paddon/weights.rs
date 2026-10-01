use super::{OrderProgressBatch, PaddonSummary};
use crate::core::production_map::ProductionMapError;
use crate::core::quantity::{erp_quantity_from_units, erp_quantity_to_units};
use serde_json::Value;
use std::collections::BTreeSet;

fn units(value: Option<f64>) -> Option<i64> {
    value.filter(|v| *v >= 0.0).and_then(erp_quantity_to_units)
}

fn validate_bobina_units(gross: Option<i64>, bobina: Option<i64>) -> Result<(), ProductionMapError> {
    if gross.zip(bobina).is_some_and(|(gross, bobina)| bobina > gross) {
        return Err(ProductionMapError::BobinaExceedsGross);
    }
    Ok(())
}

pub(crate) fn validate_bobina_weight(
    gross: Option<f64>,
    bobina: Option<f64>,
) -> Result<(), ProductionMapError> {
    validate_bobina_units(units(gross), units(bobina))
}

/// Validate the resulting batch so tare-only corrections keep measured gross.
pub(crate) fn validate_batch_bobina(batch: &OrderProgressBatch) -> Result<(), ProductionMapError> {
    validate_bobina_units(product_weights(batch, None).0, units(batch.bobina_kg))
}

pub(crate) fn kg_from_values(value: &Value) -> Option<f64> {
    value.get("finished_goods_kg").and_then(Value::as_f64)
}

/// Compare the actual kg fields at the same precision as PostgreSQL numeric(18,6).
/// Description, meter and core corrections do not replace explicit measured gross.
pub(crate) fn audited_kg_changed(old: &Value, new: &Value) -> bool {
    units(kg_from_values(old)) != units(kg_from_values(new))
}

pub(crate) fn product_weights(
    batch: &OrderProgressBatch,
    corrected_gross: Option<Option<f64>>,
) -> (Option<i64>, Option<i64>) {
    let gross = match corrected_gross {
        Some(value) => units(value).filter(|v| *v > 0),
        None => {
            if batch
                .payload_json
                .get("gross_qty")
                .is_some_and(|v| !v.is_null())
            {
                units(batch.payload_json.get("gross_qty").and_then(Value::as_f64))
                    .filter(|v| *v > 0)
            } else {
                units(batch.finished_goods_kg).filter(|v| *v > 0)
            }
        }
    };
    let net = gross
        .zip(units(batch.bobina_kg))
        .and_then(|(gross, tare)| gross.checked_sub(tare))
        .filter(|net| *net >= 0);
    (gross, net)
}

pub(crate) fn set_totals<'a>(
    summary: &mut PaddonSummary,
    items: impl IntoIterator<Item = (&'a OrderProgressBatch, Option<Option<f64>>)>,
) {
    let mut seen = BTreeSet::new();
    let (mut gross, mut net) = (Some(0_i64), Some(0_i64));
    for (batch, corrected) in items {
        if !seen.insert(&batch.batch_id) {
            continue;
        }
        let (g, n) = product_weights(batch, corrected);
        gross = gross.zip(g).and_then(|(a, b)| a.checked_add(b));
        net = net.zip(n).and_then(|(a, b)| a.checked_add(b));
    }
    summary.total_gross_kg = gross.map(erp_quantity_from_units);
    summary.total_net_kg = net.map(erp_quantity_from_units);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn batch() -> OrderProgressBatch {
        serde_json::from_value(json!({
            "batch_id":"roll", "session_id":"session", "started_at_unix":0, "completed_at_unix":0,
            "apparatus":"apparatus:default:asset-010", "order_id":"order", "action":"roll_complete",
            "status":"completed", "produced_qty":100.0, "uom":"m", "qr_payload":"QR",
            "label_item_code":"", "label_item_name":"", "executor_name":"", "worker_role":"aparatchi",
            "worker_ref":"worker", "worker_display_name":"Worker", "wip_status":"waiting",
            "finished_goods_kg":10.0, "bobina_kg":0.5, "payload_json":{"gross_qty":12.0}
        })).unwrap()
    }
    fn summary() -> PaddonSummary {
        serde_json::from_value(json!({"id":"p", "code":"00001", "created_at_unix":0,
            "updated_at_unix":0, "item_count":0}))
        .unwrap()
    }
    #[test]
    fn explicit_measured_gross_is_distinct_from_finished_goods() {
        assert_eq!(
            product_weights(&batch(), None),
            (Some(12_000_000), Some(11_500_000))
        );
    }
    #[test]
    fn bobina_validation_uses_stored_precision_and_preserves_unknowns() {
        for (gross, bobina) in [
            (Some(55.0), Some(0.808)),
            (Some(55.0), Some(55.0)),
            (Some(55.0000001), Some(55.0000002)),
            (Some(55.0), None),
            (None, Some(808.0)),
        ] {
            assert!(validate_bobina_weight(gross, bobina).is_ok());
        }
        for (gross, bobina) in [(55.0, 808.0), (55.0, 55.000001)] {
            assert!(matches!(
                validate_bobina_weight(Some(gross), Some(bobina)),
                Err(ProductionMapError::BobinaExceedsGross)
            ));
        }
    }

    #[test]
    fn correction_bobina_validation_uses_resulting_measured_gross() {
        let b = batch(); // measured gross 12, finished_goods_kg 10
        let mut values = b.correction_values();
        values["batch_id"] = json!(b.batch_id);
        values["expected_revision"] = json!(b.revision);
        values["reason"] = json!("Corrected tare");
        let mut input: crate::core::production_map::ProgressBatchCorrectionInput =
            serde_json::from_value(values).unwrap();
        input.bobina_kg = Some(11.0);
        let corrected = b.corrected(&input);
        assert!(validate_batch_bobina(&corrected).is_ok());
        assert_eq!(product_weights(&corrected, None).1, Some(1_000_000));
        input.bobina_kg = Some(12.0);
        assert!(validate_batch_bobina(&b.corrected(&input)).is_ok());
        input.bobina_kg = Some(13.0);
        assert!(matches!(validate_batch_bobina(&b.corrected(&input)),
            Err(ProductionMapError::BobinaExceedsGross)));
        input.bobina_kg = Some(11.0);
        input.finished_goods_kg = Some(9.0);
        assert!(matches!(validate_batch_bobina(&b.corrected(&input)),
            Err(ProductionMapError::BobinaExceedsGross)));
    }
    #[test]
    fn missing_gross_or_core_is_unknown_independently() {
        let mut b = batch();
        b.bobina_kg = None;
        assert_eq!(product_weights(&b, None), (Some(12_000_000), None));
        b.payload_json = json!({});
        b.finished_goods_kg = None;
        assert_eq!(product_weights(&b, None), (None, None));
    }
    #[test]
    fn empty_is_zero_and_duplicate_members_are_counted_once() {
        let mut p = summary();
        set_totals(&mut p, std::iter::empty());
        assert_eq!((p.total_gross_kg, p.total_net_kg), (Some(0.0), Some(0.0)));
        let b = batch();
        set_totals(&mut p, [(&b, None), (&b, None)]);
        assert_eq!((p.total_gross_kg, p.total_net_kg), (Some(12.0), Some(11.5)));
    }
    #[test]
    fn partial_pallet_does_not_report_a_partial_sum_as_total() {
        let known = batch();
        let mut unknown = batch();
        unknown.batch_id = "other".into();
        unknown.payload_json = json!({});
        unknown.finished_goods_kg = None;
        let mut p = summary();
        set_totals(&mut p, [(&known, None), (&unknown, None)]);
        assert_eq!((p.total_gross_kg, p.total_net_kg), (None, None));
    }
    #[test]
    fn six_decimal_sum_has_no_binary_noise() {
        let mut a = batch();
        a.payload_json = json!({"gross_qty":0.1});
        a.bobina_kg = Some(0.000001);
        let mut b = a.clone();
        b.batch_id = "other".into();
        b.payload_json = json!({"gross_qty":0.2});
        let mut p = summary();
        set_totals(&mut p, [(&a, None), (&b, None)]);
        assert_eq!(
            (p.total_gross_kg, p.total_net_kg),
            (Some(0.3), Some(0.299998))
        );
    }
    #[test]
    fn invalid_weights_and_core_larger_than_gross_are_unknown() {
        let mut b = batch();
        b.payload_json = json!({"gross_qty":-1});
        assert_eq!(product_weights(&b, None), (None, None));
        b.payload_json = json!({"gross_qty":0.1});
        assert_eq!(product_weights(&b, None), (Some(100_000), None));
    }
    #[test]
    fn audit_compares_stored_precision_and_only_kg() {
        assert!(!audited_kg_changed(
            &json!({"finished_goods_kg":10.0000001, "bobina_kg":0.1}),
            &json!({"finished_goods_kg":10.0000002, "bobina_kg":0.8})
        ));
        assert!(audited_kg_changed(
            &json!({"finished_goods_kg":10}),
            &json!({"finished_goods_kg":11})
        ));
        assert!(!audited_kg_changed(
            &json!({"produced_qty":10,"uom":"kg"}),
            &json!({"produced_qty":11,"uom":"kg"})
        ));
        assert!(!audited_kg_changed(
            &json!({"produced_qty":10,"uom":"m"}),
            &json!({"produced_qty":11,"uom":"m"})
        ));
    }
    #[test]
    fn actual_kg_correction_synchronizes_gross_and_metadata_preserves_it() {
        let b = batch();
        let mut values = b.correction_values();
        values["batch_id"] = json!(b.batch_id);
        values["expected_revision"] = json!(b.revision);
        values["reason"] = json!("Corrected measurement");
        values["description"] = json!("new note");
        let mut input: crate::core::production_map::ProgressBatchCorrectionInput =
            serde_json::from_value(values).unwrap();
        assert_eq!(b.corrected(&input).payload_json["gross_qty"], json!(12.0));
        input.bobina_kg = Some(0.7);
        input.finished_goods_meter = Some(110.0);
        assert_eq!(b.corrected(&input).payload_json["gross_qty"], json!(12.0));
        input.finished_goods_kg = Some(11.234567);
        let corrected = b.corrected(&input);
        assert_eq!(corrected.payload_json["gross_qty"], json!(11.234567));
        assert_eq!(
            product_weights(&corrected, None),
            (Some(11_234_567), Some(10_534_567))
        );
    }
    #[test]
    fn gross_zero_and_sub_micro_measurements_are_unknown() {
        let mut b = batch();
        for value in [0.0, 0.0000001, -1.0] {
            b.payload_json = json!({"gross_qty":value});
            assert_eq!(product_weights(&b, None), (None, None));
        }
        b.payload_json = json!({});
        b.finished_goods_kg = None;
        b.uom = "kg".into();
        assert_eq!(product_weights(&b, None), (None, None));
    }
    #[test]
    fn removed_correction_is_unknown_and_zero_core_is_known() {
        let mut b = batch();
        b.bobina_kg = Some(0.0);
        assert_eq!(
            product_weights(&b, None),
            (Some(12_000_000), Some(12_000_000))
        );
        assert_eq!(product_weights(&b, Some(None)), (None, None));
    }
    #[test]
    fn overflowing_sums_are_unknown() {
        let batches: Vec<_> = (0..11)
            .map(|i| {
                let mut b = batch();
                b.batch_id = i.to_string();
                b.payload_json = json!({"gross_qty":999_999_999_999.0});
                b.bobina_kg = Some(0.0);
                b
            })
            .collect();
        let mut p = summary();
        set_totals(&mut p, batches.iter().map(|b| (b, None)));
        assert_eq!((p.total_gross_kg, p.total_net_kg), (None, None));
    }
}
