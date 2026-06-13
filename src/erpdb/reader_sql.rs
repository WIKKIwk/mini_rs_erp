pub(crate) const PURCHASE_RECEIPT_ROWS_SQL: &str = r#"
    SELECT
        pr.name AS name,
        pr.supplier AS supplier,
        COALESCE(pr.supplier_name, '') AS supplier_name,
        pr.docstatus AS doc_status,
        COALESCE(pr.status, '') AS status,
        CAST(COALESCE(pr.total_qty, 0) AS DOUBLE) AS total_qty,
        COALESCE(CAST(pr.posting_date AS CHAR), '') AS posting_date,
        COALESCE(pr.supplier_delivery_note, '') AS supplier_delivery_note,
        COALESCE(pr.remarks, '') AS remarks,
        COALESCE(pr.currency, '') AS currency,
        COALESCE(pri.item_code, '') AS item_code,
        COALESCE(pri.item_name, '') AS item_name,
        COALESCE(pri.uom, '') AS uom,
        CAST(COALESCE(pri.amount, 0) AS DOUBLE) AS amount
    FROM `tabPurchase Receipt` pr
    LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
    WHERE pr.supplier_delivery_note LIKE 'TG:%'
    ORDER BY pr.name DESC
"#;

pub(crate) const DELIVERY_NOTE_ROWS_SQL: &str = r#"
    SELECT
        dn.name AS name,
        dn.customer AS customer,
        COALESCE(dn.customer_name, '') AS customer_name,
        dn.docstatus AS doc_status,
        COALESCE(CAST(dn.modified AS CHAR), '') AS modified,
        CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS qty,
        CAST(COALESCE(dni.returned_qty, 0) AS DOUBLE) AS returned_qty,
        COALESCE(dn.accord_customer_reason, '') AS customer_reason,
        COALESCE(dni.item_code, '') AS item_code,
        COALESCE(dni.item_name, '') AS item_name,
        COALESCE(dni.uom, '') AS uom,
        COALESCE(dn.accord_flow_state, 0) AS accord_flow_state,
        COALESCE(dn.accord_customer_state, 0) AS accord_customer_state
    FROM `tabDelivery Note` dn
    LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    ORDER BY dn.name DESC
"#;

pub(crate) const PURCHASE_RECEIPT_ROWS_LIMIT_SQL: &str = r#"
    SELECT
        pr.name AS name,
        pr.supplier AS supplier,
        COALESCE(pr.supplier_name, '') AS supplier_name,
        pr.docstatus AS doc_status,
        COALESCE(pr.status, '') AS status,
        CAST(COALESCE(pr.total_qty, 0) AS DOUBLE) AS total_qty,
        COALESCE(CAST(pr.posting_date AS CHAR), '') AS posting_date,
        COALESCE(pr.supplier_delivery_note, '') AS supplier_delivery_note,
        COALESCE(pr.remarks, '') AS remarks,
        COALESCE(pr.currency, '') AS currency,
        COALESCE(pri.item_code, '') AS item_code,
        COALESCE(pri.item_name, '') AS item_name,
        COALESCE(pri.uom, '') AS uom,
        CAST(COALESCE(pri.amount, 0) AS DOUBLE) AS amount
    FROM `tabPurchase Receipt` pr
    LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
    WHERE pr.supplier_delivery_note LIKE 'TG:%'
    ORDER BY pr.name DESC
    LIMIT ?
"#;

pub(crate) const DELIVERY_NOTE_ROWS_LIMIT_SQL: &str = r#"
    SELECT
        dn.name AS name,
        dn.customer AS customer,
        COALESCE(dn.customer_name, '') AS customer_name,
        dn.docstatus AS doc_status,
        COALESCE(CAST(dn.modified AS CHAR), '') AS modified,
        CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS qty,
        CAST(COALESCE(dni.returned_qty, 0) AS DOUBLE) AS returned_qty,
        COALESCE(dn.accord_customer_reason, '') AS customer_reason,
        COALESCE(dni.item_code, '') AS item_code,
        COALESCE(dni.item_name, '') AS item_name,
        COALESCE(dni.uom, '') AS uom,
        COALESCE(dn.accord_flow_state, 0) AS accord_flow_state,
        COALESCE(dn.accord_customer_state, 0) AS accord_customer_state
    FROM `tabDelivery Note` dn
    LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    ORDER BY dn.name DESC
    LIMIT ?
"#;

pub(crate) const WERKA_SUMMARY_PUSHDOWN_SQL: &str = r#"
    SELECT
        CAST(COALESCE(SUM(status IN ('pending', 'draft')), 0) AS SIGNED) AS pending_count,
        CAST(COALESCE(SUM(status = 'accepted'), 0) AS SIGNED) AS confirmed_count,
        CAST(COALESCE(SUM(status IN ('partial', 'rejected', 'cancelled')), 0) AS SIGNED) AS returned_count
    FROM (
        SELECT
            CASE
                WHEN pr.docstatus = 2 OR LOWER(TRIM(COALESCE(pr.status, ''))) = 'cancelled' THEN 'cancelled'
                WHEN pr.docstatus = 1 THEN
                    CASE
                        WHEN COALESCE(pr.total_qty, 0) <= 0 THEN 'rejected'
                        WHEN GREATEST(
                            COALESCE(pr.total_qty, 0),
                            CASE
                                WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                                     REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                                THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                                ELSE COALESCE(pr.total_qty, 0)
                            END
                        ) > 0
                        AND COALESCE(pr.total_qty, 0) < GREATEST(
                            COALESCE(pr.total_qty, 0),
                            CASE
                                WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                                     REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                                THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                                ELSE COALESCE(pr.total_qty, 0)
                            END
                        ) THEN 'partial'
                        ELSE 'accepted'
                    END
                WHEN LOWER(TRIM(COALESCE(pr.status, ''))) = 'draft' THEN 'draft'
                ELSE 'pending'
            END AS status
        FROM `tabPurchase Receipt` pr
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
          AND NOT (pr.docstatus = 0 AND COALESCE(pr.remarks, '') LIKE '%Accord Werka Aytilmagan: pending%')
          AND NOT (pr.docstatus = 1 AND COALESCE(pr.remarks, '') LIKE '%Accord Werka Aytilmagan: approved%')
        UNION ALL
        SELECT
            CASE COALESCE(dn.accord_customer_state, 0)
                WHEN 2 THEN 'rejected'
                WHEN 3 THEN 'accepted'
                WHEN 4 THEN 'partial'
                ELSE 'pending'
            END AS status
        FROM `tabDelivery Note` dn
        WHERE dn.docstatus = 1
          AND dn.accord_flow_state = 1
    ) statuses
"#;

pub(crate) const WERKA_STATUS_BREAKDOWN_PUSHDOWN_SQL: &str = r#"
    WITH records AS (
        SELECT
            0 AS source_order,
            pr.name AS sort_name,
            pr.supplier AS supplier_ref,
            COALESCE(pr.supplier_name, '') AS supplier_name,
            COALESCE(pri.uom, '') AS uom,
            CAST(GREATEST(
                COALESCE(pr.total_qty, 0),
                CASE
                    WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                         REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                    THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                    ELSE COALESCE(pr.total_qty, 0)
                END
            ) AS DOUBLE) AS sent_qty,
            CASE
                WHEN pr.docstatus = 1 AND COALESCE(pr.total_qty, 0) > 0
                THEN CAST(COALESCE(pr.total_qty, 0) AS DOUBLE)
                ELSE CAST(0 AS DOUBLE)
            END AS accepted_qty,
            CASE
                WHEN pr.docstatus = 2 OR LOWER(TRIM(COALESCE(pr.status, ''))) = 'cancelled' THEN 'cancelled'
                WHEN pr.docstatus = 1 THEN
                    CASE
                        WHEN COALESCE(pr.total_qty, 0) <= 0 THEN 'rejected'
                        WHEN GREATEST(
                            COALESCE(pr.total_qty, 0),
                            CASE
                                WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                                     REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                                THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                                ELSE COALESCE(pr.total_qty, 0)
                            END
                        ) > 0
                        AND COALESCE(pr.total_qty, 0) < GREATEST(
                            COALESCE(pr.total_qty, 0),
                            CASE
                                WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                                     REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                                THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                                ELSE COALESCE(pr.total_qty, 0)
                            END
                        ) THEN 'partial'
                        ELSE 'accepted'
                    END
                WHEN LOWER(TRIM(COALESCE(pr.status, ''))) = 'draft' THEN 'draft'
                ELSE 'pending'
            END AS status
        FROM `tabPurchase Receipt` pr
        LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
        UNION ALL
        SELECT
            1 AS source_order,
            dn.name AS sort_name,
            dn.customer AS supplier_ref,
            COALESCE(dn.customer_name, '') AS supplier_name,
            COALESCE(dni.uom, '') AS uom,
            CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS sent_qty,
            CASE
                WHEN COALESCE(dn.accord_customer_state, 0) = 3 THEN CAST(COALESCE(dn.total_qty, 0) AS DOUBLE)
                WHEN COALESCE(dn.accord_customer_state, 0) = 4 THEN CAST(GREATEST(
                    COALESCE(dn.total_qty, 0) -
                    CASE
                        WHEN COALESCE(dni.returned_qty, 0) <= 0 THEN COALESCE(dn.total_qty, 0)
                        ELSE COALESCE(dni.returned_qty, 0)
                    END,
                    0
                ) AS DOUBLE)
                ELSE CAST(0 AS DOUBLE)
            END AS accepted_qty,
            CASE
                WHEN dn.docstatus != 1 THEN 'draft'
                WHEN COALESCE(dn.accord_flow_state, 0) != 1 THEN 'pending'
                WHEN COALESCE(dn.accord_customer_state, 0) = 2 THEN 'rejected'
                WHEN COALESCE(dn.accord_customer_state, 0) = 3 THEN 'accepted'
                WHEN COALESCE(dn.accord_customer_state, 0) = 4 THEN 'partial'
                ELSE 'pending'
            END AS status
        FROM `tabDelivery Note` dn
        LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    ),
    matching_records AS (
        SELECT
            *,
            CASE
                WHEN TRIM(COALESCE(supplier_ref, '')) = '' THEN TRIM(COALESCE(supplier_name, ''))
                ELSE TRIM(COALESCE(supplier_ref, ''))
            END AS group_key
        FROM records
        WHERE
            (? = 'pending' AND status IN ('pending', 'draft'))
            OR (? = 'confirmed' AND status = 'accepted')
            OR (? = 'returned' AND status IN ('partial', 'rejected', 'cancelled'))
    ),
    ranked_records AS (
        SELECT
            *,
            ROW_NUMBER() OVER (PARTITION BY group_key ORDER BY source_order ASC, sort_name DESC) AS group_row_number
        FROM matching_records
    )
    SELECT
        COALESCE(MAX(CASE WHEN group_row_number = 1 THEN supplier_ref END), '') AS supplier_ref,
        COALESCE(MAX(CASE WHEN group_row_number = 1 THEN supplier_name END), '') AS supplier_name,
        CAST(COUNT(*) AS SIGNED) AS receipt_count,
        CAST(COALESCE(SUM(sent_qty), 0) AS DOUBLE) AS total_sent_qty,
        CAST(COALESCE(SUM(accepted_qty), 0) AS DOUBLE) AS total_accepted_qty,
        CAST(COALESCE(SUM(GREATEST(sent_qty - accepted_qty, 0)), 0) AS DOUBLE) AS total_returned_qty,
        COALESCE(MAX(CASE WHEN group_row_number = 1 THEN uom END), '') AS uom
    FROM ranked_records
    GROUP BY group_key
    ORDER BY receipt_count DESC, LOWER(supplier_name) ASC
"#;

pub(crate) const WERKA_STATUS_DETAILS_CONFIRMED_SQL: &str = r#"
    SELECT *
    FROM (
        SELECT
            TRIM(COALESCE(pr.name, '')) AS id,
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
            CAST(COALESCE(pr.total_qty, 0) AS DOUBLE) AS accepted_qty,
            CAST(COALESCE(pri.amount, 0) AS DOUBLE) AS amount,
            TRIM(COALESCE(pr.currency, '')) AS currency,
            'accepted' AS status,
            TRIM(COALESCE(CAST(pr.posting_date AS CHAR), '')) AS created_label
        FROM `tabPurchase Receipt` pr
        LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
          AND pr.docstatus = 1
          AND LOWER(TRIM(COALESCE(pr.status, ''))) != 'cancelled'
          AND COALESCE(pr.total_qty, 0) > 0
          AND COALESCE(pr.total_qty, 0) >= GREATEST(
              COALESCE(pr.total_qty, 0),
              CASE
                  WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                       REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                  THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                  ELSE COALESCE(pr.total_qty, 0)
              END
          )
        UNION ALL
        SELECT
            TRIM(COALESCE(dn.name, '')) AS id,
            'delivery_note' AS record_type,
            TRIM(COALESCE(dn.customer, '')) AS supplier_ref,
            TRIM(COALESCE(dn.customer_name, '')) AS supplier_name,
            TRIM(COALESCE(dni.item_code, '')) AS item_code,
            TRIM(COALESCE(dni.item_name, '')) AS item_name,
            TRIM(COALESCE(dni.uom, '')) AS uom,
            CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS sent_qty,
            CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS accepted_qty,
            CAST(0 AS DOUBLE) AS amount,
            '' AS currency,
            'accepted' AS status,
            TRIM(COALESCE(CAST(dn.modified AS CHAR), '')) AS created_label
        FROM `tabDelivery Note` dn
        LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
        WHERE dn.docstatus = 1
          AND dn.accord_flow_state = 1
          AND dn.accord_customer_state = 3
    ) records
    WHERE (? = '' OR LOWER(TRIM(supplier_ref)) = LOWER(TRIM(?)))
    ORDER BY created_label DESC
"#;

pub(crate) const WERKA_STATUS_DETAILS_RETURNED_SQL: &str = r#"
    SELECT *
    FROM (
        SELECT
            TRIM(COALESCE(pr.name, '')) AS id,
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
            CASE
                WHEN pr.docstatus = 1
                 AND LOWER(TRIM(COALESCE(pr.status, ''))) != 'cancelled'
                 AND COALESCE(pr.total_qty, 0) > 0
                 AND COALESCE(pr.total_qty, 0) < GREATEST(
                    COALESCE(pr.total_qty, 0),
                    CASE
                        WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                             REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                        THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                        ELSE COALESCE(pr.total_qty, 0)
                    END
                 )
                THEN CAST(COALESCE(pr.total_qty, 0) AS DOUBLE)
                ELSE CAST(0 AS DOUBLE)
            END AS accepted_qty,
            CAST(COALESCE(pri.amount, 0) AS DOUBLE) AS amount,
            TRIM(COALESCE(pr.currency, '')) AS currency,
            CASE
                WHEN pr.docstatus = 2 OR LOWER(TRIM(COALESCE(pr.status, ''))) = 'cancelled' THEN 'cancelled'
                WHEN COALESCE(pr.total_qty, 0) <= 0 THEN 'rejected'
                ELSE 'partial'
            END AS status,
            TRIM(COALESCE(CAST(pr.posting_date AS CHAR), '')) AS created_label
        FROM `tabPurchase Receipt` pr
        LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
          AND (
              pr.docstatus = 2
              OR LOWER(TRIM(COALESCE(pr.status, ''))) = 'cancelled'
              OR (
                  pr.docstatus = 1
                  AND (
                      COALESCE(pr.total_qty, 0) <= 0
                      OR COALESCE(pr.total_qty, 0) < GREATEST(
                          COALESCE(pr.total_qty, 0),
                          CASE
                              WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                                   REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                              THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                              ELSE COALESCE(pr.total_qty, 0)
                          END
                      )
                  )
              )
          )
        UNION ALL
        SELECT
            TRIM(COALESCE(dn.name, '')) AS id,
            'delivery_note' AS record_type,
            TRIM(COALESCE(dn.customer, '')) AS supplier_ref,
            TRIM(COALESCE(dn.customer_name, '')) AS supplier_name,
            TRIM(COALESCE(dni.item_code, '')) AS item_code,
            TRIM(COALESCE(dni.item_name, '')) AS item_name,
            TRIM(COALESCE(dni.uom, '')) AS uom,
            CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS sent_qty,
            CASE
                WHEN COALESCE(dn.accord_customer_state, 0) = 4 THEN CAST(GREATEST(
                    COALESCE(dn.total_qty, 0) -
                    CASE
                        WHEN COALESCE(dni.returned_qty, 0) <= 0 THEN GREATEST(COALESCE(dn.total_qty, 0), 0)
                        ELSE COALESCE(dni.returned_qty, 0)
                    END,
                    0
                ) AS DOUBLE)
                ELSE CAST(0 AS DOUBLE)
            END AS accepted_qty,
            CAST(0 AS DOUBLE) AS amount,
            '' AS currency,
            CASE
                WHEN COALESCE(dn.accord_customer_state, 0) = 4 THEN 'partial'
                ELSE 'rejected'
            END AS status,
            TRIM(COALESCE(CAST(dn.modified AS CHAR), '')) AS created_label
        FROM `tabDelivery Note` dn
        LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
        WHERE dn.docstatus = 1
          AND dn.accord_flow_state = 1
          AND dn.accord_customer_state IN (2, 4)
    )
    records
    WHERE (? = '' OR LOWER(TRIM(supplier_ref)) = LOWER(TRIM(?)))
    ORDER BY created_label DESC
"#;

pub(crate) const SUPPLIER_ACK_ROWS_LIMIT_SQL: &str = r#"
    SELECT
        c.name AS comment_id,
        COALESCE(CAST(c.creation AS CHAR), '') AS created_label,
        pr.supplier AS supplier_ref,
        COALESCE(pr.supplier_name, '') AS supplier_name,
        CAST(COALESCE(pr.total_qty, 0) AS DOUBLE) AS sent_qty,
        COALESCE(pri.item_code, '') AS item_code,
        COALESCE(pri.item_name, '') AS item_name,
        COALESCE(pri.uom, '') AS uom
    FROM `tabComment` c
    INNER JOIN `tabPurchase Receipt` pr ON pr.name = c.reference_name
    LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
    WHERE c.reference_doctype = 'Purchase Receipt'
      AND c.content LIKE 'Supplier%'
      AND c.content LIKE '%Tasdiqlayman%'
    ORDER BY c.name DESC
    LIMIT ?
"#;
