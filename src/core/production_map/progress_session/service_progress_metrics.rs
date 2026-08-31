use super::*;
use crate::core::apparatus_standard::RuntimeApparatusConfiguration;
use crate::core::quantity::positive_erp_quantity;

#[derive(Clone, Copy, Default)]
pub(super) struct ProgressMetrics {
    pub(super) return_ink_kg: Option<f64>,
    pub(super) lamination_print_leftover_rolls: Option<f64>,
    pub(super) lamination_film_leftover_rolls: Option<f64>,
    pub(super) rezka_bosma_waste: Option<f64>,
    pub(super) rezka_lamination_waste: Option<f64>,
    pub(super) rezka_edge_waste: Option<f64>,
    pub(super) total_waste: Option<f64>,
    pub(super) finished_goods_kg: Option<f64>,
    pub(super) bobina_kg: Option<f64>,
    pub(super) finished_goods_meter: Option<f64>,
    pub(super) diameter: Option<f64>,
}

pub(super) fn validated_progress_metrics(
    apparatus: &str,
    canonical: &RuntimeApparatusConfiguration,
    action: queue_state::ApparatusQueueAction,
    progress: &QueueProgressInput,
    rezka_total_waste_only_completion: bool,
) -> Result<ProgressMetrics, ProductionMapError> {
    let is_complete = action == queue_state::ApparatusQueueAction::Complete;
    let is_rezka_completion = matches!(
        action,
        queue_state::ApparatusQueueAction::Complete
            | queue_state::ApparatusQueueAction::RollComplete
    );
    let rezka_gross_qty = if apparatus::is_rezka_apparatus(canonical) {
        valid_optional_progress_qty(progress.gross_qty.or(progress.finished_goods_kg))?
    } else {
        None
    };
    let is_rezka = apparatus::is_rezka_apparatus(canonical);
    let is_laminatsiya = apparatus::is_laminatsiya_apparatus(canonical);
    let is_pechat = pechat::is_pechat_apparatus(canonical);
    let allow_partial_station_completion =
        (is_laminatsiya || is_rezka) && is_complete && progress.allow_partial_station_completion;
    let metrics = ProgressMetrics {
        return_ink_kg: if is_complete {
            valid_optional_progress_qty(progress.return_ink_kg)?
        } else {
            None
        },
        lamination_print_leftover_rolls: if is_complete && !allow_partial_station_completion {
            valid_optional_progress_qty(progress.lamination_print_leftover_rolls)?
        } else {
            None
        },
        lamination_film_leftover_rolls: if is_complete && !allow_partial_station_completion {
            valid_optional_progress_qty(progress.lamination_film_leftover_rolls)?
        } else {
            None
        },
        rezka_bosma_waste: if is_rezka_completion
            && !allow_partial_station_completion
            && !rezka_total_waste_only_completion
        {
            valid_optional_progress_qty(progress.rezka_bosma_waste)?
        } else {
            None
        },
        rezka_lamination_waste: if is_rezka_completion
            && !allow_partial_station_completion
            && !rezka_total_waste_only_completion
        {
            valid_optional_progress_qty(progress.rezka_lamination_waste)?
        } else {
            None
        },
        rezka_edge_waste: if is_rezka_completion
            && !allow_partial_station_completion
            && !rezka_total_waste_only_completion
        {
            valid_optional_progress_qty(progress.rezka_edge_waste)?
        } else {
            None
        },
        total_waste: if (is_rezka && !is_rezka_completion)
            || ((is_laminatsiya || is_pechat) && (!is_complete || allow_partial_station_completion))
        {
            None
        } else {
            valid_optional_progress_qty(progress.total_waste)?
        },
        finished_goods_kg: if is_rezka {
            valid_optional_progress_qty(progress.finished_goods_kg.or(progress.gross_qty))?
        } else {
            valid_optional_progress_qty(progress.finished_goods_kg)?
        },
        bobina_kg: valid_optional_progress_qty(progress.bobina_kg)?,
        finished_goods_meter: if is_rezka {
            valid_optional_progress_qty(progress.finished_goods_meter.or(progress.produced_qty))?
        } else {
            valid_optional_progress_qty(progress.finished_goods_meter)?
        },
        diameter: if is_rezka {
            valid_optional_progress_qty(progress.diameter)?
        } else {
            None
        },
    };
    validate_progress_metrics(
        apparatus,
        canonical,
        action,
        progress,
        rezka_gross_qty,
        metrics,
        progress.returned_paint_report_attached,
        allow_partial_station_completion,
        rezka_total_waste_only_completion,
    )?;
    Ok(metrics)
}

pub(super) fn validated_laminatsiya_worker_handoff_metrics(
    _apparatus: &str,
    canonical: &RuntimeApparatusConfiguration,
    progress: &QueueProgressInput,
) -> Result<ProgressMetrics, ProductionMapError> {
    if !apparatus::is_laminatsiya_apparatus(canonical) {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    let metrics = ProgressMetrics {
        return_ink_kg: None,
        lamination_print_leftover_rolls: valid_non_negative_optional_progress_qty(
            progress.lamination_print_leftover_rolls,
        )?,
        lamination_film_leftover_rolls: valid_non_negative_optional_progress_qty(
            progress.lamination_film_leftover_rolls,
        )?,
        rezka_bosma_waste: None,
        rezka_lamination_waste: None,
        rezka_edge_waste: None,
        total_waste: valid_non_negative_optional_progress_qty(progress.total_waste)?,
        finished_goods_kg: None,
        bobina_kg: valid_optional_progress_qty(progress.bobina_kg)?,
        finished_goods_meter: None,
        diameter: None,
    };
    if metrics.lamination_print_leftover_rolls.is_none()
        || metrics.lamination_film_leftover_rolls.is_none()
        || metrics.total_waste.is_none()
    {
        return Err(ProductionMapError::LaminatsiyaCompletionMetricsRequired);
    }
    Ok(metrics)
}

pub(super) fn validated_laminatsiya_removed_roll_metrics(
    _apparatus: &str,
    canonical: &RuntimeApparatusConfiguration,
    progress: &QueueProgressInput,
) -> Result<ProgressMetrics, ProductionMapError> {
    if !apparatus::is_laminatsiya_apparatus(canonical) {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    let finished_goods_meter =
        valid_optional_progress_qty(progress.finished_goods_meter.or(progress.produced_qty))?;
    let finished_goods_kg =
        valid_optional_progress_qty(progress.finished_goods_kg.or(progress.gross_qty))?;
    if finished_goods_meter.is_none() || finished_goods_kg.is_none() {
        return Err(ProductionMapError::LaminatsiyaCompletionMetricsRequired);
    }
    Ok(ProgressMetrics {
        return_ink_kg: None,
        lamination_print_leftover_rolls: None,
        lamination_film_leftover_rolls: None,
        rezka_bosma_waste: None,
        rezka_lamination_waste: None,
        rezka_edge_waste: None,
        total_waste: None,
        finished_goods_kg,
        bobina_kg: valid_optional_progress_qty(progress.bobina_kg)?,
        finished_goods_meter,
        diameter: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_progress_metrics(
    _apparatus: &str,
    canonical: &RuntimeApparatusConfiguration,
    action: queue_state::ApparatusQueueAction,
    progress: &QueueProgressInput,
    rezka_gross_qty: Option<f64>,
    metrics: ProgressMetrics,
    returned_paint_report_attached: bool,
    allow_partial_station_completion: bool,
    rezka_total_waste_only_completion: bool,
) -> Result<(), ProductionMapError> {
    let is_complete = action == queue_state::ApparatusQueueAction::Complete;
    let is_rezka = apparatus::is_rezka_apparatus(canonical);
    if is_complete
        && pechat::is_pechat_apparatus(canonical)
        && !bosma_completion_metrics_are_complete(
            metrics.return_ink_kg.is_some() || returned_paint_report_attached,
            metrics.total_waste,
            metrics.finished_goods_kg,
            metrics.finished_goods_meter,
        )
    {
        return Err(ProductionMapError::BosmaCompletionMetricsRequired);
    }
    if is_complete
        && apparatus::is_laminatsiya_apparatus(canonical)
        && !allow_partial_station_completion
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
    if is_complete
        && apparatus::is_laminatsiya_apparatus(canonical)
        && allow_partial_station_completion
        && (metrics.finished_goods_kg.is_none() || metrics.finished_goods_meter.is_none())
    {
        return Err(ProductionMapError::LaminatsiyaCompletionMetricsRequired);
    }
    let missing_rezka_waste = is_complete
        && if rezka_total_waste_only_completion {
            metrics.total_waste.is_none()
        } else {
            !allow_partial_station_completion
                && !rezka_progress_metrics_are_complete(
                    metrics.total_waste,
                    metrics.rezka_bosma_waste,
                    metrics.rezka_lamination_waste,
                    metrics.rezka_edge_waste,
                )
        };
    let missing_rezka_quantity = !rezka_quantity_metrics_are_complete(
        progress.produced_qty,
        rezka_gross_qty,
        metrics.finished_goods_kg,
        metrics.finished_goods_meter,
    );
    let missing_rezka_diameter = is_rezka && metrics.diameter.is_none();
    if is_rezka && (missing_rezka_waste || missing_rezka_quantity || missing_rezka_diameter) {
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

fn valid_non_negative_optional_progress_qty(
    value: Option<f64>,
) -> Result<Option<f64>, ProductionMapError> {
    match value {
        Some(value) if value.is_finite() && value >= 0.0 => Ok(Some(value)),
        Some(_) => Err(ProductionMapError::ProgressInputInvalid),
        None => Ok(None),
    }
}

pub(crate) fn bosma_completion_metrics_are_complete(
    has_return_ink_or_report: bool,
    total_waste: Option<f64>,
    finished_goods_kg: Option<f64>,
    finished_goods_meter: Option<f64>,
) -> bool {
    has_return_ink_or_report
        && total_waste.is_some()
        && finished_goods_kg.is_some()
        && finished_goods_meter.is_some()
}

pub(crate) fn laminatsiya_completion_metrics_are_complete(
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
    total_waste: Option<f64>,
    rezka_bosma_waste: Option<f64>,
    rezka_lamination_waste: Option<f64>,
    rezka_edge_waste: Option<f64>,
) -> bool {
    total_waste.is_some()
        || rezka_bosma_waste.is_some()
        || rezka_lamination_waste.is_some()
        || rezka_edge_waste.is_some()
}

fn rezka_quantity_metrics_are_complete(
    produced_qty: Option<f64>,
    gross_qty: Option<f64>,
    finished_goods_kg: Option<f64>,
    finished_goods_meter: Option<f64>,
) -> bool {
    (produced_qty.is_some() || finished_goods_meter.is_some())
        && (gross_qty.is_some() || finished_goods_kg.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::apparatus_standard::test_support::{
        TestApparatusSpec, runtime_configuration,
    };

    fn rezka_pause_progress(diameter: Option<f64>) -> QueueProgressInput {
        QueueProgressInput {
            produced_qty: Some(10.0),
            gross_qty: Some(11.0),
            diameter,
            ..QueueProgressInput::default()
        }
    }

    fn rezka_canonical() -> RuntimeApparatusConfiguration {
        runtime_configuration(TestApparatusSpec::cut(
            "apparatus:default:asset-010",
            "Renamed rezka",
        ))
    }

    #[test]
    fn rezka_pause_requires_positive_finite_diameter() {
        let canonical = rezka_canonical();
        let valid = validated_progress_metrics(
            "Rezka",
            &canonical,
            queue_state::ApparatusQueueAction::Pause,
            &rezka_pause_progress(Some(45.5)),
            false,
        )
        .expect("positive finite diameter");
        assert_eq!(valid.diameter, Some(45.5));

        for diameter in [Some(0.0), Some(-1.0), Some(f64::NAN), Some(f64::INFINITY)] {
            assert!(matches!(
                validated_progress_metrics(
                    "Rezka",
                    &canonical,
                    queue_state::ApparatusQueueAction::Pause,
                    &rezka_pause_progress(diameter),
                    false,
                ),
                Err(ProductionMapError::ProgressInputInvalid)
            ));
        }
        assert!(matches!(
            validated_progress_metrics(
                "Rezka",
                &canonical,
                queue_state::ApparatusQueueAction::Pause,
                &rezka_pause_progress(None),
                false,
            ),
            Err(ProductionMapError::RezkaProgressMetricsRequired)
        ));
    }
}
