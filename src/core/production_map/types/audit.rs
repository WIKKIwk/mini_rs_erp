use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionWorkflowAuditReport {
    pub ok: bool,
    pub checked_order_count: usize,
    pub checked_batch_count: usize,
    pub checked_session_count: usize,
    pub violations: Vec<ProductionWorkflowAuditViolation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionWorkflowAuditViolation {
    pub code: String,
    pub order_id: String,
    pub subject: String,
    pub detail: String,
}

impl ProductionWorkflowAuditViolation {
    pub fn new(code: &str, order_id: &str, subject: &str, detail: &str) -> Self {
        Self {
            code: code.to_string(),
            order_id: order_id.to_string(),
            subject: subject.to_string(),
            detail: detail.to_string(),
        }
    }
}
