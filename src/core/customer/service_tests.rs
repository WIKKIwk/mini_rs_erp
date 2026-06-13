use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::models::{CustomerDeliveryResponseMode, CustomerDeliveryResponseRequest};
use super::ports::{CustomerDeliveryNoteDraft, CustomerDeliveryPort, CustomerPortError};
use super::service::CustomerService;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::werka::ports::DeliveryNoteStateUpdate;

#[tokio::test]
async fn summary_counts_visible_delivery_notes_like_go() {
    let service = CustomerService::new().with_delivery_port(Arc::new(FakeDeliveryPort::new(vec![
        delivery("DN-PENDING", "1", "", 5.0),
        delivery("DN-ACCEPTED", "1", "3", 5.0),
        delivery("DN-PARTIAL", "1", "4", 5.0),
        delivery("DN-REJECTED", "1", "2", 5.0),
        delivery("DN-DRAFT", "0", "1", 5.0),
    ])));

    let summary = service
        .summary(&principal())
        .await
        .expect("summary")
        .expect("provider");

    assert_eq!(summary.pending_count, 1);
    assert_eq!(summary.confirmed_count, 1);
    assert_eq!(summary.rejected_count, 2);
}

#[tokio::test]
async fn status_details_confirmed_maps_to_accepted_like_go() {
    let service = CustomerService::new().with_delivery_port(Arc::new(FakeDeliveryPort::new(vec![
        delivery("DN-PENDING", "1", "", 5.0),
        delivery("DN-ACCEPTED", "1", "3", 5.0),
    ])));

    let items = service
        .status_details(&principal(), "confirmed")
        .await
        .expect("details")
        .expect("provider");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "DN-ACCEPTED");
    assert_eq!(items[0].status, "accepted");
}

#[tokio::test]
async fn reject_requires_meaningful_reason_like_go() {
    let service =
        CustomerService::new().with_delivery_port(Arc::new(FakeDeliveryPort::new(vec![delivery(
            "DN-PENDING",
            "1",
            "",
            5.0,
        )])));

    let error = service
        .respond(
            &principal(),
            CustomerDeliveryResponseRequest {
                delivery_note_id: "DN-PENDING".to_string(),
                approve: Some(false),
                ..CustomerDeliveryResponseRequest::default()
            },
        )
        .await
        .expect_err("invalid input");

    assert_eq!(error.to_string(), "invalid input");
}

#[tokio::test]
async fn partial_response_writes_return_and_state_like_go() {
    let port = Arc::new(FakeDeliveryPort::new(vec![delivery(
        "DN-PENDING",
        "1",
        "",
        10.0,
    )]));
    let service = CustomerService::new().with_delivery_port(port.clone());

    let detail = service
        .respond(
            &principal(),
            CustomerDeliveryResponseRequest {
                delivery_note_id: "DN-PENDING".to_string(),
                mode: Some(CustomerDeliveryResponseMode::AcceptPartial),
                returned_qty: 3.0,
                reason: "Brak".to_string(),
                comment: "3 kg qaytdi".to_string(),
                ..CustomerDeliveryResponseRequest::default()
            },
        )
        .await
        .expect("respond")
        .expect("provider");

    assert_eq!(detail.record.status, "partial");
    assert_eq!(detail.record.accepted_qty, 7.0);
    assert!(detail.record.note.contains("Sabab: Brak. 3 kg qaytdi"));
    let calls = port.calls.lock().expect("calls");
    assert!(
        calls
            .iter()
            .any(|call| call == "partial_return:DN-PENDING:3")
    );
    assert!(
        calls
            .iter()
            .any(|call| call == "state:DN-PENDING:4:partial")
    );
    assert!(
        calls
            .iter()
            .any(|call| call.contains("remarks:DN-PENDING:AC:partial"))
    );
}

struct FakeDeliveryPort {
    notes: Mutex<Vec<CustomerDeliveryNoteDraft>>,
    calls: Mutex<Vec<String>>,
}

impl FakeDeliveryPort {
    fn new(notes: Vec<CustomerDeliveryNoteDraft>) -> Self {
        Self {
            notes: Mutex::new(notes),
            calls: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl CustomerDeliveryPort for FakeDeliveryPort {
    async fn list_customer_delivery_notes_page(
        &self,
        _customer: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDeliveryNoteDraft>, CustomerPortError> {
        let notes = self.notes.lock().expect("notes");
        Ok(notes.iter().skip(offset).take(limit).cloned().collect())
    }

    async fn get_delivery_note(
        &self,
        name: &str,
    ) -> Result<CustomerDeliveryNoteDraft, CustomerPortError> {
        self.notes
            .lock()
            .expect("notes")
            .iter()
            .find(|note| note.name == name)
            .cloned()
            .ok_or_else(|| CustomerPortError::Failed("not found".to_string()))
    }

    async fn create_and_submit_delivery_note_return(
        &self,
        source_name: &str,
    ) -> Result<(), CustomerPortError> {
        self.calls
            .lock()
            .expect("calls")
            .push(format!("return:{source_name}"));
        Ok(())
    }

    async fn create_and_submit_partial_delivery_note_return(
        &self,
        source_name: &str,
        returned_qty: f64,
    ) -> Result<(), CustomerPortError> {
        self.calls
            .lock()
            .expect("calls")
            .push(format!("partial_return:{source_name}:{returned_qty}"));
        Ok(())
    }

    async fn update_delivery_note_remarks(
        &self,
        name: &str,
        remarks: &str,
    ) -> Result<(), CustomerPortError> {
        self.calls
            .lock()
            .expect("calls")
            .push(format!("remarks:{name}:{remarks}"));
        Ok(())
    }

    async fn update_delivery_note_state(
        &self,
        name: &str,
        update: DeliveryNoteStateUpdate,
    ) -> Result<(), CustomerPortError> {
        self.calls.lock().expect("calls").push(format!(
            "state:{name}:{}:{}",
            update.customer_state, update.ui_status
        ));
        Ok(())
    }
}

fn principal() -> Principal {
    Principal {
        role: PrincipalRole::Customer,
        display_name: "Customer".to_string(),
        legal_name: "Customer".to_string(),
        ref_: "CUST-001".to_string(),
        phone: "+998901234567".to_string(),
        avatar_url: String::new(),
    }
}

fn delivery(
    name: &str,
    flow_state: &str,
    customer_state: &str,
    qty: f64,
) -> CustomerDeliveryNoteDraft {
    CustomerDeliveryNoteDraft {
        name: name.to_string(),
        customer: "CUST-001".to_string(),
        customer_name: "Comfi".to_string(),
        posting_date: "2026-01-01".to_string(),
        modified: "2026-01-02 10:00:00".to_string(),
        doc_status: 1,
        accord_flow_state: flow_state.to_string(),
        accord_customer_state: customer_state.to_string(),
        item_code: "ITEM-001".to_string(),
        item_name: "Item".to_string(),
        qty,
        uom: "Kg".to_string(),
        ..CustomerDeliveryNoteDraft::default()
    }
}
