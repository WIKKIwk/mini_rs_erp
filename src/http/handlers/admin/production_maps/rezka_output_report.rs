use super::order_scan_bootstrap::{BOOTSTRAP_CAPABILITIES, authorization_scope};
use super::*;
use axum::extract::rejection::QueryRejection;

#[derive(serde::Deserialize)]
pub struct RezkaOutputReportQuery {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_id: String,
}

pub async fn production_map_rezka_output_report(
    State(state): State<AppState>,
    query: Result<Query<RezkaOutputReportQuery>, QueryRejection>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let result = match query {
        Ok(Query(query)) => tokio::time::timeout(
            std::time::Duration::from_secs(10),
            read_report(&state, query, method, &headers),
        )
        .await
        .unwrap_or_else(|_| {
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(AdminErrorResponse::new("rezka_output_report_timeout")),
            ))
        }),
        Err(_) => Err(bad_request("invalid_rezka_output_report_query")),
    };
    let mut response = match result {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn read_report(
    state: &AppState,
    query: RezkaOutputReportQuery,
    method: Method,
    headers: &HeaderMap,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(state, headers, BOOTSTRAP_CAPABILITIES).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let apparatus = query.apparatus.trim();
    let order_id = query.order_id.trim();
    if !queue_state::is_canonical_apparatus_id(apparatus) || order_id.is_empty() {
        return Err(bad_request("canonical apparatus and order_id are required"));
    }
    if order_id.starts_with("training-") {
        return Err(bad_request("training_rezka_output_report_unsupported"));
    }
    let scope = authorization_scope(state, &principal, apparatus).await?;
    let mut revision = state.production_maps.snapshot_revision();
    let report = state
        .production_maps
        .current_rezka_output_report(apparatus, order_id)
        .await
        .map_err(report_error)?;
    // An unrelated apparatus can invalidate the shared revision while this
    // read awaits. Compare just this map/control/session once, never rebuild
    // the global queue or retry indefinitely under other devices' activity.
    if state.production_maps.snapshot_revision() != revision {
        let latest = state
            .production_maps
            .current_rezka_output_report(apparatus, order_id)
            .await
            .map_err(report_error)?;
        if latest != report {
            return Err(conflict("rezka_output_cycle_conflict"));
        }
        revision = state.production_maps.snapshot_revision();
    }
    let current = authorize_any_capability(state, headers, BOOTSTRAP_CAPABILITIES).await?;
    if authorization_scope(state, &current, apparatus).await? != scope {
        return Err(conflict("rezka_output_report_scope_changed"));
    }
    if state.production_maps.snapshot_revision() != revision {
        let latest = state
            .production_maps
            .current_rezka_output_report(apparatus, order_id)
            .await
            .map_err(report_error)?;
        if latest != report {
            return Err(conflict("rezka_output_cycle_conflict"));
        }
        revision = state.production_maps.snapshot_revision();
        // This reader happened after the prior authorization check. Close its
        // authorization boundary too before returning any saved labels.
        let current = authorize_any_capability(state, headers, BOOTSTRAP_CAPABILITIES).await?;
        if authorization_scope(state, &current, apparatus).await? != scope {
            return Err(conflict("rezka_output_report_scope_changed"));
        }
    }
    Ok(json_response(serde_json::json!({
        "ok": true, "apparatus": apparatus, "order_id": order_id,
        "rezka_output_report": report.report, "kadr_counts": report.kadr_counts,
        "session_status": "active", "order_control": "active",
        "epoch": state.production_maps.snapshot_epoch(), "rev": revision,
    })))
}

fn report_error(error: ProductionMapError) -> AdminError {
    match error {
        ProductionMapError::ProgressInputInvalid => {
            bad_request("rezka_output_report_requires_cut_apparatus")
        }
        ProductionMapError::MapNotFound
        | ProductionMapError::RezkaOutputCycleConflict
        | ProductionMapError::QueueActionNotAllowed
        | ProductionMapError::OrderFrozen => conflict("rezka_output_cycle_conflict"),
        other => production_map_error(other),
    }
}
