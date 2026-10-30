use super::*;

/// Legacy cycles have no explicit reference: retain their first healthy slot.
/// New cycles persist the first physically recorded healthy slot, independently
/// of the report's display order.
pub(crate) fn rezka_reference_frame_index(payload: &serde_json::Value) -> Option<usize> {
    let frames = payload.get("rezka_output_report")?.as_array()?;
    let healthy = frames
        .iter()
        .filter(|slot| {
            slot.get("batch_id")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|id| !id.trim().is_empty())
                && slot
                    .pointer("/input/issue_note")
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(|note| note.trim().is_empty())
        })
        .filter_map(|slot| {
            slot.get("frame_index")?
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index > 0)
        })
        .collect::<Vec<_>>();
    payload
        .get("rezka_reference_frame_index")
        .and_then(serde_json::Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
        .filter(|index| healthy.contains(index))
        .or_else(|| healthy.into_iter().min())
}

/// Apply an authorized WIP correction to its still-open card cycle. Slot and
/// QR identities remain immutable; only the recorded measurements change.
pub(crate) fn correct_recorded_rezka_frame(
    payload: &mut serde_json::Value,
    corrected: &OrderProgressBatch,
) -> Result<bool, ProductionMapError> {
    let Some(frames) = payload
        .get("rezka_output_report")
        .and_then(serde_json::Value::as_array)
    else {
        return Ok(false);
    };
    let matches: Vec<_> = frames
        .iter()
        .enumerate()
        .filter(|(_, slot)| {
            slot.get("batch_id").and_then(serde_json::Value::as_str)
                == Some(corrected.batch_id.as_str())
        })
        .map(|(index, _)| index)
        .collect();
    let Some(&index) = matches.first() else {
        return Ok(false);
    };
    if matches.len() != 1
        || frames[index]
            .get("qr_payload")
            .and_then(serde_json::Value::as_str)
            != Some(corrected.qr_payload.as_str())
    {
        return Err(ProductionMapError::ProgressBatchCorrectionConflict);
    }
    let old_input = &frames[index]["input"];
    let frame: RezkaFrameProgressInput = serde_json::from_value(old_input.clone())
        .map_err(|_| ProductionMapError::ProgressBatchCorrectionConflict)?;
    if !frame.issue_note.trim().is_empty() {
        return Err(ProductionMapError::ProgressBatchCorrectionConflict);
    }
    let mut new_input = old_input.clone();
    let meter = corrected.finished_goods_meter.or_else(|| {
        corrected
            .uom
            .trim()
            .eq_ignore_ascii_case("m")
            .then_some(corrected.produced_qty)
    });
    new_input["produced_qty"] = serde_json::json!(meter);
    new_input["gross_qty"] = serde_json::json!(
        corrected
            .payload_json
            .get("gross_qty")
            .and_then(serde_json::Value::as_f64)
            .or(corrected.finished_goods_kg)
    );
    new_input["finished_goods_kg"] = serde_json::json!(corrected.finished_goods_kg);
    new_input["finished_goods_meter"] = serde_json::json!(meter);
    new_input["bobina_kg"] = serde_json::json!(corrected.bobina_kg);
    new_input["diameter"] = serde_json::json!(corrected.diameter);
    if *old_input == new_input {
        return Ok(false);
    }
    let revision = payload
        .get("rezka_output_revision")
        .map_or(Ok(0), |value| {
            value
                .as_u64()
                .ok_or(ProductionMapError::ProgressBatchCorrectionConflict)
        })?
        .checked_add(1)
        .ok_or(ProductionMapError::ProgressBatchCorrectionConflict)?;
    payload["rezka_output_report"][index]["input"] = new_input;
    payload["rezka_output_revision"] = serde_json::json!(revision);
    Ok(true)
}

impl ProductionMapService {
    pub async fn correct_progress_batch(
        &self,
        input: ProgressBatchCorrectionInput,
        actor: &QueueActionActor,
    ) -> Result<OrderProgressBatch, ProductionMapError> {
        validate_progress_batch_correction_input(&input)?;
        let current = self
            .store
            .progress_batch(&input.batch_id)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        if current.is_layer_remainder() {
            return Err(ProductionMapError::ProgressBatchCorrectionLocked);
        }
        if actor.ref_.trim().is_empty() {
            return Err(ProductionMapError::ProgressBatchCorrectionForbidden);
        }
        if current.worker_ref.trim() != actor.ref_.trim() {
            let actor_refs = vec![actor.ref_.trim().to_string()];
            let is_owned = self
                .store
                .progress_batches_for_worker(&actor_refs, "", 500)
                .await?
                .iter()
                .any(|batch| batch.batch_id.trim() == current.batch_id.trim());
            if !is_owned {
                return Err(ProductionMapError::ProgressBatchCorrectionForbidden);
            }
        }
        if current.wip_status != OrderProgressBatchWipStatus::Waiting {
            return Err(ProductionMapError::ProgressBatchCorrectionLocked);
        }
        if current.revision != input.expected_revision {
            return Err(ProductionMapError::ProgressBatchCorrectionConflict);
        }
        crate::core::production_map::paddon_weights::validate_batch_bobina(
            &current.corrected(&input),
        )?;
        if progress_batch_correction_is_unchanged(&current, &input) {
            return Err(ProductionMapError::ProgressBatchCorrectionUnchanged);
        }
        let corrected = self
            .store
            .correct_progress_batch(current, input, actor.clone())
            .await?;
        self.notify_live();
        Ok(corrected)
    }
}

fn validate_progress_batch_correction_input(
    input: &ProgressBatchCorrectionInput,
) -> Result<(), ProductionMapError> {
    if input.batch_id.trim().is_empty()
        || input.expected_revision == 0
        || !input.produced_qty.is_finite()
        || input.produced_qty <= 0.0
        || input.uom.trim().is_empty()
    {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    if input.reason.trim().is_empty() {
        return Err(ProductionMapError::ProgressBatchCorrectionReasonRequired);
    }
    let optional_values = [
        input.return_ink_kg,
        input.lamination_print_leftover_rolls,
        input.lamination_film_leftover_rolls,
        input.rezka_bosma_waste,
        input.rezka_lamination_waste,
        input.rezka_edge_waste,
        input.total_waste,
        input.finished_goods_kg,
        input.bobina_kg,
        input.finished_goods_meter,
        input.diameter,
    ];
    if optional_values
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite() || value <= 0.0)
    {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    Ok(())
}

fn progress_batch_correction_is_unchanged(
    current: &OrderProgressBatch,
    input: &ProgressBatchCorrectionInput,
) -> bool {
    current.correction_values() == current.corrected(input).correction_values()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn corrected_batch() -> OrderProgressBatch {
        serde_json::from_value(json!({
            "batch_id":"roll-1", "session_id":"run-1", "started_at_unix":1,
            "completed_at_unix":2, "apparatus":"apparatus:default:asset-010",
            "order_id":"order-1", "action":"roll_complete", "status":"completed",
            "produced_qty":100.0, "uom":"m", "qr_payload":"qr:roll-1",
            "label_item_code":"product", "label_item_name":"Product", "executor_name":"Worker",
            "worker_role":"aparatchi", "worker_ref":"worker-1", "worker_display_name":"Worker",
            "wip_status":"waiting", "finished_goods_kg":12.6, "finished_goods_meter":100.0,
            "bobina_kg":0.5, "diameter":45.0, "payload_json":{"gross_qty":12.6}
        }))
        .unwrap()
    }

    fn payload() -> serde_json::Value {
        json!({"rezka_output_cycle":"cycle-1", "rezka_output_revision":2,
        "rezka_output_report":[
            {"frame_index":1, "batch_id":"roll-1", "qr_payload":"qr:roll-1",
             "input":{"produced_qty":100.0,"gross_qty":126.0,"finished_goods_kg":126.0,
                "finished_goods_meter":100.0,"bobina_kg":0.5,"diameter":45.0,
                "total_waste":1.5}},
            {"frame_index":2, "batch_id":"", "qr_payload":"", "input":{"issue_note":"No roll"}}
        ]})
    }

    #[test]
    fn correction_refreshes_rezka_measurements_without_replacing_slot_identity_or_waste() {
        let mut value = payload();
        let issue = value["rezka_output_report"][1].clone();
        assert!(correct_recorded_rezka_frame(&mut value, &corrected_batch()).unwrap());
        assert_eq!(value["rezka_output_revision"], 3);
        assert_eq!(value["rezka_output_cycle"], "cycle-1");
        assert_eq!(value["rezka_output_report"][0]["batch_id"], "roll-1");
        assert_eq!(value["rezka_output_report"][0]["qr_payload"], "qr:roll-1");
        assert_eq!(value["rezka_output_report"][0]["frame_index"], 1);
        assert_eq!(value["rezka_output_report"][0]["input"]["gross_qty"], 12.6);
        assert_eq!(value["rezka_output_report"][0]["input"]["total_waste"], 1.5);
        assert_eq!(value["rezka_output_report"][1], issue);
        assert!(!correct_recorded_rezka_frame(&mut value, &corrected_batch()).unwrap());
        assert_eq!(value["rezka_output_revision"], 3);
    }

    #[test]
    fn correction_leaves_unrelated_cycles_and_conflicting_slots_untouched() {
        let mut batch = corrected_batch();
        batch.batch_id = "other-roll".into();
        let original = payload();
        let mut value = original.clone();
        assert!(!correct_recorded_rezka_frame(&mut value, &batch).unwrap());
        assert_eq!(value, original);
        batch.batch_id = "roll-1".into();
        batch.qr_payload = "wrong-qr".into();
        assert!(matches!(
            correct_recorded_rezka_frame(&mut value, &batch),
            Err(ProductionMapError::ProgressBatchCorrectionConflict)
        ));
        assert_eq!(value, original);
    }

    #[test]
    fn recorded_metres_use_the_measured_length_instead_of_non_metre_quantity() {
        let mut batch = corrected_batch();
        batch.uom = "kg".into();
        batch.produced_qty = 12.6;
        let mut value = payload();
        assert!(correct_recorded_rezka_frame(&mut value, &batch).unwrap());
        assert_eq!(
            value["rezka_output_report"][0]["input"]["produced_qty"],
            100.0
        );
        assert_eq!(
            value["rezka_output_report"][0]["input"]["finished_goods_meter"],
            100.0
        );
    }

    #[test]
    fn legacy_reference_skips_issues_and_explicit_reference_keeps_record_order() {
        let mut value = payload();
        assert_eq!(rezka_reference_frame_index(&value), Some(1));
        value["rezka_output_report"][1] = json!({"frame_index":3, "batch_id":"roll-3",
            "qr_payload":"qr:roll-3", "input":{"gross_qty":13.0}});
        value["rezka_reference_frame_index"] = json!(3);
        assert_eq!(rezka_reference_frame_index(&value), Some(3));
        value["rezka_output_report"][0]["input"]["issue_note"] = json!("No roll");
        value
            .as_object_mut()
            .unwrap()
            .remove("rezka_reference_frame_index");
        assert_eq!(rezka_reference_frame_index(&value), Some(3));
    }
}
