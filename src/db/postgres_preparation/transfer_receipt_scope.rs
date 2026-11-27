use super::{AVAILABLE_STOCK, SERIYO_GROUPS_SQL};

pub(super) fn pending_lots_sql(transfer_ids: &str) -> String {
    let already_eligible = preparation_lot_for_master!("$2", "true");
    format!(
        "WITH RECURSIVE {SERIYO_GROUPS_SQL}
         SELECT t.id AS transfer_id, s.id AS stock_id
         FROM mini_inventory_transfers t
         JOIN mini_inventory_transfer_lines l
           ON l.transfer_id=t.id AND l.asset_kind='raw_material'
         JOIN mini_raw_material_stock s ON s.id=l.asset_ref
         JOIN mini_warehouses w ON w.id=t.destination_warehouse_id
           AND lower(w.name)=lower(s.warehouse) AND NOT w.is_group
         JOIN mini_preparation_materials m ON m.item_code=s.item_code AND m.owner_ref=$2
         JOIN mini_items i ON i.code=m.item_code
         LEFT JOIN mini_preparation_receipts r ON r.stock_id=s.id
         WHERE t.id IN ({transfer_ids}) AND t.status='received'
           AND r.id IS NULL AND s.qty>0 AND s.uom='kg' AND {AVAILABLE_STOCK}
           AND NOT COALESCE({already_eligible}, false)
           AND EXISTS (
               SELECT 1 FROM mini_warehouse_assignments a
               WHERE a.assignment_kind='warehouse' AND a.principal_role='tayyorlov_masteri'
                 AND a.principal_ref=$2 AND lower(a.warehouse_name)=lower(w.name)
           )
           AND EXISTS (
               SELECT 1 FROM seriyo_groups g
               WHERE lower(btrim(g.name))=lower(btrim(i.item_group))
           )
           AND (
               EXISTS (
                   SELECT 1 FROM mini_preparation_material_warehouse_scopes scope
                   WHERE scope.item_code=m.item_code AND scope.warehouse_id=w.id AND scope.active
               )
               OR (
                   NOT EXISTS (
                       SELECT 1 FROM mini_preparation_material_warehouse_scopes scope
                       WHERE scope.item_code=m.item_code
                   )
                   AND (m.warehouse_name IS NULL OR lower(m.warehouse_name)=lower(w.name))
               )
           )
           AND NOT EXISTS (
               SELECT 1 FROM mini_inventory_transfer_lines later_line
               JOIN mini_inventory_transfers later ON later.id=later_line.transfer_id
               WHERE later_line.asset_kind='raw_material' AND later_line.asset_ref=s.id
                 AND later.status='received'
                 AND (later.received_at, later.id) > (t.received_at, t.id)
           )"
    )
}
