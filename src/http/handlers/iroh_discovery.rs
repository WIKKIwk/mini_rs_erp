use std::path::Path;

use axum::Json;
use axum::http::StatusCode;
use serde::Serialize;

const IROH_TICKET_ENV: &str = "IROH_ENDPOINT_TICKET";
const IROH_TICKET_FILE_ENV: &str = "IROH_TICKET_FILE";
const IROH_SUPPORTS_CONNECTION_REUSE_ENV: &str = "IROH_SUPPORTS_CONNECTION_REUSE";

#[derive(Debug, Serialize)]
pub struct IrohTicketResponse {
    pub ticket: String,
    pub source: &'static str,
    pub supports_connection_reuse: bool,
}

#[derive(Debug, Serialize)]
pub struct IrohTicketErrorResponse {
    pub error: &'static str,
}

pub async fn ticket()
-> Result<Json<IrohTicketResponse>, (StatusCode, Json<IrohTicketErrorResponse>)> {
    match load_ticket().await {
        Some((ticket, source)) => Ok(Json(IrohTicketResponse {
            ticket,
            source,
            supports_connection_reuse: supports_connection_reuse(),
        })),
        None => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(IrohTicketErrorResponse {
                error: "iroh_ticket_unavailable",
            }),
        )),
    }
}

fn supports_connection_reuse() -> bool {
    std::env::var(IROH_SUPPORTS_CONNECTION_REUSE_ENV)
        .ok()
        .map(|value| is_truthy(&value))
        .unwrap_or(false)
}

fn is_truthy(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    matches!(value.as_str(), "1" | "true" | "yes" | "on")
}

async fn load_ticket() -> Option<(String, &'static str)> {
    if let Ok(ticket) = std::env::var(IROH_TICKET_ENV)
        && let Some(ticket) = clean_ticket(&ticket)
    {
        return Some((ticket, "env"));
    }

    let path = std::env::var(IROH_TICKET_FILE_ENV).ok()?;
    let ticket = read_ticket_file(Path::new(&path)).await?;
    Some((ticket, "file"))
}

async fn read_ticket_file(path: &Path) -> Option<String> {
    let raw = tokio::fs::read_to_string(path).await.ok()?;
    clean_ticket(&raw)
}

fn clean_ticket(raw: &str) -> Option<String> {
    let ticket = raw.trim();
    if ticket.is_empty() {
        None
    } else {
        Some(ticket.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{clean_ticket, is_truthy};

    #[test]
    fn clean_ticket_rejects_blank_values() {
        assert_eq!(clean_ticket("  \n\t"), None);
    }

    #[test]
    fn clean_ticket_trims_non_blank_values() {
        assert_eq!(clean_ticket("  abc-123\n").as_deref(), Some("abc-123"));
    }

    #[test]
    fn connection_reuse_capability_parses_truthy_values() {
        assert!(is_truthy("1"));
        assert!(is_truthy("true"));
        assert!(is_truthy(" yes "));
        assert!(is_truthy("on"));
        assert!(!is_truthy(""));
        assert!(!is_truthy("false"));
    }
}
