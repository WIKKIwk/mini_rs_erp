use crate::core::combo::COMBO_ITEM_GROUP;
use crate::core::gscale::{epc::GscaleEpcGenerator, ports::EpcSource};

use super::*;

pub(super) async fn revision(
    tx: &mut Transaction<'_, Postgres>,
    id: String,
    name: String,
    revision: i64,
    layout_fingerprint: String,
    template: &CalculateOrderTemplate,
) -> Result<ComboReference, ComboError> {
    let generator = GscaleEpcGenerator::new();
    for _ in 0..5 {
        let reference = ComboReference {
            id: id.clone(),
            name: name.clone(),
            revision,
            item_code: generator.next_epc(),
            layout_fingerprint: layout_fingerprint.clone(),
        };
        let catalog_name = format!("{name} · v{revision}");
        let payload = serde_json::json!({
            "code": reference.item_code, "name": catalog_name, "uom": "Kg",
            "item_group": COMBO_ITEM_GROUP, "kind": "combo", "stockable": false,
            "combo": reference,
        });
        let inserted = sqlx::query(
            "INSERT INTO mini_items(code,name,uom,item_group,payload_json)
             VALUES ($1,$2,'Kg',$3,$4) ON CONFLICT (code) DO NOTHING",
        )
        .bind(&reference.item_code)
        .bind(&catalog_name)
        .bind(COMBO_ITEM_GROUP)
        .bind(payload)
        .execute(&mut **tx)
        .await
        .map_err(store_error)?
        .rows_affected();
        if inserted == 0 {
            continue;
        }
        let mut snapshot = template.clone();
        snapshot.combo = Some(reference.clone());
        snapshot = crate::core::calculate_orders::hydrate_template_dimensions(snapshot);
        snapshot.id.clear();
        snapshot.variant_id.clear();
        snapshot.code.clear();
        snapshot.order_number.clear();
        snapshot.saved_at.clear();
        snapshot.source_map_id.clear();
        snapshot.kg = 0.0;
        sqlx::query(
            "INSERT INTO mini_combo_revisions
            (combo_id,revision,item_code,layout_fingerprint,reference_json,template_json)
            VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(&reference.id)
        .bind(revision)
        .bind(&reference.item_code)
        .bind(&reference.layout_fingerprint)
        .bind(serde_json::to_value(&reference).map_err(|_| ComboError::StoreFailed)?)
        .bind(serde_json::to_value(snapshot).map_err(|_| ComboError::StoreFailed)?)
        .execute(&mut **tx)
        .await
        .map_err(store_error)?;
        sqlx::query("INSERT INTO mini_customer_items(customer_ref,item_code) VALUES ($1,$2)")
            .bind(template.customer_ref.trim())
            .bind(&reference.item_code)
            .execute(&mut **tx)
            .await
            .map_err(store_error)?;
        return Ok(reference);
    }
    Err(ComboError::StoreFailed)
}
