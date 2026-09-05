// A card action records one physical output or issue in the active run. The ordinary
// pause/complete action later closes this cycle and accounts for the whole set.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct RecordedRezkaFrame {
    frame_index: usize,
    batch_id: String,
    qr_payload: String,
    input: RezkaFrameProgressInput,
}

struct RezkaOutputReport {
    cycle_id: String,
    kadr_counts: Vec<usize>,
    saved: Vec<RecordedRezkaFrame>,
    record_index: Option<usize>,
}

impl RezkaOutputReport {
    fn prepare(
        session: &OrderRunSession,
        progress: &mut QueueProgressInput,
        identities: &mut [ProgressOutputIdentity],
    ) -> Result<Self, ProductionMapError> {
        let cycle_id = session
            .payload_json
            .get("rezka_output_cycle")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&session.session_id)
            .to_string();
        let saved: Vec<RecordedRezkaFrame> = serde_json::from_value(
            session
                .payload_json
                .get("rezka_output_report")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        )
        .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
        let record_index = progress.rezka_record_frame_index;
        let kadr_counts: Vec<_> = identities
            .iter()
            .map(|identity| identity.contained_kadr_count.unwrap_or(1))
            .collect();
        if !saved.is_empty()
            && session
                .payload_json
                .get("rezka_recorded_kadr_counts")
                .is_some_and(|counts| counts != &serde_json::json!(kadr_counts))
        {
            return Err(ProductionMapError::RezkaOutputCycleConflict);
        }
        if (!progress.rezka_output_cycle.is_empty() && progress.rezka_output_cycle != cycle_id)
            || ((record_index.is_some() || !saved.is_empty())
                && progress.rezka_output_cycle != cycle_id)
        {
            return Err(ProductionMapError::RezkaOutputCycleConflict);
        }
        if let Some(index) = record_index {
            if index == 0 || index > identities.len() || progress.rezka_frames.len() != 1 {
                return Err(ProductionMapError::RezkaFrameCountMismatch);
            }
            let frame = &mut progress.rezka_frames[0];
            frame.issue_note = frame.issue_note.trim().to_string();
            if (!frame.issue_note.is_empty()
                && [frame.produced_qty, frame.gross_qty, frame.finished_goods_kg,
                    frame.finished_goods_meter, frame.diameter, frame.bobina_kg]
                    .iter().any(Option::is_some))
                || frame.has_explicit_waste()
                || progress.total_waste.is_some()
                || progress.rezka_bosma_waste.is_some()
                || progress.rezka_lamination_waste.is_some()
                || progress.rezka_edge_waste.is_some()
            {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
            if let Some(existing) = saved.iter().find(|slot| slot.frame_index == index) {
                if &existing.input != frame {
                    return Err(ProductionMapError::RezkaOutputCycleConflict);
                }
            }
        } else if !saved.is_empty() {
            if progress.rezka_frames.len() != identities.len() {
                return Err(ProductionMapError::RezkaFrameCountMismatch);
            }
            for slot in &saved {
                let frame = progress
                    .rezka_frames
                    .get_mut(slot.frame_index.saturating_sub(1))
                    .ok_or(ProductionMapError::ProgressInputInvalid)?;
                if frame != &slot.input {
                    return Err(ProductionMapError::RezkaOutputCycleConflict);
                }
                *frame = slot.input.clone();
            }
        }
        if record_index.is_some() || !saved.is_empty() {
            let stamp = cycle_id
                .split(':')
                .nth(1)
                .and_then(|value| value.parse::<u128>().ok())
                .ok_or(ProductionMapError::ProgressInputInvalid)?;
            for (index, identity) in identities.iter_mut().enumerate() {
                identity.batch_id = format!("rezka-output:{stamp}:{cycle_id}:frame:{}", index + 1);
                identity.qr_payload = super::progress::progress_qr_payload(&identity.batch_id);
                if let Some(slot) = saved.iter().find(|slot| slot.frame_index == index + 1) {
                    identity.batch_id = slot.batch_id.clone();
                    identity.qr_payload = slot.qr_payload.clone();
                }
            }
        }
        Ok(Self {
            cycle_id,
            kadr_counts,
            saved,
            record_index,
        })
    }

    fn is_saved(&self, index: usize) -> bool {
        self.saved.iter().any(|slot| slot.frame_index == index + 1)
    }

    fn finish_record(
        &self,
        session: &mut OrderRunSession,
        progress: &QueueProgressInput,
        batches: &[OrderProgressBatch],
    ) -> Result<(), ProductionMapError> {
        let Some(frame_index) = self.record_index else {
            return Ok(());
        };
        let mut saved = self.saved.clone();
        if !self.is_saved(frame_index - 1) {
            let batch = batches.first();
            if batch.is_none() && progress.rezka_frames[0].issue_note.trim().is_empty() {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
            saved.push(RecordedRezkaFrame {
                frame_index,
                batch_id: batch.map(|batch| batch.batch_id.clone()).unwrap_or_default(),
                qr_payload: batch.map(|batch| batch.qr_payload.clone()).unwrap_or_default(),
                input: progress.rezka_frames[0].clone(),
            });
            saved.sort_by_key(|slot| slot.frame_index);
        }
        session.payload_json["rezka_output_cycle"] = serde_json::json!(self.cycle_id);
        session.payload_json["rezka_recorded_kadr_counts"] = serde_json::json!(self.kadr_counts);
        session.payload_json["rezka_output_report"] =
            serde_json::to_value(saved).map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(())
    }
}
