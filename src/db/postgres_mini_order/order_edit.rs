use std::collections::{BTreeMap, BTreeSet};

use sqlx::{PgPool, Postgres, Transaction};

use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::order_edit::{OrderEditError as Error, OrderEditSource, check_queue_position};
use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapService, QueueActionActor,
    queue_state::ApparatusQueueOrderState,
};

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(?error, "opened order edit persistence failed");
        Self::Store
    }
}

pub(super) async fn load_source(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<OrderEditSource, Error> {
    let (map, calculation, revision) =
        sqlx::query_as::<_, (serde_json::Value, Option<serde_json::Value>, i64)>(
            "SELECT m.map_json, o.calculation_json, o.calculation_revision
         FROM mini_orders o JOIN mini_production_maps m ON m.id = o.id WHERE o.id = $1",
        )
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(Error::NotFound)?;
    let map: ProductionMapDefinition = serde_json::from_value(map).map_err(|_| Error::Store)?;
    if id.starts_with("template-") || map.order_number.trim().is_empty() {
        return Err(Error::Locked("Faqat ochilgan buyurtmani tahrirlash mumkin"));
    }
    let legacy = calculation.is_none();
    let calculation = match calculation {
        Some(value) => value,
        None => {
            // Legacy fallback only accepts an unambiguous order-specific input,
            // never another order with the same product code.
            let values = sqlx::query_scalar::<_, serde_json::Value>(
                "SELECT payload_json FROM mini_quick_order_templates
                 WHERE payload_json->>'source_map_id' IN ($1, 'template-' || $1)
                    OR (payload_json->>'order_number' = $2 AND $2 <> '')",
            )
            .bind(id)
            .bind(&map.order_number)
            .fetch_all(&mut **tx)
            .await?;
            if values.len() != 1 {
                return Err(Error::Locked(
                    "Buyurtmaning asl Calculate ma’lumotlari saqlanmagan: tahrirlash mumkin emas",
                ));
            }
            values.into_iter().next().ok_or(Error::Store)?
        }
    };
    let mut template: CalculateOrderTemplate =
        serde_json::from_value(calculation).map_err(|_| Error::Store)?;
    if legacy {
        let layers = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT layers_json FROM mini_order_products WHERE order_id = $1",
        )
        .bind(id)
        .fetch_all(&mut **tx)
        .await?;
        let expected =
            serde_json::to_value(template.effective_layers()).map_err(|_| Error::Store)?;
        if layers.len() != 1 || layers[0] != expected {
            return Err(Error::Locked(
                "Asl Calculate materiallari tezkor buyurtma bilan mos emas",
            ));
        }
    }
    template.source_map_id = map.id.clone();
    template.order_number = map.order_number.clone();
    template.code = map.code.clone();
    template.kg = map.order_kg.unwrap_or(template.kg);
    Ok(OrderEditSource {
        map,
        template,
        revision,
    })
}

// Historical rows intentionally count, even after detach/cancel/reset. A
// pending status alone cannot prove that production has never started.
const ACTIVITY_TABLES: &[&str] = &[
    "mini_queue_action_events",
    "mini_order_run_sessions",
    "mini_order_progress_events",
    "mini_progress_batches",
    "mini_raw_material_assignments",
    "mini_raw_material_events",
    "mini_opening_wip_intakes",
    "mini_opening_wip_batches",
    "mini_order_control_states",
    "mini_order_freeze_requests",
    "mini_apparatus_order_transfers",
    "mini_production_order_lifecycle_events",
    "mini_apparatus_schedule_reservations",
    "mini_qolip_order_notes",
    "mini_finished_goods_stock",
    "mini_laminatsiya_astatka_reports",
    "mini_rezka_astatka_reports",
    "mini_bosma_astatka_reports",
    "mini_preparation_operations",
    "mini_returned_paint_images",
    "mini_returned_paint_requests",
];

pub(super) async fn check_eligible(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> Result<(), Error> {
    let pristine = sqlx::query_scalar::<_, bool>(
        "SELECT lifecycle_status = 'released' AND lifecycle_version = 0
         FROM mini_production_maps WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(Error::NotFound)?;
    if !pristine {
        return Err(Error::Locked(
            "Buyurtmada ishlab chiqarish harakati boshlangan",
        ));
    }
    for table in ACTIVITY_TABLES {
        let query = format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE order_id = $1)");
        if sqlx::query_scalar::<_, bool>(&query)
            .bind(id)
            .fetch_one(&mut **tx)
            .await?
        {
            return Err(Error::Locked(match *table {
                "mini_raw_material_assignments" | "mini_raw_material_events" => {
                    "Buyurtmaga xomashyo ulangan yoki xomashyo harakati bo‘lgan"
                }
                "mini_opening_wip_intakes" | "mini_opening_wip_batches" => {
                    "Buyurtmada opening WIP ochilgan"
                }
                "mini_apparatus_schedule_reservations" => {
                    "Buyurtma uchun ishlab chiqarish vaqti band qilingan"
                }
                _ => "Buyurtmada harakat tarixi mavjud: tahrirlash mumkin emas",
            }));
        }
    }
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM mini_queue_states WHERE order_id = $1 AND state <> 'pending')
         OR EXISTS(SELECT 1 FROM mini_raw_material_stock WHERE reserved_order_id = $1)",
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?
    {
        return Err(Error::Locked(
            "Buyurtmada ish boshlangan yoki xomashyo band qilingan",
        ));
    }
    check_queues(tx, id).await
}

async fn check_queues(tx: &mut Transaction<'_, Postgres>, id: &str) -> Result<(), Error> {
    let maps = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT map_json FROM mini_production_maps ORDER BY updated_at DESC, id ASC",
    )
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(serde_json::from_value)
    .collect::<Result<Vec<ProductionMapDefinition>, _>>()
    .map_err(|_| Error::Store)?;
    let sequences = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT canonical_apparatus_id, order_ids FROM mini_queue_sequences",
    )
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|(key, value)| serde_json::from_value::<Vec<String>>(value).map(|ids| (key, ids)))
    .collect::<Result<BTreeMap<_, _>, _>>()
    .map_err(|_| Error::Store)?;
    let frozen = sqlx::query_scalar::<_, String>(
        "SELECT order_id FROM mini_order_control_states WHERE state = 'frozen'",
    )
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .collect::<BTreeSet<_>>();
    let sequences =
        ProductionMapService::effective_apparatus_sequences_for_maps(&maps, &sequences, &frozen);
    let mut states: BTreeMap<String, BTreeMap<String, ApparatusQueueOrderState>> = BTreeMap::new();
    for (apparatus, order, state) in sqlx::query_as::<_, (String, String, String)>(
        "SELECT canonical_apparatus_id, order_id, state FROM mini_queue_states",
    )
    .fetch_all(&mut **tx)
    .await?
    {
        states.entry(apparatus).or_default().insert(
            order,
            ApparatusQueueOrderState::parse(&state).ok_or(Error::Store)?,
        );
    }
    check_queue_position(id, &sequences, &states)
}

pub(super) async fn save(
    pool: &PgPool,
    original: &OrderEditSource,
    map: &ProductionMapDefinition,
    template: &CalculateOrderTemplate,
    actor: &QueueActionActor,
) -> Result<OrderEditSource, Error> {
    let mut tx = pool.begin().await?;
    // A bounded, short critical section also excludes writers from other
    // backend processes. In-process production actions share the service guard.
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL statement_timeout = '10s'")
        .execute(&mut *tx)
        .await?;
    super::super::postgres_production_map::lock_order_for_edit_tx(&mut tx, &original.map.id)
        .await?;
    sqlx::query("LOCK TABLE mini_production_maps, mini_orders, mini_queue_sequences, mini_queue_states IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut *tx).await?;
    for table in ACTIVITY_TABLES.iter().copied().chain([
        "mini_raw_material_stock",
        "mini_order_products",
        "mini_quick_order_templates",
    ]) {
        sqlx::query(&format!("LOCK TABLE {table} IN SHARE ROW EXCLUSIVE MODE"))
            .execute(&mut *tx)
            .await?;
    }
    let current = load_source(&mut tx, &original.map.id).await?;
    if current.revision != original.revision
        || current.map != original.map
        || current.template != original.template
    {
        return Err(Error::Conflict);
    }
    if map.id != current.map.id
        || map.code != current.map.code
        || map.order_number != current.map.order_number
    {
        return Err(Error::Conflict);
    }
    check_eligible(&mut tx, &map.id).await?;
    // updated_at is also the implicit queue ordering key. Editing must not
    // move the order (or other orders) when no explicit sequence was saved.
    let queue_position = sqlx::query_scalar::<_, String>(
        "SELECT updated_at::text FROM mini_production_maps WHERE id = $1",
    )
    .bind(&map.id)
    .fetch_one(&mut *tx)
    .await?;
    super::super::postgres_production_map::save_edited_map_tx(&mut tx, map).await?;
    let payload = serde_json::to_value(template).map_err(|_| Error::Store)?;
    let audit = serde_json::json!({ "actor": actor, "before": current.template, "after": template, "revision": current.revision + 1 });
    sqlx::query("UPDATE mini_orders SET calculation_json = $2, calculation_revision = calculation_revision + 1,
        calculation_edit_log = calculation_edit_log || jsonb_build_array($3::jsonb || jsonb_build_object('at', now())) WHERE id = $1")
        .bind(&map.id).bind(payload).bind(audit).execute(&mut *tx).await?;
    super::save_order_tx(&mut tx, map, template)
        .await
        .map_err(|_| Error::Store)?;
    sqlx::query("UPDATE mini_production_maps SET updated_at = $2::text::timestamptz WHERE id = $1")
        .bind(&map.id)
        .bind(queue_position)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(OrderEditSource {
        map: map.clone(),
        template: template.clone(),
        revision: current.revision + 1,
    })
}

#[cfg(test)]
mod tests;
