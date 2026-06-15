use super::*;

use crate::core::workers::{WorkerError, WorkerUpsert};

pub async fn workers(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    if !matches!(method, Method::GET | Method::POST | Method::PUT) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => state
            .workers
            .workers(
                query.q.as_deref().unwrap_or(""),
                optional_search_limit(query.limit.as_deref(), 50, 500),
            )
            .await
            .map(json_response)
            .map_err(worker_error),
        Method::POST => {
            let input: WorkerUpsert = parse_json(&body)?;
            state
                .workers
                .upsert_worker(input)
                .await
                .map(json_response)
                .map_err(worker_error)
        }
        Method::PUT => {
            let input: WorkerUpsert = parse_json(&body)?;
            state
                .workers
                .update_worker_level(input)
                .await
                .map(json_response)
                .map_err(worker_error)
        }
        _ => Err(method_not_allowed()),
    }
}

fn worker_error(error: WorkerError) -> AdminError {
    match error {
        WorkerError::MissingName => bad_request("worker name is required"),
        WorkerError::MissingId => bad_request("worker id is required"),
        WorkerError::InvalidLevel => bad_request("worker level is invalid"),
        WorkerError::NotFound => not_found("worker not found"),
        WorkerError::StoreFailed => server_error("worker store failed"),
    }
}
