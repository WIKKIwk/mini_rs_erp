use axum::Json;
use axum::http::StatusCode;
use serde::Deserialize;
use time::{Date, Month};

use crate::http::handlers::auth::ErrorResponse;

#[derive(Debug, Deserialize)]
pub struct StatusBreakdownQuery {
    pub(super) kind: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StatusDetailsQuery {
    pub(super) kind: Option<String>,
    pub(super) supplier_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ArchiveQuery {
    pub(super) kind: Option<String>,
    pub(super) period: Option<String>,
    pub(super) from: Option<String>,
    pub(super) to: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DirectoryQuery {
    pub(super) q: Option<String>,
    pub(super) limit: Option<String>,
    pub(super) offset: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SupplierItemsQuery {
    pub(super) supplier_ref: Option<String>,
    pub(super) q: Option<String>,
    pub(super) limit: Option<String>,
    pub(super) offset: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CustomerItemsQuery {
    pub(super) customer_ref: Option<String>,
    pub(super) q: Option<String>,
    pub(super) limit: Option<String>,
    pub(super) offset: Option<String>,
}

pub(super) fn parse_archive_date(
    raw: Option<&str>,
) -> Result<Option<Date>, (StatusCode, Json<ErrorResponse>)> {
    let Some(trimmed) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let parts: Vec<_> = trimmed.split('-').collect();
    if parts.len() != 3 {
        return Err(archive_failed());
    }
    let year = parts[0].parse::<i32>().map_err(|_| archive_failed())?;
    let month = parts[1].parse::<u8>().map_err(|_| archive_failed())?;
    let day = parts[2].parse::<u8>().map_err(|_| archive_failed())?;
    let month = Month::try_from(month).map_err(|_| archive_failed())?;
    Date::from_calendar_date(year, month, day)
        .map(Some)
        .map_err(|_| archive_failed())
}

pub(super) fn archive_failed() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "werka archive failed",
        }),
    )
}

pub(super) fn archive_pdf_failed() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "werka archive pdf failed",
        }),
    )
}

pub(super) fn optional_search_limit(
    raw: Option<&str>,
    default_limit: usize,
    max_limit: usize,
) -> usize {
    let Some(trimmed) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return default_limit;
    };
    let Ok(value) = trimmed.parse::<usize>() else {
        return default_limit;
    };
    if value == 0 {
        return default_limit;
    }
    if max_limit > 0 && value > max_limit {
        return max_limit;
    }
    value
}

pub(super) fn optional_search_offset(raw: Option<&str>) -> usize {
    let Some(trimmed) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return 0;
    };
    trimmed.parse::<usize>().unwrap_or(0)
}
