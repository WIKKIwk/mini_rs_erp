use super::*;
use crate::core::production_map::output_stats::{
    OutputStatsError, OutputStatsQuery, OutputStatsRequest,
};
use axum::extract::rejection::QueryRejection;

pub async fn production_map_output_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<OutputStatsQuery>, QueryRejection>,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ProductionMapRead,
            Capability::ApparatusQueueRead,
        ],
    )
    .await?;
    let query = query
        .map_err(|_| bad_request("output_stats_invalid_query"))?
        .0;
    let request = OutputStatsRequest::try_from(query).map_err(report_error)?;
    let can_view_all = state
        .admin
        .principal_has_admin_workspace_access(&principal)
        .await
        || state
            .admin
            .principal_has_capability(&principal, Capability::ProductionMapManage)
            .await;
    let scope = if can_view_all {
        None
    } else {
        let mut assigned = state.admin.principal_assigned_apparatus(&principal).await;
        assigned.retain(|id| ApparatusId::new(id.clone()).is_ok());
        assigned.sort();
        assigned.dedup();
        if assigned.is_empty()
            || request
                .apparatus
                .as_ref()
                .is_some_and(|id| !assigned.contains(id))
        {
            return Err(forbidden());
        }
        Some(assigned)
    };
    let report = state
        .production_maps
        .output_stats(&request, scope.as_deref())
        .await
        .map_err(report_error)?;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "application/json"),
            (axum::http::header::CACHE_CONTROL, "no-store"),
        ],
        report.0,
    )
        .into_response())
}

fn report_error(error: OutputStatsError) -> AdminError {
    match error {
        OutputStatsError::Invalid(code) => bad_request(code),
        OutputStatsError::StoreFailed => server_error("output_stats_store_failed"),
    }
}
