use axum::Json;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode, header};

use crate::app::AppState;
use crate::core::werka::models::WerkaArchiveResponse;
use crate::http::archive_pdf::build_archive_pdf;
use crate::http::handlers::auth::ErrorResponse;
use crate::http::handlers::werka::authz::{authorize, require_werka};
use crate::http::handlers::werka::query::{ArchiveQuery, archive_pdf_failed, parse_archive_date};

pub async fn archive(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ArchiveQuery>,
) -> Result<Json<WerkaArchiveResponse>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&state, &principal).await?;

    let from = parse_archive_date(query.from.as_deref())?;
    let to = parse_archive_date(query.to.as_deref())?;
    let kind = query.kind.as_deref().unwrap_or("").trim();
    let period = query.period.as_deref().unwrap_or("").trim();
    match state.werka.archive(kind, period, from, to).await {
        Ok(Some(data)) => Ok(Json(data)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka archive failed",
            }),
        )),
    }
}

pub async fn archive_pdf(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ArchiveQuery>,
) -> Result<Response<Body>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&state, &principal).await?;

    let from = parse_archive_date(query.from.as_deref()).map_err(|_| archive_pdf_failed())?;
    let to = parse_archive_date(query.to.as_deref()).map_err(|_| archive_pdf_failed())?;
    let kind = query.kind.as_deref().unwrap_or("").trim();
    let period = query.period.as_deref().unwrap_or("").trim();
    let data = match state.werka.archive(kind, period, from, to).await {
        Ok(Some(data)) => data,
        Ok(None) | Err(_) => return Err(archive_pdf_failed()),
    };

    let filename = format!("werka-{}-{}.pdf", data.kind, data.period);
    let mut response = Response::new(Body::from(build_archive_pdf(&data)));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/pdf"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|_| archive_pdf_failed())?,
    );
    Ok(response)
}
