use sqlx::{Postgres, Transaction};

use crate::core::production_map::{
    OrderProgressEvent, ProductionMapError, QueueActionActor, StageAstatkaReport,
    queue_state::ApparatusQueueAction, reported_waste_kg,
};
use crate::core::quantity::erp_quantity_to_units;
use crate::db::postgres_waste_bins::{WasteEntry, record_production_waste};

pub(super) async fn record_progress(
    tx: &mut Transaction<'_, Postgres>,
    event: &OrderProgressEvent,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    let kg = event.payload_json["waste_bin_report_kg"]
        .as_f64()
        .unwrap_or_else(|| {
            reported_waste_kg(
                event.total_waste,
                [
                    event.rezka_bosma_waste,
                    event.rezka_lamination_waste,
                    event.rezka_edge_waste,
                ],
            )
        });
    let splice = event.action == ApparatusQueueAction::Merge;
    save(
        tx,
        WasteEntry {
            request_id: &format!("progress:{}", event.event_id),
            apparatus_id: &event.apparatus,
            order_id: &event.order_id,
            session_id: Some(&event.session_id),
            kg,
            reason: if splice {
                "Rulon ulash chiqindisi"
            } else {
                "Ishlab chiqarish hisoboti"
            },
            actor,
            reconcile_manual: !splice,
        },
    )
    .await
}

pub(super) async fn record_astatka(
    tx: &mut Transaction<'_, Postgres>,
    report: &StageAstatkaReport,
    session_id: Option<&str>,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    let (order_id, apparatus_id, report_id) = report.identity();
    let kg = match report {
        StageAstatkaReport::Bosma(report) => report.total_waste,
        StageAstatkaReport::Laminate(report) => report.total_waste,
        StageAstatkaReport::Cut(report) => reported_waste_kg(
            Some(report.total_waste),
            [
                Some(report.rezka_bosma_waste),
                Some(report.rezka_lamination_waste),
                Some(report.rezka_edge_waste),
            ],
        ),
    };
    save(
        tx,
        WasteEntry {
            request_id: &format!("astatka:{report_id}"),
            apparatus_id,
            order_id,
            session_id,
            kg,
            reason: "Astatka hisoboti",
            actor,
            reconcile_manual: true,
        },
    )
    .await
}

async fn save(
    tx: &mut Transaction<'_, Postgres>,
    entry: WasteEntry<'_>,
) -> Result<(), ProductionMapError> {
    let units = erp_quantity_to_units(entry.kg).ok_or(ProductionMapError::ProgressInputInvalid)?;
    if units <= 0 {
        return Ok(());
    }
    record_production_waste(tx, &entry).await.map_err(|error| {
        tracing::warn!(%error, apparatus_id = entry.apparatus_id, "could not record production waste");
        ProductionMapError::StoreFailed
    })
}

#[cfg(test)]
mod tests {
    use super::reported_waste_kg;

    #[test]
    fn a_rezka_total_covers_the_categories_and_category_only_reports_are_counted() {
        let breakdown = [Some(2.0), Some(3.0), None];
        assert_eq!(reported_waste_kg(Some(5.0), breakdown), 5.0);
        assert_eq!(reported_waste_kg(None, breakdown), 5.0);
        assert_eq!(reported_waste_kg(Some(0.0), [None; 3]), 0.0);
    }
}
