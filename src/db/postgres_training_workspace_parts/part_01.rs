
#[derive(Debug, Error)]
pub enum TrainingWorkspaceError {
    #[error("store failed")]
    StoreFailed,
    #[error("training map not found")]
    MapNotFound,
    #[error("training order number already exists")]
    DuplicateOrderNumber,
    #[error("training raw material assignment already exists")]
    DuplicateRawMaterialAssignment,
    #[error("invalid training input: {0}")]
    InvalidInput(String),
    #[error("invalid training map: {0}")]
    InvalidMap(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct TrainingProductionMapSaveWithOrder {
    pub saved: ProductionMapSaved,
    pub template: Option<CalculateOrderTemplate>,
}

#[derive(Debug, Clone)]
pub struct TrainingImage {
    pub image_id: String,
    pub image_name: String,
    pub image_mime: String,
    pub image_size_bytes: u64,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrainingQueueStateRecord {
    pub apparatus: String,
    pub order_id: String,
    pub state: String,
    pub updated_at_unix: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrainingQueueEvent {
    pub event_id: String,
    pub apparatus: String,
    pub order_id: String,
    pub action: String,
    pub from_state: String,
    pub to_state: String,
    pub actor_ref: String,
    pub actor_display_name: String,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainingInputBatchIdentity {
    pub order_id: String,
    pub apparatus: String,
    pub batch_id: String,
    pub session_id: String,
    pub qr_payload: String,
}

pub const TRAINING_VIRTUAL_INPUT_BOSMA: &str = "training-input:bosma";
pub const TRAINING_VIRTUAL_INPUT_LAMINATSIYA: &str = "training-input:laminatsiya";

#[derive(Clone)]
pub struct PostgresTrainingWorkspaceStore {
    pool: PgPool,
}

include!("../postgres_training_workspace_impl_parts/part_01.rs");
include!("../postgres_training_workspace_impl_parts/part_02.rs");
include!("../postgres_training_workspace_impl_parts/part_03.rs");

#[derive(sqlx::FromRow)]
struct TrainingRawMaterialRow {
    order_id: String,
    apparatus: String,
    #[allow(dead_code)]
    barcode: String,
    payload_json: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct TrainingImageRow {
    image_id: String,
    image_name: String,
    image_mime: String,
    image_size_bytes: i64,
    body: Vec<u8>,
}

async fn prepare_map_for_save(
    tx: &mut Transaction<'_, Postgres>,
    mut map: ProductionMapDefinition,
) -> Result<ProductionMapDefinition, TrainingWorkspaceError> {
    if map.order_number.trim().is_empty() {
        let next = sqlx::query_scalar::<_, i64>("SELECT nextval('mini_training_order_number_seq')")
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        let order_number = format!("T-{next:04}");
        map.order_number = order_number.clone();
        if map.code.trim().is_empty() {
            map.code = order_number;
        }
        if map.id.trim().is_empty() || map.id.starts_with("zakaz-draft-") {
            map.id = format!("training-zakaz-{next:04}");
        }
    }
    if map.id.trim().is_empty() {
        map.id = format!("training-zakaz-{}", unix_micros());
    }
    if map.code.trim().is_empty() {
        map.code = map.order_number.trim().to_string();
    }
    normalize_training_map_apparatus_ids(&mut map)?;

    let duplicate = sqlx::query_scalar::<_, String>(
        "SELECT id
         FROM mini_training_production_maps
         WHERE order_number = $1 AND id <> $2
         LIMIT 1",
    )
    .bind(map.order_number.trim())
    .bind(map.id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    if duplicate.is_some() {
        return Err(TrainingWorkspaceError::DuplicateOrderNumber);
    }
    Ok(map)
}

async fn save_map_tx(
    tx: &mut Transaction<'_, Postgres>,
    map: &ProductionMapDefinition,
) -> Result<(), TrainingWorkspaceError> {
    let payload = serde_json::to_value(map).map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    let program =
        compile_map(map).map_err(|error| TrainingWorkspaceError::InvalidMap(error.to_string()))?;
    validate_training_apparatus_ids(tx, &program).await?;
    sqlx::query(
        "INSERT INTO mini_training_production_maps
            (id, order_number, map_json, updated_at)
         VALUES ($1, $2, $3, now())
         ON CONFLICT (id) DO UPDATE SET
             order_number = excluded.order_number,
             map_json = excluded.map_json,
             updated_at = now()",
    )
    .bind(map.id.trim())
    .bind(map.order_number.trim())
    .bind(payload)
    .execute(&mut **tx)
    .await
    .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    Ok(())
}

async fn validate_training_apparatus_ids(
    tx: &mut Transaction<'_, Postgres>,
    program: &ProductionMapProgram,
) -> Result<(), TrainingWorkspaceError> {
    let mut requested = BTreeSet::new();
    for operation in program
        .operations
        .iter()
        .filter(|operation| operation.op_code == "apparatus")
    {
        for key in ["apparatus_id", "alternative_assigned_apparatus_id"] {
            let Some(value) = operation.args.get(key) else {
                continue;
            };
            if value.is_empty() {
                continue;
            }
            let id = ApparatusId::new(value.clone()).map_err(|_| {
                TrainingWorkspaceError::InvalidMap(format!(
                    "{key} must be an exact canonical apparatus id"
                ))
            })?;
            if id.as_str() != value {
                return Err(TrainingWorkspaceError::InvalidMap(format!(
                    "{key} must be an exact canonical apparatus id"
                )));
            }
            requested.insert(id.to_string());
        }
    }
    if requested.is_empty() {
        return Ok(());
    }

    let requested_ids = requested.iter().cloned().collect::<Vec<_>>();
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT id
         FROM mini_apparatus
         WHERE id = ANY($1)
         FOR KEY SHARE",
    )
    .bind(&requested_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| TrainingWorkspaceError::StoreFailed)?
    .into_iter()
    .collect::<BTreeSet<_>>();

    if existing != requested {
        let missing = requested.difference(&existing).cloned().collect::<Vec<_>>();
        return Err(TrainingWorkspaceError::InvalidMap(format!(
            "apparatus id(s) missing from mini_apparatus: {}",
            missing.join(", ")
        )));
    }
    Ok(())
}

fn prepare_template_for_save(
    mut template: CalculateOrderTemplate,
    map: &ProductionMapDefinition,
) -> CalculateOrderTemplate {
    template = hydrate_template_layers(template);
    if template.id.trim().is_empty() || !template.id.starts_with("training-") {
        template.id = format!("training-template-{}", unix_micros());
    }
    if template.code.trim().is_empty() {
        template.code = format!("TR-{}", map.order_number.trim());
    }
    template.order_number = map.order_number.trim().to_string();
    if template.source_map_id.trim().is_empty() {
        template.source_map_id = map.id.trim().to_string();
    }
    template.saved_at = unix_micros().to_string();
    template
}

async fn save_template_tx(
    tx: &mut Transaction<'_, Postgres>,
    template: &CalculateOrderTemplate,
    owner_key: &str,
) -> Result<(), TrainingWorkspaceError> {
    let payload =
        serde_json::to_value(template).map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO mini_training_quick_order_templates
            (id, owner_key, code, payload_json, saved_at)
         VALUES ($1, $2, $3, $4, now())
         ON CONFLICT (id) DO UPDATE SET
             owner_key = excluded.owner_key,
             code = excluded.code,
             payload_json = excluded.payload_json,
             saved_at = now()",
    )
    .bind(template.id.trim())
    .bind(owner_key.trim())
    .bind(template.code.trim())
    .bind(payload)
    .execute(&mut **tx)
    .await
    .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    Ok(())
}

fn saved_map_from_payload(
    payload: serde_json::Value,
) -> Result<ProductionMapSaved, TrainingWorkspaceError> {
    let map = serde_json::from_value::<ProductionMapDefinition>(payload)
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    saved_map_from_definition(map)
}

fn saved_map_from_definition(
    mut map: ProductionMapDefinition,
) -> Result<ProductionMapSaved, TrainingWorkspaceError> {
    normalize_training_map_apparatus_ids(&mut map)?;
    if map.code.trim().is_empty() && !map.order_number.trim().is_empty() {
        map.code = map.order_number.trim().to_string();
    }
    let program =
        compile_map(&map).map_err(|error| TrainingWorkspaceError::InvalidMap(error.to_string()))?;
    Ok(ProductionMapSaved { map, program })
}

fn normalize_training_map_apparatus_ids(
    map: &mut ProductionMapDefinition,
) -> Result<(), TrainingWorkspaceError> {
    for node in &mut map.nodes {
        if node.kind != ProductionMapNodeKind::Apparatus {
            continue;
        }

        node.apparatus_id =
            normalize_training_map_apparatus_id(&node.apparatus_id, "apparatus_id")?;
        node.alternative_assigned_apparatus_id = normalize_training_map_apparatus_id(
            &node.alternative_assigned_apparatus_id,
            "alternative_assigned_apparatus_id",
        )?;
    }
    Ok(())
}

fn normalize_training_map_apparatus_id(
    value: &str,
    field: &str,
) -> Result<String, TrainingWorkspaceError> {
    if value.trim().is_empty() {
        return Ok(String::new());
    }
    canonical_training_apparatus(value)
        .map(|id| id.to_string())
        .map_err(|_| {
            TrainingWorkspaceError::InvalidMap(format!(
                "{field} must be an exact canonical apparatus id"
            ))
        })
}

fn payload_string(payload: &serde_json::Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn canonical_training_apparatus(value: &str) -> Result<ApparatusId, TrainingWorkspaceError> {
    ApparatusId::new(value.trim().to_string()).map_err(|_| {
        TrainingWorkspaceError::InvalidInput("canonical apparatus id kerak".to_string())
    })
}

fn training_virtual_input_id(value: &str) -> Result<String, TrainingWorkspaceError> {
    let value = value.trim();
    if value.eq_ignore_ascii_case(TRAINING_VIRTUAL_INPUT_BOSMA) {
        Ok(TRAINING_VIRTUAL_INPUT_BOSMA.to_string())
    } else if value.eq_ignore_ascii_case(TRAINING_VIRTUAL_INPUT_LAMINATSIYA) {
        Ok(TRAINING_VIRTUAL_INPUT_LAMINATSIYA.to_string())
    } else {
        Err(TrainingWorkspaceError::InvalidInput(
            "training virtual input kerak".to_string(),
        ))
    }
}

fn training_queue_event_from_row(
    (
        event_id,
        apparatus,
        order_id,
        action,
        from_state,
        to_state,
        actor_ref,
        actor_display_name,
        created_at_unix,
    ): (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
    ),
) -> Result<TrainingQueueEvent, TrainingWorkspaceError> {
    Ok(TrainingQueueEvent {
        event_id,
        apparatus: canonical_training_apparatus(&apparatus)?.to_string(),
        order_id,
        action,
        from_state,
        to_state,
        actor_ref,
        actor_display_name,
        created_at_unix,
    })
}

fn training_input_batch_identity_from_row(
    (order_id, apparatus, batch_id, session_id, qr_payload): (
        String,
        String,
        String,
        String,
        String,
    ),
) -> Result<TrainingInputBatchIdentity, TrainingWorkspaceError> {
    Ok(TrainingInputBatchIdentity {
        order_id,
        apparatus: canonical_training_apparatus(&apparatus)?.to_string(),
        batch_id,
        session_id,
        qr_payload,
    })
}

fn training_progress_payload(
    batch: &OrderProgressBatch,
) -> Result<serde_json::Value, TrainingWorkspaceError> {
    let apparatus = canonical_training_apparatus(&batch.apparatus)?;
    let mut payload =
        serde_json::to_value(batch).map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    let object = payload
        .as_object_mut()
        .ok_or(TrainingWorkspaceError::StoreFailed)?;
    object.insert(
        "apparatus".to_string(),
        serde_json::Value::String(apparatus.to_string()),
    );
    for (field, value) in [
        (
            "current_apparatus_key",
            batch.current_apparatus_key.as_str(),
        ),
        ("current_apparatus", batch.current_apparatus.as_str()),
        ("next_apparatus", batch.next_apparatus.as_str()),
        ("used_by_apparatus", batch.used_by_apparatus.as_str()),
        (
            "processed_by_apparatus",
            batch.processed_by_apparatus.as_str(),
        ),
    ] {
        let value = if value.trim().is_empty() {
            String::new()
        } else {
            canonical_training_apparatus(value)?.to_string()
        };
        object.insert(field.to_string(), serde_json::Value::String(value));
    }
    Ok(payload)
}

fn training_progress_batch_from_row(
    (canonical_apparatus_id, mut payload): (String, serde_json::Value),
) -> Result<OrderProgressBatch, TrainingWorkspaceError> {
    let canonical_apparatus_id = canonical_training_apparatus(&canonical_apparatus_id)?;
    let object = payload
        .as_object_mut()
        .ok_or(TrainingWorkspaceError::StoreFailed)?;
    object.insert(
        "apparatus".to_string(),
        serde_json::Value::String(canonical_apparatus_id.to_string()),
    );
    training_progress_batch_from_payload(payload)
}
