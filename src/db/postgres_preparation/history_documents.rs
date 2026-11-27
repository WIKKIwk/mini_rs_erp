use super::*;

pub(super) async fn operation_history_documents(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    ids: Option<&[String]>,
    limit: i64,
) -> Result<Vec<Value>, PreparationError> {
    let documents = sqlx::query_scalar(
        "SELECT response_json
                    || COALESCE((
                        SELECT jsonb_build_object('warehouse', w.name)
                        FROM mini_preparation_warehouse_history_names link
                        JOIN mini_warehouses w ON w.id = link.warehouse_id
                        WHERE link.operation_id = operation.id
                    ), '{}'::jsonb)
                    || jsonb_build_object('id', operation.id, 'created_at', created_at)
                    || CASE WHEN kind = 'receipt' THEN jsonb_build_object(
                        'reversed', EXISTS (
                            SELECT 1
                            FROM mini_preparation_receipt_reversals reversal
                            WHERE reversal.receipt_id = operation.id
                        ),
                        'can_reverse', EXISTS (
                            SELECT 1
                            FROM mini_preparation_receipts receipt
                            JOIN mini_raw_material_stock stock
                              ON stock.id = receipt.stock_id
                            WHERE receipt.id = operation.id
                              AND receipt.owner_ref = $1
                              AND stock.status = 'available'
                              AND stock.payload_json->>'paddon_issue_id' IS NULL
                              AND stock.reserved_order_id = ''
                              AND round(stock.qty::numeric, 6) = receipt.initial_kg
                              AND btrim(COALESCE(stock.payload_json->>'inventory_transfer_id', '')) = ''
                              AND NOT EXISTS (
                                  SELECT 1
                                  FROM mini_raw_material_assignments assignment
                                  WHERE lower(assignment.barcode) = lower(stock.barcode)
                              )
                              AND NOT EXISTS (
                                  SELECT 1
                                  FROM mini_preparation_allocations allocation
                                  WHERE allocation.receipt_id = receipt.id
                              )
                              AND NOT EXISTS (
                                  SELECT 1
                                  FROM mini_raw_material_splits split
                                  WHERE split.parent_stock_id = stock.id
                              )
                              AND NOT EXISTS (
                                  SELECT 1
                                  FROM mini_raw_material_split_outputs split_output
                                  WHERE split_output.stock_id = stock.id
                              )
                              AND NOT EXISTS (
                                  SELECT 1
                                  FROM mini_inventory_placements placement
                                  JOIN mini_inventory_locations location
                                    ON location.id = placement.physical_location_id
                                  LEFT JOIN mini_warehouses warehouse
                                    ON warehouse.id = location.warehouse_id
                                  WHERE placement.asset_kind = 'raw_material'
                                    AND lower(placement.asset_ref) = lower(stock.id)
                                    AND (
                                        location.kind <> 'warehouse'
                                        OR warehouse.name IS NULL
                                        OR lower(warehouse.name) <> lower(stock.warehouse)
                                    )
                              )
                              AND NOT EXISTS (
                                  SELECT 1
                                  FROM mini_raw_material_events event
                                  WHERE lower(event.barcode) = lower(stock.barcode)
                                    AND event.event_type <> 'receipt_posted'
                              )
                              AND NOT EXISTS (
                                  SELECT 1
                                  FROM mini_preparation_receipt_reversals reversal
                                  WHERE reversal.receipt_id = receipt.id
                              )
                        )
                    ) ELSE '{}'::jsonb END
             FROM mini_preparation_operations operation
             WHERE operation.owner_ref = $1 AND operation.kind <> 'material'
               AND operation.request_json->>'kind' IS DISTINCT FROM 'paddon_portion'
               AND ($2::text[] IS NULL OR operation.id = ANY($2))
             ORDER BY operation.created_at DESC, operation.id DESC LIMIT $3",
    )
    .bind(owner)
    .bind(ids)
    .bind(limit)
    .fetch_all(&mut **tx)
    .await?;
    Ok(documents)
}
