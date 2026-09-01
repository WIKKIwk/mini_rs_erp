#[cfg(test)]
mod apparatus_identity_tests {
    use super::{
        apparatus_ids_match, canonical_apparatus_id, canonical_apparatus_key, stage_ids_match,
    };

    #[test]
    fn progress_identity_requires_canonical_ids_and_ignores_display_titles() {
        assert!(apparatus_ids_match(
            "apparatus:catalog:press-001",
            "apparatus:catalog:press-001"
        ));
        assert!(!apparatus_ids_match(
            "apparatus:catalog:press-001",
            "apparatus:catalog:press-002"
        ));
        assert!(!apparatus_ids_match(
            "apparatus:press-001",
            "apparatus:catalog:press-001"
        ));
        assert!(!apparatus_ids_match(
            "8 ta rangli pechat",
            "apparatus:catalog:press-001"
        ));
        assert!(!apparatus_ids_match(
            "task:lamination-1",
            "task:lamination-1"
        ));
        assert!(stage_ids_match("task:lamination-1", "task:lamination-1"));
        assert!(!stage_ids_match("task:", "task:"));
        assert!(!stage_ids_match("Laminatsiya", "laminatsiya"));
        assert!(!stage_ids_match("laminatsiya", "laminatsiya"));
    }

    #[test]
    fn progress_key_preserves_id_across_display_rename() {
        let id = canonical_apparatus_id("apparatus:catalog:press-001").unwrap();
        assert_eq!(
            canonical_apparatus_key(id.as_str()),
            "apparatus:catalog:press-001"
        );
        assert_eq!(canonical_apparatus_key("8 ta rangli pechat"), "");
    }

    #[test]
    fn progress_qr_remains_owned_by_the_batch_identity() {
        let batch_id = "progress-batch:123:apparatus:catalog:press-001:order-7";
        let renamed_display_batch_id = batch_id;
        assert_eq!(
            crate::core::production_map::progress_qr_payload(batch_id),
            crate::core::production_map::progress_qr_payload(renamed_display_batch_id)
        );
        assert_ne!(
            crate::core::production_map::progress_qr_payload(batch_id),
            crate::core::production_map::progress_qr_payload(
                "progress-batch:123:apparatus:catalog:press-002:order-7"
            )
        );
    }
}

/// Per-frame Rezka measurements supplied with a queue progress action.
///
/// The field names intentionally match the existing queue-action contract so a
/// mobile client can move the same measurements into a per-frame array without
/// introducing a second vocabulary.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RezkaFrameProgressInput {
    #[serde(default)]
    pub produced_qty: Option<f64>,
    #[serde(default)]
    pub gross_qty: Option<f64>,
    #[serde(default)]
    pub finished_goods_kg: Option<f64>,
    #[serde(default)]
    pub finished_goods_meter: Option<f64>,
    #[serde(default)]
    pub diameter: Option<f64>,
    #[serde(default)]
    pub bobina_kg: Option<f64>,
    #[serde(default)]
    pub rezka_bosma_waste: Option<f64>,
    #[serde(default)]
    pub rezka_lamination_waste: Option<f64>,
    #[serde(default)]
    pub rezka_edge_waste: Option<f64>,
    #[serde(default)]
    pub total_waste: Option<f64>,
    /// A frame may be completed as an issue without producing a QR/WIP
    /// output. This is accepted by Rezka roll progress actions.
    #[serde(default)]
    pub issue_note: String,
}

impl RezkaFrameProgressInput {
    pub fn to_queue_progress(
        &self,
        base: &QueueProgressInput,
        inherit_global_waste: bool,
    ) -> QueueProgressInput {
        QueueProgressInput {
            freeze_request_id: base.freeze_request_id.clone(),
            freeze_with_issue: base.freeze_with_issue,
            rezka_frames: Vec::new(),
            produced_qty: self.produced_qty,
            gross_qty: self.gross_qty,
            uom: if self.produced_qty.is_some() || self.finished_goods_meter.is_some() {
                "m".to_string()
            } else {
                base.uom.clone()
            },
            progress_batch_id: base.progress_batch_id.clone(),
            qr_payload: base.qr_payload.clone(),
            return_ink_kg: base.return_ink_kg,
            lamination_print_leftover_rolls: base.lamination_print_leftover_rolls,
            lamination_film_leftover_rolls: base.lamination_film_leftover_rolls,
            rezka_bosma_waste: self.rezka_bosma_waste.or_else(|| {
                inherit_global_waste
                    .then_some(base.rezka_bosma_waste)
                    .flatten()
            }),
            rezka_lamination_waste: self.rezka_lamination_waste.or_else(|| {
                inherit_global_waste
                    .then_some(base.rezka_lamination_waste)
                    .flatten()
            }),
            rezka_edge_waste: self.rezka_edge_waste.or_else(|| {
                inherit_global_waste
                    .then_some(base.rezka_edge_waste)
                    .flatten()
            }),
            total_waste: self
                .total_waste
                .or_else(|| inherit_global_waste.then_some(base.total_waste).flatten()),
            finished_goods_kg: self.finished_goods_kg,
            bobina_kg: self.bobina_kg,
            finished_goods_meter: self.finished_goods_meter,
            diameter: self.diameter,
            description: base.description.clone(),
            returned_paint_report_attached: base.returned_paint_report_attached,
            force_full_completion_metrics: base.force_full_completion_metrics,
            allow_partial_station_completion: base.allow_partial_station_completion,
            worker_handoff: base.worker_handoff,
            remove_roll_from_apparatus: base.remove_roll_from_apparatus,
        }
    }

    pub fn has_explicit_waste(&self) -> bool {
        self.rezka_bosma_waste.is_some()
            || self.rezka_lamination_waste.is_some()
            || self.rezka_edge_waste.is_some()
            || self.total_waste.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueueProgressInput {
    pub freeze_request_id: String,
    /// Backward-compatible marker for the legacy pause-plus-issue request.
    /// The queue action is canonicalized to `Freeze` before persistence.
    pub freeze_with_issue: bool,
    pub rezka_frames: Vec<RezkaFrameProgressInput>,
    pub produced_qty: Option<f64>,
    pub gross_qty: Option<f64>,
    pub uom: String,
    pub progress_batch_id: String,
    pub qr_payload: String,
    pub return_ink_kg: Option<f64>,
    pub lamination_print_leftover_rolls: Option<f64>,
    pub lamination_film_leftover_rolls: Option<f64>,
    pub rezka_bosma_waste: Option<f64>,
    pub rezka_lamination_waste: Option<f64>,
    pub rezka_edge_waste: Option<f64>,
    pub total_waste: Option<f64>,
    pub finished_goods_kg: Option<f64>,
    pub bobina_kg: Option<f64>,
    pub finished_goods_meter: Option<f64>,
    pub diameter: Option<f64>,
    pub description: String,
    pub returned_paint_report_attached: bool,
    /// A worker may finish the currently available Laminatsiya or Rezka WIP
    /// while the upstream stage is still producing more WIPs. In that case
    /// only the finished-goods quantities are reported. This flag is computed
    /// by the queue service; clients can force the full accounting form when
    /// they intentionally leave an order for another order.
    pub force_full_completion_metrics: bool,
    pub allow_partial_station_completion: bool,
    /// Laminatsiya worker is leaving the order while the current roll remains
    /// in the apparatus. This is a handoff, not a production pause with a
    /// finished WIP output.
    pub worker_handoff: bool,
    /// The worker is removing the unfinished roll from the apparatus after a
    /// previous worker handed the order off. The roll remains unfinished and
    /// is put back into waiting WIP.
    pub remove_roll_from_apparatus: bool,
}

impl QueueProgressInput {
    pub(crate) fn has_reported_output(&self) -> bool {
        !self.rezka_frames.is_empty()
            || self.produced_qty.is_some()
            || self.gross_qty.is_some()
            || self.return_ink_kg.is_some()
            || self.lamination_print_leftover_rolls.is_some()
            || self.lamination_film_leftover_rolls.is_some()
            || self.rezka_bosma_waste.is_some()
            || self.rezka_lamination_waste.is_some()
            || self.rezka_edge_waste.is_some()
            || self.total_waste.is_some()
            || self.finished_goods_kg.is_some()
            || self.bobina_kg.is_some()
            || self.finished_goods_meter.is_some()
            || self.diameter.is_some()
    }

    pub(crate) fn has_rezka_quantity_metrics(&self) -> bool {
        let is_positive =
            |value: Option<f64>| value.is_some_and(|value| value.is_finite() && value > 0.0);
        is_positive(self.produced_qty.or(self.finished_goods_meter))
            && is_positive(self.gross_qty.or(self.finished_goods_kg))
            && is_positive(self.diameter)
    }

    pub(crate) fn has_complete_freeze_safe_stop_output(&self, is_rezka: bool) -> bool {
        if is_rezka {
            return !self.rezka_frames.is_empty()
                || (self.has_rezka_quantity_metrics() && self.bobina_kg.is_some());
        }
        self.produced_qty.or(self.finished_goods_meter).is_some()
            && self.gross_qty.or(self.finished_goods_kg).is_some()
            && self.bobina_kg.is_some()
    }
}
