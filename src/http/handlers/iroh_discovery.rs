use std::path::Path;

use axum::Json;
use axum::http::StatusCode;
use serde::Serialize;

const IROH_TICKET_ENV: &str = "IROH_ENDPOINT_TICKET";
const IROH_TICKET_FILE_ENV: &str = "IROH_TICKET_FILE";

#[derive(Debug, Serialize)]
pub struct IrohTicketResponse {
    pub ticket: String,
    pub source: &'static str,
}

#[derive(Debug, Serialize)]
pub struct IrohTicketErrorResponse {
    pub error: &'static str,
}

pub async fn ticket()
-> Result<Json<IrohTicketResponse>, (StatusCode, Json<IrohTicketErrorResponse>)> {
    match load_ticket().await {
        Some((ticket, source)) => Ok(Json(IrohTicketResponse { ticket, source })),
        None => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(IrohTicketErrorResponse {
                error: "iroh_ticket_unavailable",
            }),
        )),
    }
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
    use super::clean_ticket;

    #[test]
    fn clean_ticket_rejects_blank_values() {
        assert_eq!(clean_ticket("  \n\t"), None);
    }

    #[test]
    fn clean_ticket_trims_non_blank_values() {
        assert_eq!(clean_ticket("  abc-123\n").as_deref(), Some("abc-123"));
    }
}
