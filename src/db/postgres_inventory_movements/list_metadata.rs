use super::*;
use crate::core::production_map::{ProductionMapDefinition, compile_map};

// Both reads are bounded by the inventory page, not by the complete production
// history. No execution-status hydration or global queue snapshot is needed.
pub(super) async fn list_order_assignments(
    store: &PostgresInventoryMovementStore,
    actor: &InventoryActor,
    barcodes: &[String],
    assigned_apparatus: &[String],
) -> Result<Vec<InventoryOrderAssignment>, InventoryMovementError> {
    let warehouses = actor
        .assigned_warehouses
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let rows = sqlx::query_as::<_, (String, String, String)>(
        r#"
        WITH visible_assignments AS MATERIALIZED (
            SELECT barcode, order_id, canonical_apparatus_id
            FROM mini_raw_material_assignments
            WHERE lower(barcode) = ANY($1::text[])
              AND canonical_apparatus_id = ANY($2::text[])
              AND EXISTS (
                  SELECT 1 FROM mini_apparatus apparatus
                  WHERE apparatus.id = canonical_apparatus_id
              )
        )
        SELECT assignment.barcode, assignment.order_id,
               COALESCE(control.state, 'active') AS order_control
        FROM visible_assignments assignment
        JOIN mini_raw_material_stock stock ON lower(stock.barcode) = lower(assignment.barcode)
        LEFT JOIN mini_order_control_states control ON control.order_id = assignment.order_id
        WHERE lower(btrim(stock.warehouse)) = ANY($3::text[])
          AND stock.status <> 'deleted'
        ORDER BY assignment.barcode
        "#,
    )
    .bind(barcodes)
    .bind(assigned_apparatus)
    .bind(warehouses)
    .fetch_all(&store.pool)
    .await
    .map_err(store_error)?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let order_ids = rows
        .iter()
        .filter(|(_, _, state)| state == "active")
        .map(|(_, id, _)| id.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut labels = BTreeMap::new();
    if !order_ids.is_empty() {
        let maps = sqlx::query_scalar::<_, Value>(
            "SELECT map_json FROM mini_production_maps
             WHERE id = ANY($1::text[])
               AND lifecycle_status IN ('released', 'in_progress')",
        )
        .bind(order_ids)
        .fetch_all(&store.pool)
        .await
        .map_err(store_error)?;
        for payload in maps {
            // Match the prior assignment-orders contract, including its
            // fallback for a stored map which cannot be compiled.
            if let Ok(map) = serde_json::from_value::<ProductionMapDefinition>(payload) {
                if map.id.trim().starts_with("zakaz-") && compile_map(&map).is_ok() {
                    labels.insert(map.id.trim().to_string(), order_label(&map));
                }
            }
        }
    }
    Ok(rows
        .into_iter()
        .map(|(barcode, order_id, order_control)| {
            let order_label = labels
                .get(order_id.trim())
                .cloned()
                .unwrap_or_else(|| order_id.trim().to_string());
            InventoryOrderAssignment {
                barcode,
                order_id,
                order_label,
                order_control,
            }
        })
        .collect())
}

fn order_label(map: &ProductionMapDefinition) -> String {
    let code = [map.code.trim(), map.order_number.trim(), map.id.trim()]
        .into_iter()
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    let title = map.title.trim();
    if title.is_empty() {
        code.to_string()
    } else {
        format!("{code} · {title}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_list_label_preserves_code_number_and_id_fallbacks() {
        for (code, number, title, expected) in [
            (" 0042 ", "0043", " BOPP ", "0042 · BOPP"),
            ("", "0043", " BOPP ", "0043 · BOPP"),
            ("", "", "BOPP", "zakaz-0044 · BOPP"),
            ("0042", "0043", "", "0042"),
        ] {
            let map = serde_json::from_value::<ProductionMapDefinition>(serde_json::json!({
                "id": "zakaz-0044", "product_code": "BOPP", "title": title,
                "code": code, "order_number": number,
            }))
            .unwrap();
            assert_eq!(order_label(&map), expected);
        }
    }

    #[test]
    fn inventory_list_assignment_exposes_only_compact_badge_fields() {
        let badge = InventoryOrderAssignment {
            barcode: "internal-index-only".into(),
            order_id: "zakaz-0044".into(),
            order_label: "0044 · BOPP".into(),
            order_control: "frozen".into(),
        };
        assert_eq!(
            serde_json::to_value(badge).unwrap(),
            serde_json::json!({
                "order_id": "zakaz-0044", "order_label": "0044 · BOPP", "order_control": "frozen",
            })
        );
    }
}
