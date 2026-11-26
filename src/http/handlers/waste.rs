use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    app::AppState,
    core::{
        auth::models::{Principal, PrincipalRole},
        authz::Capability,
        production_map::{QueueActionActor, queue_state::apparatus_matches_assigned},
    },
    db::postgres_waste_bins::{PostgresWasteBinStore, WasteBinError, WasteEntry},
};

type ApiError = (StatusCode, Json<Value>);
type ApiResponse = ([(header::HeaderName, &'static str); 1], Json<Value>);

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/mobile/waste/bins", get(list))
        .route("/v1/mobile/waste/bins/active", get(active))
        .route("/v1/mobile/waste/bins/detail", get(detail))
        .route("/v1/mobile/waste/bins/history", get(history))
        .route("/v1/mobile/waste/bins/register", post(register))
        .route("/v1/mobile/waste/bins/assign", post(assign))
        .route("/v1/mobile/waste/bins/submit", post(submit))
        .route("/v1/mobile/waste/bins/entry", post(add))
        .route("/v1/mobile/waste/bins/reconcile", post(reconcile))
        .route("/v1/mobile/waste/warehouses", get(warehouses))
        .route("/v1/mobile/waste/bins/receive", post(receive))
        .route("/v1/mobile/waste/bins/destination", post(record_destination))
        .route("/v1/mobile/waste/bins/reset", post(reset))
}

fn error(status: StatusCode, code: &'static str, message: &'static str) -> ApiError {
    (status, Json(json!({"code": code, "error": message})))
}

fn forbidden() -> ApiError {
    error(
        StatusCode::FORBIDDEN,
        "forbidden",
        "Bu amal uchun ruxsat yo‘q",
    )
}

fn domain_error(err: WasteBinError) -> ApiError {
    let (status, message) = match err {
        WasteBinError::InvalidInput => (StatusCode::BAD_REQUEST, "Chiqindi ma’lumotini tekshiring"),
        WasteBinError::NotFound => (
            StatusCode::NOT_FOUND,
            "Chiqindi qutisi yoki partiyasi topilmadi",
        ),
        WasteBinError::Conflict => (
            StatusCode::CONFLICT,
            "Quti yoki partiya holati o‘zgargan. Yangilang",
        ),
        WasteBinError::Forbidden => (StatusCode::FORBIDDEN, "Bu amal uchun ruxsat yo‘q"),
        WasteBinError::DiscrepancyNoteRequired => (StatusCode::CONFLICT, "Tafovut sababini yozing"),
        WasteBinError::WarehouseRequired => (StatusCode::BAD_REQUEST, "Chiqindi omborini tanlang"),
        WasteBinError::WarehouseNotAssigned => (
            StatusCode::FORBIDDEN,
            "Bu chiqindi ombori sizga biriktirilmagan",
        ),
        WasteBinError::StoreFailed => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Chiqindi ma’lumoti saqlanmadi. Qayta tekshiring",
        ),
    };
    error(status, err.code(), message)
}

fn result(value: Result<Value, WasteBinError>) -> Result<ApiResponse, ApiError> {
    value
        .map(|value| ([(header::CACHE_CONTROL, "no-store")], Json(value)))
        .map_err(domain_error)
}

fn warehouse_credit(
    state: &AppState,
    value: Result<Value, WasteBinError>,
) -> Result<ApiResponse, ApiError> {
    let snapshot = value.map_err(domain_error)?;
    if let Some(warehouse) = snapshot.pointer("/cycle/receipt/warehouse").and_then(Value::as_str) {
        state.warehouse_events.notify_updated(warehouse, "waste_receipt");
    }
    result(Ok(snapshot))
}

fn store(state: &AppState) -> Result<&PostgresWasteBinStore, ApiError> {
    state.waste_bins.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "waste_unavailable",
            "Chiqindi qutilari hozir mavjud emas",
        )
    })
}

async fn principal(state: &AppState, headers: &HeaderMap) -> Result<Principal, ApiError> {
    let token = super::auth::bearer_token(headers).ok_or_else(|| {
        error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Avval akkauntga kiring",
        )
    })?;
    state.sessions.get(&token).await.map_err(|_| {
        error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Avval akkauntga kiring",
        )
    })
}

async fn is_receiver(state: &AppState, principal: &Principal) -> bool {
    principal.role == PrincipalRole::ChiqindiQabulchi
        && state
            .admin
            .principal_has_capability(principal, Capability::WasteReceive)
            .await
}

async fn can_inspect_all(state: &AppState, principal: &Principal) -> bool {
    is_receiver(state, principal).await
        || (principal.role == PrincipalRole::Admin
            && state
                .admin
                .principal_has_capability(principal, Capability::AdminAccess)
                .await)
}

async fn receiver(state: &AppState, headers: &HeaderMap) -> Result<QueueActionActor, ApiError> {
    let principal = principal(state, headers).await?;
    if !is_receiver(state, &principal).await {
        return Err(forbidden());
    }
    Ok(actor(&principal))
}

async fn warehouses(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<ApiResponse, ApiError> {
    let principal = principal(&state, &headers).await?;
    let receiver_ref = if is_receiver(&state, &principal).await {
        Some(principal.ref_.as_str())
    } else if principal.role == PrincipalRole::Admin
        && state.admin.principal_has_capability(&principal, Capability::AdminAccess).await
    {
        None
    } else {
        return Err(forbidden());
    };
    result(store(&state)?.warehouses(receiver_ref).await)
}

fn actor(principal: &Principal) -> QueueActionActor {
    QueueActionActor {
        role: match principal.role {
            PrincipalRole::ChiqindiQabulchi => "chiqindi_qabulchi",
            PrincipalRole::RezkaMaster => "rezka_master",
            PrincipalRole::TayyorlovMasteri => "tayyorlov_masteri",
            PrincipalRole::Metalayzer => "metalayzer",
            PrincipalRole::HomashyoRezkachi => "homashyo_rezkachi",
            _ => "aparatchi",
        }
        .into(),
        ref_: principal.ref_.clone(),
        display_name: principal.display_name.clone(),
    }
}

async fn operator_scope(
    state: &AppState,
    principal: &Principal,
    apparatus: &str,
    write: bool,
) -> Result<(), ApiError> {
    let capability = match principal.role {
        PrincipalRole::Aparatchi | PrincipalRole::RezkaMaster if write => {
            Capability::ApparatusQueueManage
        }
        PrincipalRole::Aparatchi | PrincipalRole::RezkaMaster => Capability::ApparatusQueueRead,
        PrincipalRole::TayyorlovMasteri => Capability::PreparationAccess,
        PrincipalRole::Metalayzer => Capability::MetalayzerAccess,
        PrincipalRole::HomashyoRezkachi => Capability::RawMaterialSplitAccess,
        _ => return Err(forbidden()),
    };
    if !state
        .admin
        .principal_has_capability(principal, capability)
        .await
        || !apparatus_matches_assigned(
            apparatus,
            &state.principal_assigned_apparatus(principal).await,
        )
    {
        return Err(forbidden());
    }
    state
        .production_maps
        .resolve_canonical_apparatus_text(apparatus)
        .await
        .map_err(|_| {
            error(
                StatusCode::CONFLICT,
                "waste_apparatus_unavailable",
                "Apparat hozir faol emas",
            )
        })?;
    Ok(())
}

fn require_cycle_scope(bin: &Value, apparatus: &str) -> Result<(), ApiError> {
    if bin.pointer("/cycle/apparatus_id").and_then(Value::as_str) != Some(apparatus.trim()) {
        return Err(forbidden());
    }
    Ok(())
}

#[derive(Deserialize)]
struct ListQuery {
    status: Option<String>,
    apparatus_id: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<ApiResponse, ApiError> {
    let principal = principal(&state, &headers).await?;
    let status = query.status.as_deref();
    let mut apparatus = query.apparatus_id.as_deref().map(str::trim);
    if !can_inspect_all(&state, &principal).await {
        operator_scope(&state, &principal, apparatus.ok_or_else(forbidden)?, false).await?;
        if status == Some("empty") {
            // Empty bins can be selected at any apparatus.
            apparatus = None;
        }
    }
    result(store(&state)?.list(status, apparatus).await)
}

#[derive(Deserialize)]
struct ApparatusQuery {
    apparatus_id: String,
}

async fn active(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ApparatusQuery>,
) -> Result<ApiResponse, ApiError> {
    let principal = principal(&state, &headers).await?;
    let apparatus = query.apparatus_id.trim();
    let inspect_all = can_inspect_all(&state, &principal).await;
    if !inspect_all {
        operator_scope(&state, &principal, apparatus, false).await?;
    }
    let mut value = store(&state)?
        .active(apparatus)
        .await
        .map_err(domain_error)?;
    if !inspect_all && let Some(pending) = value.get_mut("pending").and_then(Value::as_array_mut) {
        pending.retain(|entry| entry["actor_ref"].as_str() == Some(principal.ref_.as_str()));
    }
    result(Ok(value))
}

#[derive(Deserialize)]
struct DetailQuery {
    code: String,
    apparatus_id: Option<String>,
    cycle_id: Option<String>,
}

async fn detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DetailQuery>,
) -> Result<ApiResponse, ApiError> {
    let principal = principal(&state, &headers).await?;
    let inspect_all = can_inspect_all(&state, &principal).await;
    let apparatus = query.apparatus_id.as_deref();
    if !inspect_all {
        operator_scope(&state, &principal, apparatus.ok_or_else(forbidden)?, false).await?;
    }
    let store = store(&state)?;
    let bin = match query.cycle_id.as_deref() {
        Some(cycle_id) => store.detail_cycle(&query.code, cycle_id).await,
        None => store.detail(&query.code).await,
    }
    .map_err(domain_error)?;
    if !inspect_all {
        require_cycle_scope(&bin, apparatus.ok_or_else(forbidden)?)?;
    }
    result(Ok(bin))
}

#[derive(Deserialize)]
struct CodeQuery {
    code: String,
}

async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CodeQuery>,
) -> Result<ApiResponse, ApiError> {
    let principal = principal(&state, &headers).await?;
    if !can_inspect_all(&state, &principal).await {
        return Err(forbidden());
    }
    result(store(&state)?.history(&query.code).await)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterRequest {
    code: String,
    tare_kg: f64,
    tolerance_kg: Option<f64>,
}

async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RegisterRequest>,
) -> Result<ApiResponse, ApiError> {
    let actor = receiver(&state, &headers).await?;
    result(
        store(&state)?
            .register(&input.code, input.tare_kg, input.tolerance_kg, &actor)
            .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssignRequest {
    code: String,
    apparatus_id: String,
}

async fn assign(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<AssignRequest>,
) -> Result<ApiResponse, ApiError> {
    let principal = principal(&state, &headers).await?;
    operator_scope(&state, &principal, &input.apparatus_id, true).await?;
    result(
        store(&state)?
            .assign(&input.code, input.apparatus_id.trim(), &actor(&principal))
            .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitRequest {
    code: String,
    cycle_id: String,
    apparatus_id: String,
    replacement_code: Option<String>,
}

async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SubmitRequest>,
) -> Result<ApiResponse, ApiError> {
    let principal = principal(&state, &headers).await?;
    operator_scope(&state, &principal, &input.apparatus_id, true).await?;
    let store = store(&state)?;
    if !store
        .cycle_matches_apparatus(&input.code, &input.cycle_id, &input.apparatus_id)
        .await
        .map_err(domain_error)?
    {
        return Err(forbidden());
    }
    result(
        store
            .submit(
                &input.code,
                &input.cycle_id,
                input.replacement_code.as_deref(),
                &actor(&principal),
            )
            .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryRequest {
    code: String,
    cycle_id: String,
    apparatus_id: String,
    request_id: String,
    kg: f64,
    reason: String,
    order_id: Option<String>,
}

async fn add(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<EntryRequest>,
) -> Result<ApiResponse, ApiError> {
    let principal = principal(&state, &headers).await?;
    let apparatus = input.apparatus_id.trim();
    operator_scope(&state, &principal, apparatus, true).await?;
    let store = store(&state)?;
    if !store
        .cycle_matches_apparatus(&input.code, &input.cycle_id, apparatus)
        .await
        .map_err(domain_error)?
    {
        return Err(forbidden());
    }
    result(
        store
            .add(
                &input.code,
                &input.cycle_id,
                &WasteEntry {
                    request_id: &input.request_id,
                    apparatus_id: apparatus,
                    order_id: input.order_id.as_deref(),
                    session_id: None,
                    expected_cycle_id: Some(&input.cycle_id),
                    kg: input.kg,
                    reason: &input.reason,
                    actor: &actor(&principal),
                    reconcile_manual: false,
                },
            )
            .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReconcileRequest {
    code: String,
    cycle_id: String,
    apparatus_id: String,
    entry_ids: Vec<String>,
}

async fn reconcile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ReconcileRequest>,
) -> Result<ApiResponse, ApiError> {
    let principal = principal(&state, &headers).await?;
    operator_scope(&state, &principal, &input.apparatus_id, true).await?;
    let store = store(&state)?;
    if !store
        .cycle_matches_apparatus(&input.code, &input.cycle_id, &input.apparatus_id)
        .await
        .map_err(domain_error)?
    {
        return Err(forbidden());
    }
    result(
        store
            .reconcile(
                &input.code,
                &input.cycle_id,
                &input.entry_ids,
                &actor(&principal),
            )
            .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiveRequest {
    code: String,
    cycle_id: String,
    #[serde(default)]
    warehouse_id: String,
    gross_kg: f64,
    measurement_source: String,
    #[serde(default)]
    note: String,
}

async fn receive(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ReceiveRequest>,
) -> Result<ApiResponse, ApiError> {
    let actor = receiver(&state, &headers).await?;
    warehouse_credit(
        &state,
        store(&state)?
            .receive(
                &input.code,
                &input.cycle_id,
                &input.warehouse_id,
                input.gross_kg,
                &input.measurement_source,
                &input.note,
                &actor,
            )
            .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DestinationRequest {
    code: String,
    cycle_id: String,
    warehouse_id: String,
}

async fn record_destination(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<DestinationRequest>,
) -> Result<ApiResponse, ApiError> {
    let actor = receiver(&state, &headers).await?;
    warehouse_credit(
        &state,
        store(&state)?
            .record_legacy_destination(&input.code, &input.cycle_id, &input.warehouse_id, &actor)
            .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResetRequest {
    code: String,
    cycle_id: String,
}

async fn reset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ResetRequest>,
) -> Result<ApiResponse, ApiError> {
    let actor = receiver(&state, &headers).await?;
    result(
        store(&state)?
            .reset(&input.code, &input.cycle_id, &actor)
            .await,
    )
}
