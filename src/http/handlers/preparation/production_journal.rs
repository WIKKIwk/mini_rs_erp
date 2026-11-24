use super::*;
use crate::core::apparatus_standard::LifecycleState;

#[derive(Debug, Default, Deserialize)]
pub struct ProductionJournalQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub order_id: String,
    #[serde(default)]
    pub batch_id: String,
    #[serde(default)]
    pub offset: usize,
    pub limit: Option<usize>,
}

impl ProductionJournalQuery {
    fn status(&self) -> &str {
        if self.status.is_empty() { "all" } else { &self.status }
    }

    fn validate(&self) -> Result<(), ApiError> {
        if self.q.chars().count() > 200
            || self.order_id.len() > 512
            || self.batch_id.len() > 1024
            || self.offset > 1_000_000
            || self.limit.is_some_and(|limit| limit == 0 || limit > 100)
            || !matches!(self.status.as_str(), "" | "all" | "active" | "completed")
        {
            return Err(error(PreparationError::Invalid("Jurnal filtri noto‘g‘ri")));
        }
        Ok(())
    }
}

pub async fn production_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProductionJournalQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    query.validate()?;
    store(&state)?
        .production_journal_orders(
            &actor.ref_,
            &query.q,
            query.status(),
            query.offset,
            query.limit.unwrap_or(50),
        )
        .await
        .map(Json)
        .map_err(error)
}

pub async fn production_batches(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProductionJournalQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    query.validate()?;
    store(&state)?.production_journal_batches(
        &actor.ref_, &query.q, query.status(), query.offset, query.limit.unwrap_or(50),
    ).await.map(Json).map_err(error)
}

pub async fn production_rolls(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProductionJournalQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    query.validate()?;
    store(&state)?.production_journal_rolls(
        &actor.ref_, &query.q, query.status(), query.offset, query.limit.unwrap_or(50),
    ).await.map(Json).map_err(error)
}

pub async fn production_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProductionJournalQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    query.validate()?;
    if query.order_id.trim().is_empty() {
        return Err(error(PreparationError::Invalid("Buyurtma tanlanmagan")));
    }
    store(&state)?
        .production_journal_order(
            &actor.ref_,
            query.order_id.trim(),
            query.offset,
            query.limit.unwrap_or(30),
        )
        .await
        .map_err(error)?
        .map(Json)
        .ok_or_else(not_found)
}

pub async fn production_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProductionJournalQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    query.validate()?;
    if query.batch_id.trim().is_empty() {
        return Err(error(PreparationError::Invalid(
            "Ishlab chiqarish yozuvi tanlanmagan",
        )));
    }
    store(&state)?
        .production_journal_batch(
            &actor.ref_,
            query.batch_id.trim(),
            query.offset,
            query.limit.unwrap_or(50),
        )
        .await
        .map_err(error)?
        .map(Json)
        .ok_or_else(not_found)
}

fn not_found() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(
            json!({"code":"preparation_production_not_found","error":"Ishlab chiqarish yozuvi topilmadi"}),
        ),
    )
}

pub async fn update_production_note(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PreparationProductionNoteUpdate>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    input.validate().map_err(error)?;
    let note = store(&state)?.update_production_order_note(&actor, &input)
        .await.map_err(error)?.ok_or_else(not_found)?;
    state.warehouse_events.invalidate_inventory();
    Ok(Json(json!({"preparation_note":note})))
}

#[derive(Deserialize)]
pub struct ScannedRollQuery {
    barcode: String,
}

pub async fn scanned_roll_notes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ScannedRollQuery>,
) -> Result<Json<Value>, ApiError> {
    let token = super::super::auth::bearer_token(&headers).unwrap_or_default();
    let actor = state.sessions.get(&token).await.map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "unauthorized"})),
        )
    })?;
    let forbidden = || (StatusCode::FORBIDDEN, Json(json!({"error": "forbidden"})));
    let viewer = PreparationNoteViewerRole::for_principal(actor.role).ok_or_else(forbidden)?;
    let capabilities = state.admin.principal_capability_codes(&actor).await;
    if !roll_scan_capabilities(actor.role)
        .iter()
        .any(|capability| capabilities.iter().any(|code| code == capability))
    {
        return Err(forbidden());
    }

    let barcode = query.barcode.trim();
    if barcode.is_empty() || barcode.len() > 1024 || barcode.contains('\0') {
        return Err(error(PreparationError::Invalid("Rulon QR kodi noto‘g‘ri")));
    }

    let (batch_note, roll_note) = match state.preparation.as_ref() {
        Some(store) => {
            let viewer = if viewer == PreparationNoteViewerRole::Aparatchi
                && has_rezka_assignment(&state, &actor).await?
            {
                PreparationNoteViewerRole::RezkaWorker
            } else {
                viewer
            };
            match viewer {
                PreparationNoteViewerRole::RezkaMaster
                | PreparationNoteViewerRole::HomashyoRezkachi
                | PreparationNoteViewerRole::RezkaWorker => {
                    store.output_notes_for_barcode(barcode, viewer, &actor.ref_).await.map_err(error)?
                }
                _ => (None, None),
            }
        }
        None => (None, None),
    };
    let scan_note = |note: Value| json!({"text": note["text"], "revision": note["revision"]});
    Ok(Json(json!({
        "barcode": barcode,
        "batch_note": batch_note.map(scan_note),
        "roll_note": roll_note.map(scan_note),
    })))
}

async fn has_rezka_assignment(state: &AppState, actor: &Principal) -> Result<bool, ApiError> {
    let Some(assignment) = state.admin.principal_assignment(actor).await
        .map_err(|_| error(PreparationError::StoreFailed))?
    else {
        return Ok(false);
    };
    if assignment.assigned_apparatus.is_empty() {
        return Ok(false);
    }
    let rezka = state.rezka_apparatus().await.map_err(|_| error(PreparationError::StoreFailed))?;
    Ok(rezka.iter().any(|apparatus| {
        apparatus.lifecycle.state == LifecycleState::Active
            && assignment.assigned_apparatus.iter().any(|id| id == apparatus.apparatus_id.as_ref())
    }))
}

fn roll_scan_capabilities(role: PrincipalRole) -> &'static [&'static str] {
    match role {
        PrincipalRole::RezkaMaster => &["rezka.master.access"],
        PrincipalRole::HomashyoRezkachi => &["raw_material.split"],
        PrincipalRole::Aparatchi => &[
            "apparatus.queue.read",
            "apparatus.queue.manage",
            "inventory.movement.manage",
        ],
        PrincipalRole::TayyorlovMasteri => &["preparation.access"],
        PrincipalRole::Metalayzer => &["metalayzer.access"],
        PrincipalRole::Werka => &["werka.access", "inventory.movement.manage"],
        PrincipalRole::MaterialTaminotchi => &[
            "raw_material.assign",
            "inventory.movement.manage",
        ],
        PrincipalRole::Admin => &[
            "admin.access",
            "production.map.manage",
            "inventory.movement.manage",
        ],
        PrincipalRole::Brigader => &["brigader.access"],
        PrincipalRole::Qolipchi => &["inventory.movement.manage"],
        _ => &[],
    }
}

pub async fn update_output_notes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PreparationOutputNoteUpdate>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let notes = store(&state)?.update_output_notes(&actor, &input)
        .await.map_err(error)?.ok_or_else(not_found)?;
    state.warehouse_events.invalidate_inventory();
    Ok(Json(json!({"notes":notes})))
}
