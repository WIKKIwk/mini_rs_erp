use crate::core::werka::ports::{
    PurchaseReceiptDraft, PurchaseReceiptSubmissionResult, WerkaPortError,
};

pub(super) fn build_accord_decision_note(
    draft: &PurchaseReceiptDraft,
    accepted_qty: f64,
    mut returned_qty: f64,
    return_reason: &str,
    return_comment: &str,
) -> Result<String, WerkaPortError> {
    let implied_returned_qty = (draft.qty - accepted_qty).max(0.0);
    if implied_returned_qty <= 0.0 {
        return Ok(String::new());
    }
    if returned_qty < 0.0 {
        return Err(WerkaPortError::WriteFailed(
            "returned qty cannot be negative".to_string(),
        ));
    }
    if returned_qty == 0.0 {
        returned_qty = implied_returned_qty;
    }
    if returned_qty - implied_returned_qty > 0.0001 {
        return Err(WerkaPortError::WriteFailed(
            "returned qty cannot exceed sent minus accepted qty".to_string(),
        ));
    }

    let mut lines = vec![
        format!("Accord Qabul: {:.4} {}", accepted_qty, draft.uom),
        format!("Accord Qaytarildi: {:.4} {}", returned_qty, draft.uom),
    ];
    if !return_reason.trim().is_empty() {
        lines.push(format!("Accord Sabab: {}", return_reason.trim()));
    }
    if !return_comment.trim().is_empty() {
        lines.push(format!("Accord Izoh: {}", return_comment.trim()));
    }
    Ok(lines.join("\n"))
}

pub(super) fn upsert_accord_decision_in_remarks(existing: &str, decision: &str) -> String {
    let mut lines = existing
        .replace("\r\n", "\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            !line.starts_with("Accord Qabul:")
                && !line.starts_with("Accord Qaytarildi:")
                && !line.starts_with("Accord Sabab:")
                && !line.starts_with("Accord Izoh:")
                && !line.starts_with("Accord Supplier Tasdiq:")
        })
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if !decision.trim().is_empty() {
        lines.extend(decision.trim().lines().map(|line| line.trim().to_string()));
    }
    lines.join("\n")
}

pub(super) fn submission_result(
    draft: &PurchaseReceiptDraft,
    accepted_qty: f64,
    decision_note: &str,
) -> PurchaseReceiptSubmissionResult {
    PurchaseReceiptSubmissionResult {
        name: draft.name.clone(),
        supplier: draft.supplier.clone(),
        item_code: draft.item_code.clone(),
        uom: draft.uom.clone(),
        sent_qty: draft.qty,
        accepted_qty,
        supplier_delivery_note: draft.supplier_delivery_note.clone(),
        note: extract_accord_decision_note(decision_note),
    }
}

fn extract_accord_decision_note(remarks: &str) -> String {
    remarks
        .replace("\r\n", "\n")
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            line.strip_prefix("Accord Qabul:")
                .map(|value| format!("Qabul: {}", value.trim()))
                .or_else(|| {
                    line.strip_prefix("Accord Qaytarildi:")
                        .map(|value| format!("Qaytarildi: {}", value.trim()))
                })
                .or_else(|| {
                    line.strip_prefix("Accord Sabab:")
                        .map(|value| format!("Sabab: {}", value.trim()))
                })
                .or_else(|| {
                    line.strip_prefix("Accord Izoh:")
                        .map(|value| format!("Izoh: {}", value.trim()))
                })
                .or_else(|| {
                    line.strip_prefix("Accord Supplier Tasdiq:")
                        .map(|value| format!("Supplier tasdiqladi: {}", value.trim()))
                })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        build_accord_decision_note, extract_accord_decision_note, upsert_accord_decision_in_remarks,
    };
    use crate::core::werka::ports::PurchaseReceiptDraft;

    #[test]
    fn decision_note_matches_go_format() {
        let note = build_accord_decision_note(&draft(), 7.0, 3.0, "Brak chiqdi", "Qop yorilgan")
            .expect("note");

        assert_eq!(
            note,
            "Accord Qabul: 7.0000 Kg\nAccord Qaytarildi: 3.0000 Kg\nAccord Sabab: Brak chiqdi\nAccord Izoh: Qop yorilgan"
        );
        assert_eq!(
            extract_accord_decision_note(&note),
            "Qabul: 7.0000 Kg\nQaytarildi: 3.0000 Kg\nSabab: Brak chiqdi\nIzoh: Qop yorilgan"
        );
    }

    #[test]
    fn upsert_decision_removes_old_decision_and_supplier_ack_like_go() {
        let remarks = upsert_accord_decision_in_remarks(
            "Keep\nAccord Qabul: 1.0000 Kg\nAccord Supplier Tasdiq: yes",
            "Accord Qabul: 7.0000 Kg\nAccord Qaytarildi: 3.0000 Kg",
        );

        assert_eq!(
            remarks,
            "Keep\nAccord Qabul: 7.0000 Kg\nAccord Qaytarildi: 3.0000 Kg"
        );
    }

    fn draft() -> PurchaseReceiptDraft {
        PurchaseReceiptDraft {
            name: "PR-001".to_string(),
            supplier: "SUP-001".to_string(),
            item_code: "ITEM-001".to_string(),
            qty: 10.0,
            uom: "Kg".to_string(),
            supplier_delivery_note: "TG:+998901111111:20260116100000:10.0000".to_string(),
            ..PurchaseReceiptDraft::default()
        }
    }
}
