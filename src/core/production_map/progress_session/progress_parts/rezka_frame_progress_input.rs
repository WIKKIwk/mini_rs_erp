/// Per-frame Rezka measurements supplied with a queue progress action.
///
/// The field names intentionally match the existing queue-action contract so a
/// mobile client can move the same measurements into a per-frame array without
/// introducing a second vocabulary.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub peremotka_reason: String,
}

impl RezkaFrameProgressInput {
    pub fn to_queue_progress(
        &self,
        base: &QueueProgressInput,
        inherit_global_waste: bool,
    ) -> QueueProgressInput {
        QueueProgressInput {
            lamination_layer_dispositions_attached: false,
            complete_without_output: false,
            freeze_request_id: base.freeze_request_id.clone(),
            freeze_with_issue: base.freeze_with_issue,
            rezka_frames: Vec::new(),
            rezka_record_frame_index: None,
            rezka_output_cycle: String::new(),
            rezka_expected_kadr_counts: Vec::new(),
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
    /// Set only by the atomic lamination report after validating every physical layer.
    pub lamination_layer_dispositions_attached: bool,
    /// Close an already recorded Bosma output without manufacturing another roll.
    /// progress_batch_id identifies the detached output shown in the server contract.
    pub complete_without_output: bool,
    pub freeze_request_id: String,
    /// Backward-compatible marker for the legacy pause-plus-issue request.
    /// The queue action is canonicalized to `Freeze` before persistence.
    pub freeze_with_issue: bool,
    pub rezka_frames: Vec<RezkaFrameProgressInput>,
    /// One-based output slot. Records a single roll without closing the output cycle.
    pub rezka_record_frame_index: Option<usize>,
    /// Server-issued cycle identity prevents stale dialogs from creating new rolls.
    pub rezka_output_cycle: String,
    /// Layout the operator used when entering the card.
    pub rezka_expected_kadr_counts: Vec<usize>,
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

pub(crate) const INPUT_LINEAGE_PAYLOAD_FIELD: &str = "input_lineage";
pub(crate) const REZKA_ACTIVE_PARTIAL_ROLLS_PAYLOAD_FIELD: &str =
    "rezka_active_partial_rolls";
pub(crate) const SOURCE_INPUT_LINKS_PAYLOAD_FIELD: &str = "source_input_links";

pub(crate) fn order_run_input_links_from_payload(
    payload: &serde_json::Value,
) -> Result<Vec<OrderRunInputLink>, ()> {
    let Some(value) = payload.get(INPUT_LINEAGE_PAYLOAD_FIELD) else {
        return Ok(Vec::new());
    };
    let links: Vec<OrderRunInputLink> = serde_json::from_value(value.clone()).map_err(|_| ())?;
    let mut batch_ids = std::collections::BTreeSet::new();
    let mut sequence_numbers = std::collections::BTreeSet::new();
    let mut active_count = 0usize;
    for link in &links {
        if !link.is_valid()
            || !batch_ids.insert(link.input_batch_id.trim())
            || !sequence_numbers.insert(link.sequence_no)
        {
            return Err(());
        }
        if link.status == OrderRunInputStatus::InUse {
            active_count += 1;
        }
    }
    if active_count > 1 {
        return Err(());
    }
    Ok(links)
}

pub(crate) fn write_order_run_input_links(
    payload: &mut serde_json::Value,
    links: &[OrderRunInputLink],
) {
    ensure_payload_object(payload);
    payload[INPUT_LINEAGE_PAYLOAD_FIELD] = serde_json::json!(links);
}

pub(crate) fn rezka_active_partial_rolls_from_payload(
    payload: &serde_json::Value,
) -> Result<Vec<RezkaActivePartialRoll>, ()> {
    let Some(value) = payload.get(REZKA_ACTIVE_PARTIAL_ROLLS_PAYLOAD_FIELD) else {
        return Ok(Vec::new());
    };
    let rolls: Vec<RezkaActivePartialRoll> =
        serde_json::from_value(value.clone()).map_err(|_| ())?;
    let mut slots = std::collections::BTreeSet::new();
    for roll in &rolls {
        if !roll.is_valid() || !slots.insert(roll.slot_index) {
            return Err(());
        }
    }
    Ok(rolls)
}

pub(crate) fn write_rezka_active_partial_rolls(
    payload: &mut serde_json::Value,
    rolls: &[RezkaActivePartialRoll],
) {
    ensure_payload_object(payload);
    payload[REZKA_ACTIVE_PARTIAL_ROLLS_PAYLOAD_FIELD] = serde_json::json!(rolls);
}

pub(crate) fn progress_batch_input_links_from_payload(
    payload: &serde_json::Value,
) -> Result<Vec<ProgressBatchInputLink>, ()> {
    let Some(value) = payload.get(SOURCE_INPUT_LINKS_PAYLOAD_FIELD) else {
        return Ok(Vec::new());
    };
    let links: Vec<ProgressBatchInputLink> =
        serde_json::from_value(value.clone()).map_err(|_| ())?;
    let mut batch_ids = std::collections::BTreeSet::new();
    let mut sequence_numbers = std::collections::BTreeSet::new();
    for link in &links {
        if !link.is_valid()
            || !batch_ids.insert(link.input_batch_id.trim())
            || !sequence_numbers.insert(link.sequence_no)
        {
            return Err(());
        }
    }
    Ok(links)
}

pub(crate) fn rezka_merge_state_is_consistent(
    input_links: &[OrderRunInputLink],
    active_rolls: &[RezkaActivePartialRoll],
) -> bool {
    let lineage_batch_ids = input_links
        .iter()
        .map(|link| link.input_batch_id.trim())
        .collect::<std::collections::BTreeSet<_>>();
    let active_input_batch_id = input_links
        .iter()
        .find(|link| link.status == OrderRunInputStatus::InUse)
        .map(|link| link.input_batch_id.trim());

    active_rolls.iter().all(|roll| {
        let sources_exist_in_lineage = roll
            .source_input_batch_ids
            .iter()
            .all(|batch_id| lineage_batch_ids.contains(batch_id.trim()));
        let active_source_is_present = match active_input_batch_id {
            Some(active) => roll
                .source_input_batch_ids
                .iter()
                .any(|source| source.trim() == active),
            None => roll.source_input_batch_ids.is_empty(),
        };
        sources_exist_in_lineage && active_source_is_present
    })
}

pub(crate) fn write_progress_batch_input_links(
    payload: &mut serde_json::Value,
    links: &[ProgressBatchInputLink],
) {
    ensure_payload_object(payload);
    payload[SOURCE_INPUT_LINKS_PAYLOAD_FIELD] = serde_json::json!(links);
}

fn ensure_payload_object(payload: &mut serde_json::Value) {
    if !payload.is_object() {
        *payload = serde_json::json!({});
    }
}
