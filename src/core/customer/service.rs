use std::collections::HashSet;
use std::sync::Arc;

use super::decision::{
    DELIVERY_ACTOR_WERKA, DELIVERY_FLOW_STATE_SUBMITTED, combine_customer_reason_and_comment,
    customer_delivery_ui_status, nearly_equal_qty, normalize_customer_delivery_decision,
    upsert_customer_decision_payload_in_remarks,
};
use super::mapping::{
    customer_delivery_status, customer_delivery_visible, delivery_note_to_dispatch_record,
    detail_from_draft,
};
use crate::core::auth::models::Principal;
use crate::core::customer::models::{
    CustomerDeliveryDetail, CustomerDeliveryResponseRequest, CustomerHomeSummary,
};
use crate::core::customer::ports::{
    CustomerDeliveryNoteDraft, CustomerDeliveryPort, CustomerServiceError,
};
use crate::core::werka::models::DispatchRecord;
use crate::core::werka::ports::DeliveryNoteStateUpdate;

#[derive(Clone, Default)]
pub struct CustomerService {
    delivery_port: Option<Arc<dyn CustomerDeliveryPort>>,
}

impl CustomerService {
    pub fn new() -> Self {
        Self {
            delivery_port: None,
        }
    }

    pub fn with_delivery_port(mut self, delivery_port: Arc<dyn CustomerDeliveryPort>) -> Self {
        self.delivery_port = Some(delivery_port);
        self
    }

    pub async fn summary(
        &self,
        principal: &Principal,
    ) -> Result<Option<CustomerHomeSummary>, CustomerServiceError> {
        let items = match self
            .collect_customer_delivery_notes(&principal.ref_)
            .await?
        {
            Some(items) => items,
            None => return Ok(None),
        };
        let mut summary = CustomerHomeSummary::default();
        for item in items.iter().filter(|item| customer_delivery_visible(item)) {
            match customer_delivery_status(item) {
                "accepted" => summary.confirmed_count += 1,
                "partial" | "rejected" => summary.rejected_count += 1,
                _ => summary.pending_count += 1,
            }
        }
        Ok(Some(summary))
    }

    pub async fn history(
        &self,
        principal: &Principal,
    ) -> Result<Option<Vec<DispatchRecord>>, CustomerServiceError> {
        let items = match self
            .collect_customer_delivery_notes(&principal.ref_)
            .await?
        {
            Some(items) => items,
            None => return Ok(None),
        };
        Ok(Some(
            items
                .into_iter()
                .filter(customer_delivery_visible)
                .map(delivery_note_to_dispatch_record)
                .collect(),
        ))
    }

    pub async fn status_details(
        &self,
        principal: &Principal,
        kind: &str,
    ) -> Result<Option<Vec<DispatchRecord>>, CustomerServiceError> {
        let items = match self
            .collect_customer_delivery_notes(&principal.ref_)
            .await?
        {
            Some(items) => items,
            None => return Ok(None),
        };
        let mut filter_kind = kind.trim();
        if filter_kind == "confirmed" {
            filter_kind = "accepted";
        }
        Ok(Some(
            items
                .into_iter()
                .filter(customer_delivery_visible)
                .filter(|item| {
                    let status = customer_delivery_status(item);
                    if filter_kind == "rejected" {
                        status == "rejected" || status == "partial"
                    } else {
                        status == filter_kind
                    }
                })
                .map(delivery_note_to_dispatch_record)
                .collect(),
        ))
    }

    pub async fn detail(
        &self,
        principal: &Principal,
        delivery_note_id: &str,
    ) -> Result<Option<CustomerDeliveryDetail>, CustomerServiceError> {
        let Some(port) = &self.delivery_port else {
            return Ok(None);
        };
        let draft = port.get_delivery_note(delivery_note_id.trim()).await?;
        if draft.customer.trim() != principal.ref_.trim() {
            return Err(CustomerServiceError::Unauthorized);
        }
        Ok(Some(detail_from_draft(draft)))
    }

    pub async fn respond(
        &self,
        principal: &Principal,
        request: CustomerDeliveryResponseRequest,
    ) -> Result<Option<CustomerDeliveryDetail>, CustomerServiceError> {
        let Some(port) = &self.delivery_port else {
            return Ok(None);
        };
        let mut draft = port
            .get_delivery_note(request.delivery_note_id.trim())
            .await?;
        if draft.customer.trim() != principal.ref_.trim() {
            return Err(CustomerServiceError::Unauthorized);
        }
        let decision = normalize_customer_delivery_decision(&request, &draft)?;
        if decision.returned_qty > 0.0 {
            if nearly_equal_qty(decision.returned_qty, draft.qty) {
                port.create_and_submit_delivery_note_return(&draft.name)
                    .await?;
            } else {
                port.create_and_submit_partial_delivery_note_return(
                    &draft.name,
                    decision.returned_qty,
                )
                .await?;
            }
        }
        let combined_reason =
            combine_customer_reason_and_comment(&decision.reason, &decision.comment);
        let remarks = upsert_customer_decision_payload_in_remarks(
            &draft.remarks,
            decision.state_label(),
            &decision.reason,
            decision.accepted_qty,
            decision.returned_qty,
            &draft.uom,
            &decision.comment,
        );
        if remarks != draft.remarks.trim() {
            port.update_delivery_note_remarks(&draft.name, &remarks)
                .await?;
        }
        port.update_delivery_note_state(
            &draft.name,
            DeliveryNoteStateUpdate {
                flow_state: DELIVERY_FLOW_STATE_SUBMITTED.to_string(),
                customer_state: decision.customer_state.to_string(),
                customer_reason: combined_reason.clone(),
                delivery_actor: DELIVERY_ACTOR_WERKA.to_string(),
                ui_status: customer_delivery_ui_status(
                    DELIVERY_FLOW_STATE_SUBMITTED,
                    decision.customer_state,
                )
                .to_string(),
            },
        )
        .await?;

        draft.remarks = remarks;
        draft.accord_flow_state = DELIVERY_FLOW_STATE_SUBMITTED.to_string();
        draft.accord_customer_state = decision.customer_state.to_string();
        draft.accord_customer_reason = combined_reason;
        draft.accord_delivery_actor = DELIVERY_ACTOR_WERKA.to_string();
        draft.accord_ui_status =
            customer_delivery_ui_status(DELIVERY_FLOW_STATE_SUBMITTED, decision.customer_state)
                .to_string();

        Ok(Some(CustomerDeliveryDetail {
            record: delivery_note_to_dispatch_record(draft),
            can_approve: false,
            can_reject: false,
            can_partially_accept: false,
            can_report_claim: false,
        }))
    }

    async fn collect_customer_delivery_notes(
        &self,
        customer_ref: &str,
    ) -> Result<Option<Vec<CustomerDeliveryNoteDraft>>, CustomerServiceError> {
        let Some(port) = &self.delivery_port else {
            return Ok(None);
        };
        const PAGE_SIZE: usize = 200;
        let mut result = Vec::with_capacity(PAGE_SIZE);
        let mut seen = HashSet::with_capacity(PAGE_SIZE);
        let mut offset = 0;
        loop {
            let items = port
                .list_customer_delivery_notes_page(customer_ref, PAGE_SIZE, offset)
                .await?;
            for item in &items {
                let name = item.name.trim();
                if name.is_empty() || !seen.insert(name.to_string()) {
                    continue;
                }
                result.push(item.clone());
            }
            if items.len() < PAGE_SIZE {
                return Ok(Some(result));
            }
            offset += PAGE_SIZE;
        }
    }
}
