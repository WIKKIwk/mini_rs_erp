use super::mapping::customer_delivery_status;
use crate::core::customer::models::{
    CustomerDeliveryResponseMode, CustomerDeliveryResponseRequest,
};
use crate::core::customer::ports::{CustomerDeliveryNoteDraft, CustomerServiceError};

pub(super) const DELIVERY_FLOW_STATE_SUBMITTED: i32 = 1;
pub(super) const DELIVERY_ACTOR_WERKA: i32 = 2;

const CUSTOMER_STATE_REJECTED: i32 = 2;
const CUSTOMER_STATE_CONFIRMED: i32 = 3;
const CUSTOMER_STATE_PARTIAL: i32 = 4;
const CUSTOMER_QTY_TOLERANCE: f64 = 0.0001;
const MIN_CUSTOMER_REJECT_REASON_RUNES: usize = 3;

pub(super) struct CustomerDeliveryDecision {
    pub(super) customer_state: i32,
    pub(super) accepted_qty: f64,
    pub(super) returned_qty: f64,
    pub(super) reason: String,
    pub(super) comment: String,
}

impl CustomerDeliveryDecision {
    pub(super) fn state_label(&self) -> &'static str {
        match self.customer_state {
            CUSTOMER_STATE_CONFIRMED => "confirmed",
            CUSTOMER_STATE_REJECTED => "rejected",
            CUSTOMER_STATE_PARTIAL => "partial",
            _ => "pending",
        }
    }
}

pub(super) fn normalize_customer_delivery_decision(
    request: &CustomerDeliveryResponseRequest,
    draft: &CustomerDeliveryNoteDraft,
) -> Result<CustomerDeliveryDecision, CustomerServiceError> {
    let current_status = customer_delivery_status(draft);
    let mode = request.mode.or_else(|| {
        request.approve.map(|approve| {
            if approve {
                CustomerDeliveryResponseMode::AcceptAll
            } else {
                CustomerDeliveryResponseMode::RejectAll
            }
        })
    });
    let reason = request.reason.trim().to_string();
    let comment = request.comment.trim().to_string();
    let sent_qty = draft.qty;
    if sent_qty <= 0.0 {
        return Err(CustomerServiceError::InvalidInput);
    }

    match mode {
        Some(CustomerDeliveryResponseMode::AcceptAll) => {
            require_pending(current_status)?;
            Ok(CustomerDeliveryDecision {
                customer_state: CUSTOMER_STATE_CONFIRMED,
                accepted_qty: sent_qty,
                returned_qty: 0.0,
                reason,
                comment,
            })
        }
        Some(CustomerDeliveryResponseMode::RejectAll) => {
            require_pending(current_status)?;
            require_meaningful_return_reason(&reason, &comment)?;
            Ok(CustomerDeliveryDecision {
                customer_state: CUSTOMER_STATE_REJECTED,
                accepted_qty: 0.0,
                returned_qty: sent_qty,
                reason,
                comment,
            })
        }
        Some(CustomerDeliveryResponseMode::AcceptPartial) => {
            require_pending(current_status)?;
            require_meaningful_return_reason(&reason, &comment)?;
            let (accepted_qty, returned_qty) =
                normalize_partial_quantities(sent_qty, request.accepted_qty, request.returned_qty)?;
            Ok(CustomerDeliveryDecision {
                customer_state: CUSTOMER_STATE_PARTIAL,
                accepted_qty,
                returned_qty,
                reason,
                comment,
            })
        }
        Some(CustomerDeliveryResponseMode::ClaimAfterAccept) => {
            if current_status != "accepted" {
                return Err(CustomerServiceError::Failed(format!(
                    "delivery note cannot accept claim in status {current_status}"
                )));
            }
            require_meaningful_return_reason(&reason, &comment)?;
            let returned_qty = request.returned_qty;
            if returned_qty <= 0.0 || returned_qty > sent_qty + CUSTOMER_QTY_TOLERANCE {
                return Err(CustomerServiceError::InvalidInput);
            }
            if nearly_equal_qty(returned_qty, sent_qty) {
                return Ok(CustomerDeliveryDecision {
                    customer_state: CUSTOMER_STATE_REJECTED,
                    accepted_qty: 0.0,
                    returned_qty: sent_qty,
                    reason,
                    comment,
                });
            }
            Ok(CustomerDeliveryDecision {
                customer_state: CUSTOMER_STATE_PARTIAL,
                accepted_qty: sent_qty - returned_qty,
                returned_qty,
                reason,
                comment,
            })
        }
        None => Err(CustomerServiceError::InvalidInput),
    }
}

fn require_pending(status: &str) -> Result<(), CustomerServiceError> {
    if status == "pending" {
        Ok(())
    } else {
        Err(CustomerServiceError::Failed(
            "delivery note is not pending".to_string(),
        ))
    }
}

fn require_meaningful_return_reason(
    reason: &str,
    comment: &str,
) -> Result<(), CustomerServiceError> {
    if reason.trim().chars().count() >= MIN_CUSTOMER_REJECT_REASON_RUNES
        || comment.trim().chars().count() >= MIN_CUSTOMER_REJECT_REASON_RUNES
    {
        Ok(())
    } else {
        Err(CustomerServiceError::InvalidInput)
    }
}

fn normalize_partial_quantities(
    sent_qty: f64,
    mut accepted_qty: f64,
    mut returned_qty: f64,
) -> Result<(f64, f64), CustomerServiceError> {
    if accepted_qty > 0.0 && returned_qty > 0.0 {
    } else if accepted_qty > 0.0 {
        returned_qty = sent_qty - accepted_qty;
    } else if returned_qty > 0.0 {
        accepted_qty = sent_qty - returned_qty;
    } else {
        return Err(CustomerServiceError::InvalidInput);
    }
    if accepted_qty <= 0.0 || returned_qty <= 0.0 {
        return Err(CustomerServiceError::InvalidInput);
    }
    if ((accepted_qty + returned_qty) - sent_qty).abs() > CUSTOMER_QTY_TOLERANCE {
        return Err(CustomerServiceError::InvalidInput);
    }
    Ok((accepted_qty, returned_qty))
}

pub(super) fn nearly_equal_qty(left: f64, right: f64) -> bool {
    (left - right).abs() <= CUSTOMER_QTY_TOLERANCE
}

pub(super) fn customer_delivery_ui_status(flow_state: i32, customer_state: i32) -> &'static str {
    if flow_state != DELIVERY_FLOW_STATE_SUBMITTED {
        return "pending";
    }
    match customer_state {
        CUSTOMER_STATE_CONFIRMED => "confirm",
        CUSTOMER_STATE_PARTIAL => "partial",
        CUSTOMER_STATE_REJECTED => "rejected",
        _ => "pending",
    }
}

pub(super) fn upsert_customer_decision_payload_in_remarks(
    existing_note: &str,
    state: &str,
    reason: &str,
    accepted_qty: f64,
    returned_qty: f64,
    uom: &str,
    comment: &str,
) -> String {
    let mut filtered = Vec::new();
    for line in existing_note.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("AC:")
            || trimmed.starts_with("AR:")
            || trimmed.starts_with("AQ:")
            || trimmed.starts_with("AT:")
            || trimmed.starts_with("AX:")
        {
            continue;
        }
        filtered.push(trimmed.to_string());
    }
    if let Some(normalized) = normalize_customer_decision_state(state) {
        filtered.push(format!("AC:{normalized}"));
    }
    if !reason.trim().is_empty() {
        filtered.push(format!("AR:{}", reason.trim()));
    }
    if accepted_qty > 0.0 {
        filtered.push(format!("AQ:{accepted_qty:.4} {}", uom.trim()));
    }
    if returned_qty > 0.0 {
        filtered.push(format!("AT:{returned_qty:.4} {}", uom.trim()));
    }
    if !comment.trim().is_empty() {
        filtered.push(format!("AX:{}", comment.trim()));
    }
    filtered.join("\n")
}

fn normalize_customer_decision_state(state: &str) -> Option<&'static str> {
    match state.trim().to_ascii_lowercase().as_str() {
        "pending" | "pd" => Some("pending"),
        "confirmed" | "accepted" | "cf" => Some("confirmed"),
        "partial" | "pt" => Some("partial"),
        "rejected" | "rj" => Some("rejected"),
        _ => None,
    }
}

pub(super) fn combine_customer_reason_and_comment(reason: &str, comment: &str) -> String {
    let reason = reason.trim();
    let comment = comment.trim();
    match (reason.is_empty(), comment.is_empty()) {
        (true, _) => comment.to_string(),
        (_, true) => reason.to_string(),
        _ => format!("{reason}. {comment}"),
    }
}
