/// Link a received stock row to preparation allocations without posting another stock receipt.
/// Callers hold the stock row lock, including the fallback for transfers received before migration.
pub(crate) async fn received_preparation_lot(
    tx: &mut Transaction<'_, Postgres>,
    master: &Principal,
    stock_id: &str,
    warehouse: &str,
) -> Result<Option<String>, PreparationError> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT item_code,item_name,qty::text FROM mini_raw_material_stock
         WHERE id=$1 AND warehouse=$2 AND payload_json->>'source'='raw_material_paddon' AND status='available'")
        .bind(stock_id).bind(warehouse).fetch_optional(&mut **tx).await?;
    let Some((code, name, kg)) = row else {
        return Ok(None);
    };
    if let Some(id) =
        sqlx::query_scalar("SELECT id FROM mini_preparation_receipts WHERE stock_id=$1")
            .bind(stock_id)
            .fetch_optional(&mut **tx)
            .await?
    {
        return Ok(Some(id));
    }
    let id = new_id();
    let output = json!({"id":id,"kind":"receipt","warehouse":warehouse,"item_code":code,"name":name,"kg":kg});
    record(
        tx,
        &id,
        &master.ref_,
        &format!("paddon-transfer-{id}"),
        "receipt",
        None,
        json!({"kind":"paddon_transfer","stock_id":stock_id}),
        &output,
    )
    .await?;
    sqlx::query(
        "INSERT INTO mini_preparation_receipts (id,stock_id,item_code,owner_ref,initial_kg)
        VALUES ($1,$2,$3,$4,$5::text::numeric)",
    )
    .bind(&id)
    .bind(stock_id)
    .bind(code)
    .bind(&master.ref_)
    .bind(kg)
    .execute(&mut **tx)
    .await?;
    Ok(Some(id))
}

enum PaddonEvent {
    Receipt { packed: bool },
    Weighed { previous: i64, packed: bool },
    SplitOut,
    SplitIn,
}

async fn record_paddon_event(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Principal,
    warehouse: &str,
    material: &PaddonMaterial,
    barcode: &str,
    difference: i64,
    event: PaddonEvent,
    source_id: &str,
    paddon_id: &str,
) -> Result<(), PreparationError> {
    if difference == 0 {
        return Ok(());
    }
    let role = role_code(actor)?;
    let (event_type, before, after, source, previous_kg) = match event {
        PaddonEvent::Receipt { packed } => (
            "receipt_posted",
            None,
            if packed { "on_paddon" } else { "available" },
            "system",
            None,
        ),
        PaddonEvent::Weighed { previous, packed } => (
            if difference > 0 {
                "adjustment_increase"
            } else {
                "adjustment_decrease"
            },
            Some("on_paddon"),
            if packed { "on_paddon" } else { "available" },
            "system",
            Some(previous),
        ),
        PaddonEvent::SplitOut => (
            "split_consumed",
            Some("on_paddon"),
            "on_paddon",
            "raw_material_split",
            None,
        ),
        PaddonEvent::SplitIn => (
            "split_produced",
            None,
            "available",
            "raw_material_split",
            None,
        ),
    };
    insert_raw_material_event_tx(
        tx,
        RawMaterialEventDraft {
            idempotency_key: format!("paddon:{source_id}:{barcode}:{event_type}"),
            event_type: event_type.into(),
            warehouse: warehouse.into(),
            barcode: barcode.into(),
            item_code: material.item_code.clone(),
            item_name: material.item_name.clone(),
            qty_delta: difference as f64 / SCALE as f64,
            uom: "kg".into(),
            stock_status_before: before.map(str::to_owned),
            stock_status_after: Some(after.into()),
            order_id: None,
            apparatus: None,
            actor_role: role.into(),
            actor_ref: actor.ref_.clone(),
            actor_display_name: actor.display_name.clone(),
            owner_role: role.into(),
            owner_ref: actor.ref_.clone(),
            owner_display_name: actor.display_name.clone(),
            source_type: source.into(),
            source_id: source_id.into(),
            source_line_ref: None,
            correlation_id: Some(paddon_id.into()),
            payload_json: json!({"source":"raw_material_paddon","paddon_id":paddon_id,
                                 "lot_number":material.lot_number,
                                 "previous_kg":previous_kg.map(decimal_text)}),
        },
    )
    .await?;
    Ok(())
}
