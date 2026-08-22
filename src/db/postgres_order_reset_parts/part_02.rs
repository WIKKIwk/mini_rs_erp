
async fn create_reset_targets(tx: &mut Transaction<'_, Postgres>) -> Result<(), OrderResetError> {
    for statement in [
        "CREATE TEMP TABLE reset_order_ids (
             id TEXT PRIMARY KEY
         ) ON COMMIT DROP",
        "CREATE TEMP TABLE reset_map_ids (
             id TEXT PRIMARY KEY
         ) ON COMMIT DROP",
        "CREATE TEMP TABLE reset_progress_batch_ids (
             batch_id TEXT PRIMARY KEY
         ) ON COMMIT DROP",
        "CREATE TEMP TABLE reset_paddon_ids (
             paddon_id TEXT PRIMARY KEY
         ) ON COMMIT DROP",
        "CREATE TEMP TABLE reset_raw_barcodes (
             barcode TEXT PRIMARY KEY
         ) ON COMMIT DROP",
        "CREATE TEMP TABLE reset_qolip_codes (
             qolip_code TEXT PRIMARY KEY
         ) ON COMMIT DROP",
    ] {
        sqlx::query(statement)
            .execute(&mut **tx)
            .await
            .map_err(OrderResetError::StoreFailed)?;
    }

    sqlx::query(
        "INSERT INTO reset_order_ids (id)
         SELECT btrim(id) FROM mini_orders WHERE btrim(id) <> ''
         UNION
         SELECT btrim(id) FROM mini_production_maps WHERE btrim(id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_production_maps
          WHERE btrim(COALESCE(order_id, '')) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_queue_states WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_queue_action_events WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_order_run_sessions WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_order_progress_events WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_progress_batches WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_raw_material_assignments WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_raw_material_events
          WHERE btrim(COALESCE(order_id, '')) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_qolip_order_notes WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_returned_paint_requests WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_returned_paint_images WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_apparatus_order_transfers WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_apparatus_schedule_reservations WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_laminatsiya_astatka_reports WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_rezka_astatka_reports WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_finished_goods_stock WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_order_control_states WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(order_id) FROM mini_order_freeze_requests WHERE btrim(order_id) <> ''
         UNION
         SELECT btrim(reserved_order_id) FROM mini_raw_material_stock
          WHERE btrim(reserved_order_id) <> ''
         ON CONFLICT (id) DO NOTHING",
    )
    .execute(&mut **tx)
    .await
    .map_err(OrderResetError::StoreFailed)?;

    sqlx::query(
        "INSERT INTO reset_map_ids (id)
         SELECT btrim(id) FROM mini_production_maps WHERE btrim(id) <> ''
         ON CONFLICT (id) DO NOTHING",
    )
    .execute(&mut **tx)
    .await
    .map_err(OrderResetError::StoreFailed)?;

    sqlx::query(
        "INSERT INTO reset_progress_batch_ids (batch_id)
         SELECT btrim(batch_id)
         FROM mini_progress_batches
         WHERE btrim(batch_id) <> ''
           AND lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)
         ON CONFLICT (batch_id) DO NOTHING",
    )
    .execute(&mut **tx)
    .await
    .map_err(OrderResetError::StoreFailed)?;

    sqlx::query(
        "INSERT INTO reset_paddon_ids (paddon_id)
         SELECT DISTINCT btrim(paddon_id)
         FROM mini_paddon_items
         WHERE progress_batch_id IN (SELECT batch_id FROM reset_progress_batch_ids)
           AND btrim(paddon_id) <> ''
         ON CONFLICT (paddon_id) DO NOTHING",
    )
    .execute(&mut **tx)
    .await
    .map_err(OrderResetError::StoreFailed)?;

    sqlx::query(
        "INSERT INTO reset_raw_barcodes (barcode)
         SELECT lower(btrim(barcode))
         FROM mini_raw_material_assignments
         WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)
         UNION
         SELECT lower(btrim(barcode))
         FROM mini_raw_material_events
         WHERE lower(order_id) IN (SELECT lower(id) FROM reset_order_ids)
           AND btrim(barcode) <> ''
         UNION
         SELECT lower(btrim(barcode))
         FROM mini_raw_material_stock
         WHERE lower(reserved_order_id) IN (SELECT lower(id) FROM reset_order_ids)
           AND btrim(barcode) <> ''
         ON CONFLICT (barcode) DO NOTHING",
    )
    .execute(&mut **tx)
    .await
    .map_err(OrderResetError::StoreFailed)?;

    sqlx::query(
        "INSERT INTO reset_qolip_codes (qolip_code)
         SELECT lower(btrim(code))
         FROM mini_qolip_order_notes note
         CROSS JOIN LATERAL unnest(note.qolip_codes) AS codes(code)
         WHERE lower(note.order_id) IN (SELECT lower(id) FROM reset_order_ids)
           AND btrim(code) <> ''
         ON CONFLICT (qolip_code) DO NOTHING",
    )
    .execute(&mut **tx)
    .await
    .map_err(OrderResetError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO reset_qolip_codes (qolip_code)
         SELECT lower(btrim(session.payload_json->>'qolip_code'))
         FROM mini_order_run_sessions session
         WHERE lower(session.order_id) IN (SELECT lower(id) FROM reset_order_ids)
           AND btrim(COALESCE(session.payload_json->>'qolip_code', '')) <> ''
         ON CONFLICT (qolip_code) DO NOTHING",
    )
    .execute(&mut **tx)
    .await
    .map_err(OrderResetError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO reset_qolip_codes (qolip_code)
         SELECT lower(btrim(code.value))
         FROM mini_order_run_sessions session
         CROSS JOIN LATERAL jsonb_array_elements_text(
             CASE
                 WHEN jsonb_typeof(session.payload_json->'qolip_codes') = 'array'
                 THEN session.payload_json->'qolip_codes'
                 ELSE '[]'::jsonb
             END
         ) AS code(value)
         WHERE lower(session.order_id) IN (SELECT lower(id) FROM reset_order_ids)
           AND btrim(code.value) <> ''
         ON CONFLICT (qolip_code) DO NOTHING",
    )
    .execute(&mut **tx)
    .await
    .map_err(OrderResetError::StoreFailed)?;

    Ok(())
}

async fn count_rows(
    tx: &mut Transaction<'_, Postgres>,
    statement: &str,
) -> Result<u64, OrderResetError> {
    let count = sqlx::query_scalar::<_, i64>(statement)
        .fetch_one(&mut **tx)
        .await
        .map_err(OrderResetError::StoreFailed)?;
    u64::try_from(count).map_err(|_| OrderResetError::VerificationFailed)
}

async fn delete_rows(
    tx: &mut Transaction<'_, Postgres>,
    statement: &str,
) -> Result<u64, OrderResetError> {
    Ok(sqlx::query(statement)
        .execute(&mut **tx)
        .await
        .map_err(OrderResetError::StoreFailed)?
        .rows_affected())
}
