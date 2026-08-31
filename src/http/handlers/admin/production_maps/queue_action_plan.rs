#[derive(Debug)]
struct QueueActionPreflight {
    freeze_safe_stop_with_issue: bool,
    returned_paint_requested: bool,
}

#[derive(Debug)]
enum QueueActionDecision {
    Execute(QueueProgressInput),
    RequestCompletion {
        note: String,
        zero_metric_codes: Vec<String>,
    },
}

fn validate_queue_action_preflight(
    input: &QueueActionCommand,
    apparatus: &QueueApparatusMetadata,
) -> Result<QueueActionPreflight, AdminError> {
    let freeze_safe_stop = !input.progress.freeze_request_id.trim().is_empty()
        && matches!(
            input.action,
            queue_state::ApparatusQueueAction::Pause
                | queue_state::ApparatusQueueAction::DetachRoll
        );
    let has_output = input.progress.has_reported_output();
    let freeze_safe_stop_with_issue =
        freeze_safe_stop && !has_output && !input.progress.description.trim().is_empty();
    if freeze_safe_stop {
        if !has_output && input.progress.description.trim().is_empty() {
            return Err(bad_request(
                "freeze_safe_stop_output_or_issue_note_required",
            ));
        }
        if has_output
            && !input
                .progress
                .has_complete_freeze_safe_stop_output(apparatus.is_rezka())
        {
            return Err(bad_request("freeze_safe_stop_output_incomplete"));
        }
    }

    let returned_paint_requested = !input.completion.returned_paint_items.is_empty()
        || !input.completion.returned_paint_image_id.trim().is_empty();
    if input.action != queue_state::ApparatusQueueAction::Complete && returned_paint_requested {
        return Err(bad_request("returned_paint_only_on_complete"));
    }
    if input.action == queue_state::ApparatusQueueAction::Complete
        && apparatus.is_pechat()
        && !returned_paint_report_can_close(
            &input.completion.returned_paint_items,
            !input.completion.returned_paint_image_id.trim().is_empty(),
        )
    {
        return Err(bad_request(
            "returned_paint_minimum_three_fields_or_image_only",
        ));
    }

    Ok(QueueActionPreflight {
        freeze_safe_stop_with_issue,
        returned_paint_requested,
    })
}

fn plan_queue_action(
    input: &QueueActionCommand,
    apparatus: &QueueApparatusMetadata,
    return_ink_kg: Option<f64>,
    returned_paint_report_attached: bool,
    freeze_safe_stop_with_issue: bool,
) -> Result<QueueActionDecision, AdminError> {
    let metrics = QueueMetricCoverage::from_command(
        input,
        apparatus,
        return_ink_kg,
        returned_paint_report_attached,
    );
    if metrics.rezka_progress_required(input, apparatus)
        && !input.progress.freeze_with_issue
        && !freeze_safe_stop_with_issue
        && !metrics.has_rezka_frames
        && !metrics.has_rezka_quantities
    {
        return Err(bad_request("rezka_progress_metrics_required"));
    }

    let zero_metric_codes = if metrics.has_rezka_frames {
        Vec::new()
    } else {
        zero_completion_metric_codes(input, return_ink_kg)
    };
    let is_complete = input.action == queue_state::ApparatusQueueAction::Complete;
    if is_complete
        && !zero_metric_codes.is_empty()
        && input.progress.description.trim().is_empty()
    {
        return Err(bad_request("zero_metric_explanation_required"));
    }
    if is_complete
        && (!zero_metric_codes.is_empty() || metrics.missing_output_with_explanation(input))
    {
        return Ok(QueueActionDecision::RequestCompletion {
            note: input.progress.description.clone(),
            zero_metric_codes,
        });
    }

    let mut progress = input.progress.clone();
    progress.return_ink_kg = return_ink_kg;
    progress.returned_paint_report_attached = returned_paint_report_attached;
    Ok(QueueActionDecision::Execute(progress))
}

struct QueueMetricCoverage {
    has_complete_bosma: bool,
    has_complete_laminatsiya: bool,
    has_rezka_quantities: bool,
    has_rezka_frames: bool,
}

impl QueueMetricCoverage {
    fn from_command(
        input: &QueueActionCommand,
        apparatus: &QueueApparatusMetadata,
        return_ink_kg: Option<f64>,
        returned_paint_report_attached: bool,
    ) -> Self {
        Self {
            has_complete_bosma:
                crate::core::production_map::bosma_completion_metrics_are_complete(
                    return_ink_kg.is_some() || returned_paint_report_attached,
                    input.progress.total_waste,
                    input.progress.finished_goods_kg,
                    input.progress.finished_goods_meter,
                ),
            has_complete_laminatsiya:
                crate::core::production_map::laminatsiya_completion_metrics_are_complete(
                    input.progress.lamination_print_leftover_rolls,
                    input.progress.lamination_film_leftover_rolls,
                    input.progress.total_waste,
                    input.progress.finished_goods_kg,
                    input.progress.finished_goods_meter,
                ),
            has_rezka_quantities: apparatus.is_rezka()
                && input.progress.has_rezka_quantity_metrics(),
            has_rezka_frames: apparatus.is_rezka() && !input.progress.rezka_frames.is_empty(),
        }
    }

    fn rezka_progress_required(
        &self,
        input: &QueueActionCommand,
        apparatus: &QueueApparatusMetadata,
    ) -> bool {
        apparatus.is_rezka() && input.action.records_progress_output()
    }

    fn missing_output_with_explanation(&self, input: &QueueActionCommand) -> bool {
        !self.has_complete_bosma
            && !self.has_complete_laminatsiya
            && !self.has_rezka_frames
            && !self.has_rezka_quantities
            && input.progress.gross_qty.is_none()
            && !input.progress.description.trim().is_empty()
    }
}
