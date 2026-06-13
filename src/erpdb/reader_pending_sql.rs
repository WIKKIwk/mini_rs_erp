pub(crate) const WERKA_PENDING_PUSHDOWN_SQL: &str = r#"
    SELECT *
    FROM (
        SELECT
            pr.name AS id,
            'purchase_receipt' AS record_type,
            TRIM(COALESCE(pr.supplier, '')) AS supplier_ref,
            TRIM(COALESCE(pr.supplier_name, '')) AS supplier_name,
            TRIM(COALESCE(pri.item_code, '')) AS item_code,
            TRIM(COALESCE(pri.item_name, '')) AS item_name,
            TRIM(COALESCE(pri.uom, '')) AS uom,
            CAST(GREATEST(
                COALESCE(pr.total_qty, 0),
                CASE
                    WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                         REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                    THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                    ELSE COALESCE(pr.total_qty, 0)
                END
            ) AS DOUBLE) AS sent_qty,
            CAST(0 AS DOUBLE) AS accepted_qty,
            CAST(COALESCE(pri.amount, 0) AS DOUBLE) AS amount,
            TRIM(COALESCE(pr.currency, '')) AS currency,
            CASE
                WHEN LOWER(TRIM(COALESCE(pr.status, ''))) = 'draft' THEN 'draft'
                ELSE 'pending'
            END AS status,
            TRIM(COALESCE(CAST(pr.posting_date AS CHAR), '')) AS created_label
        FROM `tabPurchase Receipt` pr
        LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
          AND pr.docstatus = 0
          AND COALESCE(pr.remarks, '') NOT LIKE '%Accord Werka Aytilmagan: pending%'
        UNION ALL
        SELECT
            dn.name AS id,
            'delivery_note' AS record_type,
            TRIM(COALESCE(dn.customer, '')) AS supplier_ref,
            TRIM(COALESCE(dn.customer_name, '')) AS supplier_name,
            TRIM(COALESCE(dni.item_code, '')) AS item_code,
            TRIM(COALESCE(dni.item_name, '')) AS item_name,
            TRIM(COALESCE(dni.uom, '')) AS uom,
            CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS sent_qty,
            CAST(0 AS DOUBLE) AS accepted_qty,
            CAST(0 AS DOUBLE) AS amount,
            '' AS currency,
            'pending' AS status,
            TRIM(COALESCE(CAST(dn.modified AS CHAR), '')) AS created_label
        FROM `tabDelivery Note` dn
        LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
        WHERE dn.docstatus = 1
          AND dn.accord_flow_state = 1
          AND dn.accord_customer_state NOT IN (2, 3, 4)
    ) pending_rows
    ORDER BY created_label DESC
"#;

pub(crate) const WERKA_PENDING_PUSHDOWN_LIMIT_SQL: &str = r#"
    SELECT *
    FROM (
        SELECT
            pr.name AS id,
            'purchase_receipt' AS record_type,
            TRIM(COALESCE(pr.supplier, '')) AS supplier_ref,
            TRIM(COALESCE(pr.supplier_name, '')) AS supplier_name,
            TRIM(COALESCE(pri.item_code, '')) AS item_code,
            TRIM(COALESCE(pri.item_name, '')) AS item_name,
            TRIM(COALESCE(pri.uom, '')) AS uom,
            CAST(GREATEST(
                COALESCE(pr.total_qty, 0),
                CASE
                    WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                         REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                    THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                    ELSE COALESCE(pr.total_qty, 0)
                END
            ) AS DOUBLE) AS sent_qty,
            CAST(0 AS DOUBLE) AS accepted_qty,
            CAST(COALESCE(pri.amount, 0) AS DOUBLE) AS amount,
            TRIM(COALESCE(pr.currency, '')) AS currency,
            CASE
                WHEN LOWER(TRIM(COALESCE(pr.status, ''))) = 'draft' THEN 'draft'
                ELSE 'pending'
            END AS status,
            TRIM(COALESCE(CAST(pr.posting_date AS CHAR), '')) AS created_label
        FROM `tabPurchase Receipt` pr
        LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
          AND pr.docstatus = 0
          AND COALESCE(pr.remarks, '') NOT LIKE '%Accord Werka Aytilmagan: pending%'
        UNION ALL
        SELECT
            dn.name AS id,
            'delivery_note' AS record_type,
            TRIM(COALESCE(dn.customer, '')) AS supplier_ref,
            TRIM(COALESCE(dn.customer_name, '')) AS supplier_name,
            TRIM(COALESCE(dni.item_code, '')) AS item_code,
            TRIM(COALESCE(dni.item_name, '')) AS item_name,
            TRIM(COALESCE(dni.uom, '')) AS uom,
            CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS sent_qty,
            CAST(0 AS DOUBLE) AS accepted_qty,
            CAST(0 AS DOUBLE) AS amount,
            '' AS currency,
            'pending' AS status,
            TRIM(COALESCE(CAST(dn.modified AS CHAR), '')) AS created_label
        FROM `tabDelivery Note` dn
        LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
        WHERE dn.docstatus = 1
          AND dn.accord_flow_state = 1
          AND dn.accord_customer_state NOT IN (2, 3, 4)
    ) pending_rows
    ORDER BY created_label DESC
    LIMIT ?
"#;
