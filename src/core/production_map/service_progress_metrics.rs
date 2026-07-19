use super::*;
use crate::core::quantity::positive_erp_quantity;

#[derive(Clone, Copy)]
pub(super) struct ProgressMetrics {
    pub(super) return_ink_kg: Option<f64>,
    pub(super) lamination_print_leftover_rolls: Option<f64>,
    pub(super) lamination_film_leftover_rolls: Option<f64>,
    pub(super) rezka_bosma_waste: Option<f64>,
    pub(super) rezka_lamination_waste: Option<f64>,
    pub(super) rezka_edge_waste: Option<f64>,
    pub(super) total_waste: Option<f64>,
    pub(super) finished_goods_kg: Option<f64>,
    pub(super) finished_goods_meter: Option<f64>,
}

pub(super) fn validated_progress_metrics(
    apparatus: &str,
    action: queue_state::ApparatusQueueAction,
    progress: &QueueProgressInput,
) -> Result<ProgressMetrics, ProductionMapError> {
    let is_complete = action == queue_state::ApparatusQueueAction::Complete;
    let metrics = ProgressMetrics {
        return_ink_kg: if is_complete {
            valid_optional_progress_qty(progress.return_ink_kg)?
        } else {
            None
        },
        lamination_print_leftover_rolls: if is_complete {
            valid_optional_progress_qty(progress.lamination_print_leftover_rolls)?
        } else {
            None
        },
        lamination_film_leftover_rolls: valid_optional_progress_qty(
            progress.lamination_film_leftover_rolls,
        )?,
        rezka_bosma_waste: valid_optional_progress_qty(progress.rezka_bosma_waste)?,
        rezka_lamination_waste: valid_optional_progress_qty(progress.rezka_lamination_waste)?,
        rezka_edge_waste: valid_optional_progress_qty(progress.rezka_edge_waste)?,
        total_waste: valid_optional_progress_qty(progress.total_waste)?,
        finished_goods_kg: valid_optional_progress_qty(progress.finished_goods_kg)?,
        finished_goods_meter: valid_optional_progress_qty(progress.finished_goods_meter)?,
    };
    validate_progress_metrics(
        apparatus,
        action,
        metrics,
        progress.returned_paint_report_attached,
    )?;
    Ok(metrics)
}

fn validate_progress_metrics(
    apparatus: &str,
    action: queue_state::ApparatusQueueAction,
    metrics: ProgressMetrics,
    returned_paint_report_attached: bool,
) -> Result<(), ProductionMapError> {
    let is_complete = action == queue_state::ApparatusQueueAction::Complete;
    if is_complete
        && pechat::pechat_color_count(apparatus).is_some()
        && !(returned_paint_report_attached
            && metrics.total_waste.is_some()
            && metrics.finished_goods_kg.is_some()
            && metrics.finished_goods_meter.is_some())
        && !bosma_completion_metrics_are_complete(
            metrics.return_ink_kg,
            metrics.total_waste,
            metrics.finished_goods_kg,
            metrics.finished_goods_meter,
        )
    {
        return Err(ProductionMapError::BosmaCompletionMetricsRequired);
    }
    if is_complete
        && apparatus::is_laminatsiya_title(apparatus)
        && !laminatsiya_completion_metrics_are_complete(
            metrics.lamination_print_leftover_rolls,
            metrics.lamination_film_leftover_rolls,
            metrics.total_waste,
            metrics.finished_goods_kg,
            metrics.finished_goods_meter,
        )
    {
        return Err(ProductionMapError::LaminatsiyaCompletionMetricsRequired);
    }
    if apparatus::is_rezka_title(apparatus)
        && !rezka_progress_metrics_are_complete(
            metrics.rezka_bosma_waste,
            metrics.rezka_lamination_waste,
            metrics.rezka_edge_waste,
        )
    {
        return Err(ProductionMapError::RezkaProgressMetricsRequired);
    }
    Ok(())
}

fn valid_optional_progress_qty(value: Option<f64>) -> Result<Option<f64>, ProductionMapError> {
    match value {
        Some(value) => positive_erp_quantity(value)
            .map(Some)
            .ok_or(ProductionMapError::ProgressInputInvalid),
        None => Ok(None),
    }
}

fn bosma_completion_metrics_are_complete(
    return_ink_kg: Option<f64>,
    total_waste: Option<f64>,
    finished_goods_kg: Option<f64>,
    finished_goods_meter: Option<f64>,
) -> bool {
    return_ink_kg.is_some()
        && total_waste.is_some()
        && finished_goods_kg.is_some()
        && finished_goods_meter.is_some()
}

fn laminatsiya_completion_metrics_are_complete(
    lamination_print_leftover_rolls: Option<f64>,
    lamination_film_leftover_rolls: Option<f64>,
    total_waste: Option<f64>,
    finished_goods_kg: Option<f64>,
    finished_goods_meter: Option<f64>,
) -> bool {
    (lamination_print_leftover_rolls.is_some() || lamination_film_leftover_rolls.is_some())
        && total_waste.is_some()
        && finished_goods_kg.is_some()
        && finished_goods_meter.is_some()
}

fn rezka_progress_metrics_are_complete(
    rezka_bosma_waste: Option<f64>,
    rezka_lamination_waste: Option<f64>,
    rezka_edge_waste: Option<f64>,
) -> bool {
    rezka_bosma_waste.is_some() && rezka_lamination_waste.is_some() && rezka_edge_waste.is_some()
}
