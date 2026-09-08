use crate::{
    app::AppState,
    core::{
        auth::models::{Principal, PrincipalRole},
        authz::Capability,
        gscale::models::ScaleDriverPrintRequest,
        raw_material_split::*,
    },
};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
type ApiError = (StatusCode, Json<Value>);

async fn authorize(state: &AppState, headers: &HeaderMap) -> Result<Principal, ApiError> {
    let token = super::auth::bearer_token(headers).unwrap_or_default();
    let actor = state.sessions.get(&token).await.map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"unauthorized"})),
        )
    })?;
    if actor.role != PrincipalRole::HomashyoRezkachi
        || !state
            .admin
            .principal_has_capability(&actor, Capability::RawMaterialSplitAccess)
            .await
    {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error":"forbidden"}))));
    }
    Ok(actor)
}
fn store(
    state: &AppState,
) -> Result<&crate::db::postgres_raw_material_split::PostgresRawMaterialSplitStore, ApiError> {
    state.raw_material_split.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error":"Homashyo rezka ombori hozir mavjud emas"})),
    ))
}
fn error(e: SplitError) -> ApiError {
    let (status, code) = match &e {
        SplitError::Invalid(_) => (StatusCode::BAD_REQUEST, "raw_split_invalid"),
        SplitError::Conflict(_) => (StatusCode::CONFLICT, "raw_split_conflict"),
        SplitError::Forbidden => (StatusCode::FORBIDDEN, "raw_split_scope"),
        SplitError::StoreFailed => (StatusCode::INTERNAL_SERVER_ERROR, "raw_split_store"),
    };
    (status, Json(json!({"code":code,"error":e.to_string()})))
}
pub async fn snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .snapshot(&actor.ref_)
        .await
        .map(Json)
        .map_err(error)
}
#[derive(Deserialize)]
pub struct SourceQuery {
    pub barcode: String,
}
pub async fn source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SourceQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .source(&actor.ref_, &q.barcode)
        .await
        .map(Json)
        .map_err(error)
}
pub async fn split(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SplitCreate>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let result = store(&state)?.split(&actor, input).await.map_err(error)?;
    state.warehouse_events.notify_updated(
        result["warehouse"].as_str().unwrap_or_default(),
        "raw_material_stock",
    );
    Ok(Json(result))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintInput {
    pub split_id: String,
    pub barcode: String,
    pub driver_url: String,
    pub printer: String,
    pub print_mode: String,
}
/// Printing reads the immutable saved output. It never submits a material receipt
/// or modifies inventory, including after an uncertain printer response.
pub async fn print(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PrintInput>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let saved = store(&state)?
        .saved(&actor.ref_, &input.split_id)
        .await
        .map_err(error)?;
    let output = saved["outputs"]
        .as_array()
        .and_then(|xs| {
            xs.iter()
                .find(|x| x["barcode"].as_str() == Some(&input.barcode))
        })
        .ok_or_else(|| error(SplitError::Forbidden))?;
    let request = saved_print_request(&input, output, &actor.display_name).map_err(error)?;
    let response = state
        .raw_material_split_printer
        .print_material_receipt(request)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":e.to_string()})),
            )
        })?;
    if !response.ok || response.status != "done" {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(
                json!({"error":"Rulon saqlangan, lekin chop etish tasdiqlanmadi. Qayta chop eting."}),
            ),
        ));
    }
    Ok(Json(
        json!({"ok":true,"status":"done","barcode":input.barcode}),
    ))
}

fn saved_print_request(
    input: &PrintInput,
    output: &Value,
    executor: &str,
) -> Result<ScaleDriverPrintRequest, SplitError> {
    if !matches!(input.printer.as_str(), "godex" | "zebra")
        || !matches!(input.print_mode.as_str(), "label" | "rfid" | "both")
    {
        return Err(SplitError::Invalid("Printer turi noto‘g‘ri"));
    }
    let text = |field: &str| output[field].as_str().unwrap_or("").to_owned();
    let kg = quantity(&text("kg"), false)?;
    let (gross, bobina, measured) =
        match (output["gross_kg"].as_str(), output["bobina_kg"].as_str()) {
            (Some(gross), Some(bobina)) => (quantity(gross, false)?, quantity(bobina, true)?, true),
            (None, None) => (kg, 0, false),
            _ => return Err(SplitError::StoreFailed),
        };
    if gross - bobina != kg {
        return Err(SplitError::StoreFailed);
    }
    // Decimal arithmetic above is authoritative; floats are only the printer protocol.
    let as_print_qty = |value| decimal_text(value).parse::<f64>().unwrap();
    Ok(ScaleDriverPrintRequest {
        driver_url: input.driver_url.clone(),
        epc: text("barcode"),
        item_code: text("item_code"),
        item_name: split_item_name(&text("item_name"), &text("width_mm"), &text("micron")),
        warehouse: text("warehouse"),
        executor_name: executor.to_owned(),
        label_kind: "material_product".into(),
        printer: input.printer.clone(),
        print_mode: input.print_mode.clone(),
        gross_qty: as_print_qty(gross),
        qty: Some(as_print_qty(kg)),
        unit: "kg".into(),
        progress_unit: String::new(),
        tare_enabled: measured,
        tare_kg: as_print_qty(bobina),
        print_count: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_material_split_reprint_uses_saved_child_name_gross_and_core() {
        let input = PrintInput {
            split_id: "split".into(),
            barcode: "child".into(),
            driver_url: "http://printer".into(),
            printer: "godex".into(),
            print_mode: "label".into(),
        };
        let mut output = json!({"barcode":"child","item_code":"FILM","item_name":"BOPP METAL 700/12","warehouse":"Raw W","width_mm":"350.000000","micron":"12","kg":"3","gross_kg":"3.5","bobina_kg":"0.5"});
        let print = saved_print_request(&input, &output, "Cutter").unwrap();
        assert_eq!(print.item_name, "BOPP METAL 350/12");
        assert_eq!(
            (print.gross_qty, print.tare_kg, print.qty),
            (3.5, 0.5, Some(3.0))
        );
        assert!(print.tare_enabled);
        output["gross_kg"] = json!("4");
        assert!(saved_print_request(&input, &output, "Cutter").is_err());
        output.as_object_mut().unwrap().remove("gross_kg");
        output.as_object_mut().unwrap().remove("bobina_kg");
        let legacy = saved_print_request(&input, &output, "Cutter").unwrap();
        assert_eq!(legacy.item_name, "BOPP METAL 350/12");
        assert!(!legacy.tare_enabled);
    }
}
