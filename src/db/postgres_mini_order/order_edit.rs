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
    let mut template: CalculateOrderTemplate = match calculation {
        Some(value) => serde_json::from_value(value).map_err(|_| Error::Store)?,
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
            let layers = sqlx::query_scalar::<_, serde_json::Value>(
                "SELECT layers_json FROM mini_order_products WHERE order_id = $1",
            )
            .bind(id)
            .fetch_all(&mut **tx)
            .await?;
            legacy_calculation(values, &map, &layers)?
        }
    };
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

fn legacy_calculation(
    values: Vec<serde_json::Value>,
    map: &ProductionMapDefinition,
    layers: &[serde_json::Value],
) -> Result<CalculateOrderTemplate, Error> {
    if values.is_empty() {
        return Err(Error::Locked(
            "Bu buyurtmaning dastlabki Calculate hisob-kitobi bazada saqlanmagan va unga mos shablon topilmadi. Bu hozir kiritgan ma’lumotlaringizdagi xato emas. Hisob-kitobni taxmin qilib o‘zgartirmaslik uchun tahrirlash bloklandi. Mas’ul administratorga buyurtma raqamini yuboring",
        ));
    }
    let candidates = values
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<CalculateOrderTemplate>, _>>()
        .map_err(|_| Error::Store)?;
    // Order numbers are stronger evidence than reusable source-map links.
    // Never select by recency or product alone, and never fall back to a weaker
    // link when an explicit order-specific record exists but is incompatible.
    let has_exact_order = candidates
        .iter()
        .any(|t| t.order_number.trim() == map.order_number.trim());
    let same = |a: f64, b: f64| a.is_finite() && b.is_finite() && (a - b).abs() < 0.001;
    let same_optional = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) => same(a, b),
        (None, None) => true,
        _ => false,
    };
    let mut matches = Vec::new();
    for template in candidates {
        let identity_matches = if has_exact_order {
            template.order_number.trim() == map.order_number.trim()
        } else {
            template.order_number.trim().is_empty()
                && (template.source_map_id.trim() == map.id
                    || template.source_map_id.trim() == format!("template-{}", map.id))
        };
        let width = crate::core::formula::derive_width_mm(
            Some(template.frame_product_size_mm),
            Some(template.frame_count),
            Some(template.edge_allowance_mm),
        )
        .ok();
        if !identity_matches
            || template.item_code.trim() != map.product_code.trim()
            || template.product.trim() != map.title.trim()
            || !same_optional(width, map.width_mm)
            || template.roll_count != map.roll_count
            || !same_optional(template.print_val_size_mm, map.print_val_size_mm)
            || (template.kg != 0.0 && !map.order_kg.is_some_and(|kg| same(template.kg, kg)))
            || layers.len() != 1
            || layers[0]
                != serde_json::to_value(template.effective_layers()).map_err(|_| Error::Store)?
        {
            continue;
        }
        matches.push(template);
    }
    match matches.len() {
        0 => Err(Error::Locked(
            "Buyurtmaning Calculate shablonlari topildi, lekin order raqami, mahsulot, o‘lcham, rang yoki materiallari saqlangan buyurtmaga mos kelmadi. Noto‘g‘ri hisob-kitobni yuklamaslik uchun tahrirlash bloklandi. Mas’ul administrator buyurtma va shablonlarni tekshirishi kerak",
        )),
        1 => matches.pop().ok_or(Error::Store),
        _ => Err(Error::Locked(
            "Bu buyurtmaga mos bir nechta Calculate shabloni topildi. Order raqami, o‘lcham va materiallar bo‘yicha ham yagona nusxani ajratib bo‘lmadi. Tahrirlash uchun mas’ul administrator buyurtma va shablon bog‘lanishini tekshirishi kerak",
        )),
    }
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

fn activity_reason(table: &str) -> &'static str {
    match table {
        "mini_queue_action_events" => {
            "Buyurtma bo‘yicha apparat navbatida amal bajarilgan. Navbat amallari tarixini tekshiring. Harakat tarixi bor buyurtmani tahrirlash taqiqlangan"
        }
        "mini_order_run_sessions" => {
            "Buyurtmada apparatdagi ish sessiyasi ochilgan. Ish keyin to‘xtatilgan yoki tugatilgan bo‘lsa ham tahrirlash taqiqlangan"
        }
        "mini_order_progress_events" | "mini_progress_batches" => {
            "Buyurtmada ishlab chiqarish natijasi yoki WIP partiyasi qayd etilgan. Ishlab chiqarish tarixini tekshiring. Harakat boshlangan buyurtmani tahrirlash taqiqlangan"
        }
        "mini_raw_material_assignments" | "mini_raw_material_events" => {
            "Buyurtmaga xomashyo biriktirilgan yoki xomashyo harakati qayd etilgan. Xomashyo tarixini tekshiring. Xomashyo keyin ajratilgan bo‘lsa ham tahrirlash taqiqlangan"
        }
        "mini_opening_wip_intakes" | "mini_opening_wip_batches" => {
            "Buyurtmada boshlang‘ich yarim tayyor mahsulot (opening WIP) ochilgan. Opening WIP tarixini tekshiring. Keyin bekor qilingan bo‘lsa ham tahrirlash taqiqlangan"
        }
        "mini_order_control_states" | "mini_order_freeze_requests" => {
            "Buyurtmada muzlatish yoki boshqaruv holati qayd etilgan. Buyurtma holati tarixini tekshiring. Holat keyin tiklangan bo‘lsa ham tahrirlash taqiqlangan"
        }
        "mini_apparatus_order_transfers" => {
            "Buyurtmani boshqa apparatga ko‘chirish qayd etilgan. Ko‘chirish tarixini tekshiring. Harakat tarixi bor buyurtmani tahrirlash taqiqlangan"
        }
        "mini_production_order_lifecycle_events" => {
            "Buyurtmaning ishlab chiqarish holati o‘zgartirilgan. Buyurtma tarixini tekshiring. Harakat tarixi bor buyurtmani tahrirlash taqiqlangan"
        }
        "mini_apparatus_schedule_reservations" => {
            "Buyurtma uchun apparatda ishlab chiqarish vaqti band qilingan. Ish jadvalini tekshiring. Bandlov keyin bekor qilingan bo‘lsa ham uning tarixi tahrirlashni bloklaydi"
        }
        "mini_qolip_order_notes" => {
            "Buyurtma bo‘yicha qolip bo‘limida yozuv kiritilgan. Qolip bo‘limidagi buyurtma yozuvini tekshiring. Harakat tarixi bor buyurtmani tahrirlash taqiqlangan"
        }
        "mini_finished_goods_stock" => {
            "Buyurtma bo‘yicha tayyor mahsulot ombor yozuvi mavjud. Tayyor mahsulot tarixini tekshiring. Bunday buyurtmani tahrirlash taqiqlangan"
        }
        "mini_laminatsiya_astatka_reports" => {
            "Buyurtmada laminatsiya qoldig‘i hisoboti kiritilgan. Laminatsiya tarixini tekshiring. Harakat tarixi bor buyurtmani tahrirlash taqiqlangan"
        }
        "mini_rezka_astatka_reports" => {
            "Buyurtmada Rezka qoldig‘i hisoboti kiritilgan. Rezka tarixini tekshiring. Harakat tarixi bor buyurtmani tahrirlash taqiqlangan"
        }
        "mini_bosma_astatka_reports" => {
            "Buyurtmada bosma qoldig‘i hisoboti kiritilgan. Bosma tarixini tekshiring. Harakat tarixi bor buyurtmani tahrirlash taqiqlangan"
        }
        "mini_preparation_operations" => {
            "Buyurtmada tayyorlov amali qayd etilgan. Tayyorlov bo‘limidagi buyurtma tarixini tekshiring. Harakat boshlangan buyurtmani tahrirlash taqiqlangan"
        }
        "mini_returned_paint_images" | "mini_returned_paint_requests" => {
            "Buyurtmada bo‘yoq qaytarish yozuvi mavjud. Bo‘yoq qaytarish tarixini tekshiring. Harakat tarixi bor buyurtmani tahrirlash taqiqlangan"
        }
        _ => {
            "Buyurtmada harakat tarixi mavjud. Mas’ul administrator buyurtma tarixini tekshirishi kerak. Tahrirlash taqiqlangan"
        }
    }
}

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
            "Buyurtmaning ishlab chiqarish holati avval o‘zgartirilgan yoki hali chiqarilgan holatda emas. Buyurtma tarixini tekshiring. Faqat hech qanday harakat boshlanmagan buyurtmani tahrirlash mumkin",
        ));
    }
    for table in ACTIVITY_TABLES {
        let query = format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE order_id = $1)");
        if sqlx::query_scalar::<_, bool>(&query)
            .bind(id)
            .fetch_one(&mut **tx)
            .await?
        {
            return Err(Error::Locked(activity_reason(table)));
        }
    }
    let (work_started, material_reserved) = sqlx::query_as::<_, (bool, bool)>(
        "SELECT EXISTS(SELECT 1 FROM mini_queue_states WHERE order_id = $1 AND state <> 'pending'),
         EXISTS(SELECT 1 FROM mini_raw_material_stock WHERE reserved_order_id = $1)",
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?;
    if work_started {
        return Err(Error::Locked(
            "Buyurtma apparatda kutish holatida emas: ish boshlangan yoki holati o‘zgartirilgan. Apparatdagi buyurtma holatini tekshiring. Bunday buyurtmani tahrirlash taqiqlangan",
        ));
    }
    if material_reserved {
        return Err(Error::Locked(
            "Buyurtma uchun omborda xomashyo band qilingan. Xomashyo bandlovini tekshiring. Xomashyo bog‘langan buyurtmani tahrirlash taqiqlangan",
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
