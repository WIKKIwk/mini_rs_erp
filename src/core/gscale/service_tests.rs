use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Notify;

use super::*;
use crate::core::gscale::models::{MaterialReceiptDraft, ScaleDriverPrintResponse};
use crate::core::gscale::ports::GscalePortError;

fn request() -> MaterialReceiptPrintRequest {
    MaterialReceiptPrintRequest {
        driver_url: "http://127.0.0.1:39117".to_string(),
        item_code: " ITEM-1 ".to_string(),
        item_name: " Green Tea ".to_string(),
        warehouse: " Stores - A ".to_string(),
        printer: "zebra".to_string(),
        print_mode: "rfid".to_string(),
        gross_qty: 2.5,
        unit: String::new(),
        tare_enabled: true,
        tare_kg: 0.78,
        print_count: 1,
        actor_role: String::new(),
        actor_ref: String::new(),
        actor_display_name: String::new(),
    }
}

#[tokio::test]
async fn rejects_small_gross_and_net_qty_before_receipt_store() {
    let service = GscaleService::new()
        .with_receipt_store(Arc::new(FakeReceiptStore::new(Arc::new(Mutex::new(
            Vec::new(),
        )))))
        .with_driver(Arc::new(FakeDriver::done(Arc::new(Mutex::new(Vec::new())))));
    let mut gross = request();
    gross.gross_qty = 0.099;
    let mut net = request();
    net.gross_qty = 0.5;
    net.tare_kg = 0.45;

    let gross_error = service
        .print_material_receipt_driver_first(gross)
        .await
        .unwrap_err();
    let net_error = service
        .print_material_receipt_driver_first(net)
        .await
        .unwrap_err();

    assert_eq!(
        gross_error.to_string(),
        "invalid input: QTY juda kichik: 0.099 kg | min 0.100 kg"
    );
    assert_eq!(
        net_error.to_string(),
        "invalid input: NETTO juda kichik: brutto 0.500 kg - babina 0.450 kg = 0.050 kg | min 0.100 kg"
    );
}

#[tokio::test]
async fn driver_first_starts_draft_before_slow_driver_finishes_and_submits_after_print_success() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let print_gate = Arc::new(Notify::new());
    let service = GscaleService::new()
        .with_receipt_store(Arc::new(FakeReceiptStore::new(events.clone())))
        .with_driver(Arc::new(GatedDriver {
            events: events.clone(),
            print_gate: print_gate.clone(),
        }))
        .with_epc_source(Arc::new(QueueEpc::new(["EPC-1"])));

    let print_task =
        tokio::spawn(async move { service.print_material_receipt_driver_first(request()).await });

    wait_for_event(&events, "create:EPC-1:1.720").await;
    assert!(
        !events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event == "submit:MAT-STE-001"),
        "draft must not submit before printer success"
    );

    print_gate.notify_one();
    let response = print_task.await.unwrap().unwrap();
    assert_eq!(response.status, "printed");

    wait_for_event(&events, "submit:MAT-STE-001").await;
    let events = events.lock().unwrap().clone();
    let create_pos = events
        .iter()
        .position(|event| event == "create:EPC-1:1.720")
        .unwrap();
    let print_done_pos = events
        .iter()
        .position(|event| event == "print:done:EPC-1")
        .unwrap();
    let submit_pos = events
        .iter()
        .position(|event| event == "submit:MAT-STE-001")
        .unwrap();
    assert!(
        create_pos < print_done_pos,
        "receipt draft must start while printer request is still in flight: {events:?}"
    );
    assert!(
        print_done_pos < submit_pos,
        "receipt submit must wait for printer success: {events:?}"
    );
}

#[tokio::test]
async fn notifies_warehouse_after_successful_material_receipt_submit() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let notifications = Arc::new(Mutex::new(Vec::new()));
    let notifications_for_handler = notifications.clone();
    let service = GscaleService::new()
        .with_receipt_store(Arc::new(FakeReceiptStore::new(events.clone())))
        .with_driver(Arc::new(FakeDriver::done(events.clone())))
        .with_epc_source(Arc::new(QueueEpc::new(["EPC-LIVE"])))
        .with_warehouse_event_handler(Arc::new(move |warehouse, reason| {
            notifications_for_handler
                .lock()
                .unwrap()
                .push(format!("{warehouse}:{reason}"));
        }));

    let response = service
        .print_material_receipt_driver_first(request())
        .await
        .unwrap();

    assert_eq!(response.status, "printed");
    wait_for_event(&events, "submit:MAT-STE-001").await;
    for _ in 0..50 {
        if notifications
            .lock()
            .unwrap()
            .iter()
            .any(|event| event == "Stores - A:raw_material_stock")
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "timed out waiting for warehouse event; events={:?}",
        notifications.lock().unwrap()
    );
}

#[tokio::test]
async fn forwards_print_count_to_driver_without_creating_extra_receipt_drafts() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service = GscaleService::new()
        .with_receipt_store(Arc::new(FakeReceiptStore::new(events.clone())))
        .with_driver(Arc::new(FakeDriver::done(events.clone())))
        .with_epc_source(Arc::new(QueueEpc::new(["EPC-DUP"])));
    let mut request = request();
    request.print_count = 5;

    let response = service
        .print_material_receipt_driver_first(request)
        .await
        .unwrap();

    assert_eq!(response.status, "printed");
    assert_eq!(response.print_count, 5);
    wait_for_event(&events, "submit:MAT-STE-001").await;
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            "print:EPC-DUP:5",
            "create:EPC-DUP:1.720",
            "submit:MAT-STE-001"
        ]
    );
}

#[tokio::test]
async fn progress_label_prints_without_receipt_store() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service = GscaleService::new().with_driver(Arc::new(FakeDriver::done(events.clone())));

    let response = service
        .print_progress_label(ProgressLabelPrintRequest {
            qr_payload: "GSP:PROGRESS-1".to_string(),
            item_code: "zakaz-1202".to_string(),
            item_name: "Vesta yarim tayyor, 7 ta rangli pechat holatda, pauza".to_string(),
            executor_name: "Ali".to_string(),
            gross_qty: 12.5,
            unit: "kg".to_string(),
            print_count: 1,
            ..ProgressLabelPrintRequest::default()
        })
        .await
        .unwrap();

    assert_eq!(response.status, "printed");
    assert_eq!(response.qr_payload, "GSP:PROGRESS-1");
    assert_eq!(response.executor_name, "Ali");
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["print:progress:GSP:PROGRESS-1:Ali:1"]
    );
}

#[test]
fn progress_label_can_be_prepared_without_calling_a_driver() {
    let service = GscaleService::new();

    let response = service
        .prepare_progress_label(ProgressLabelPrintRequest {
            qr_payload: "GSP:PROGRESS-2".to_string(),
            item_code: "ORDER-2".to_string(),
            item_name: "Progress label".to_string(),
            executor_name: "Ali".to_string(),
            gross_qty: 20.0,
            progress_qty: 125.0,
            unit: "kg".to_string(),
            progress_unit: "m".to_string(),
            label_kind: "progress".to_string(),
            print_count: 2,
            ..ProgressLabelPrintRequest::default()
        })
        .expect("prepare progress label");

    assert_eq!(response.status, "prepared");
    assert_eq!(response.printer_status, "client_usb_pending");
    assert_eq!(response.label_kind, "progress");
    assert_eq!(response.qty, 125.0);
    assert_eq!(response.print_count, 2);
}

#[tokio::test]
async fn material_receipt_client_print_records_only_after_usb_confirmation() {
    const EPC: &str = "303132333435363738394142";
    let events = Arc::new(Mutex::new(Vec::new()));
    let service = GscaleService::new()
        .with_receipt_store(Arc::new(FakeReceiptStore::new(events.clone())))
        .with_epc_source(Arc::new(QueueEpc::new([EPC])));
    let input = request();

    let prepared = service
        .prepare_material_receipt_client_print(input.clone())
        .expect("prepare client print");

    assert_eq!(prepared.status, "prepared");
    assert_eq!(prepared.epc, EPC);
    assert!(events.lock().unwrap().is_empty());

    let confirmed = service
        .confirm_material_receipt_client_print(input, &prepared.epc)
        .await
        .expect("confirm client print");

    assert_eq!(confirmed.status, "printed");
    assert_eq!(confirmed.draft_name, "MAT-STE-001");
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            "create:303132333435363738394142:1.720",
            "submit:MAT-STE-001"
        ]
    );
}

async fn wait_for_event(events: &Arc<Mutex<Vec<String>>>, needle: &str) {
    for _ in 0..50 {
        if events.lock().unwrap().iter().any(|event| event == needle) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "timed out waiting for {needle}; events={:?}",
        events.lock().unwrap()
    );
}

struct QueueEpc {
    values: Mutex<VecDeque<String>>,
}

impl QueueEpc {
    fn new(values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            values: Mutex::new(values.into_iter().map(Into::into).collect()),
        }
    }
}

impl EpcSource for QueueEpc {
    fn next_epc(&self) -> String {
        self.values.lock().unwrap().pop_front().unwrap_or_default()
    }
}

struct FakeReceiptStore {
    events: Arc<Mutex<Vec<String>>>,
    duplicate_failures: usize,
    submit_error: Option<String>,
}

impl FakeReceiptStore {
    fn new(events: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            events,
            duplicate_failures: 0,
            submit_error: None,
        }
    }
}

#[async_trait]
impl MaterialReceiptStorePort for FakeReceiptStore {
    async fn create_material_receipt_draft(
        &self,
        input: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError> {
        self.events
            .lock()
            .unwrap()
            .push(format!("create:{}:{:.3}", input.barcode, input.qty));
        if self.duplicate_failures > 0
            && self.events.lock().unwrap().len() <= self.duplicate_failures
        {
            return Err(GscalePortError::StoreWrite(
                "barcode duplicate entry".to_string(),
            ));
        }
        Ok(MaterialReceiptDraft {
            name: "MAT-STE-001".to_string(),
            item_code: input.item_code,
            warehouse: input.warehouse,
            qty: input.qty,
            uom: "Kg".to_string(),
            barcode: input.barcode,
        })
    }

    async fn submit_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("submit:{name}"));
        if let Some(error) = &self.submit_error {
            return Err(GscalePortError::StoreWrite(error.clone()));
        }
        Ok(())
    }

    async fn delete_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("delete:{name}"));
        Ok(())
    }
}

struct FakeDriver {
    events: Arc<Mutex<Vec<String>>>,
    status: &'static str,
    ok: bool,
}

impl FakeDriver {
    fn done(events: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            events,
            status: "done",
            ok: true,
        }
    }
}

#[async_trait]
impl ScaleDriverPort for FakeDriver {
    async fn print_material_receipt(
        &self,
        request: ScaleDriverPrintRequest,
    ) -> Result<ScaleDriverPrintResponse, GscalePortError> {
        let event = if request.label_kind.trim().is_empty() {
            format!("print:{}:{}", request.epc, request.print_count)
        } else {
            format!(
                "print:{}:{}:{}:{}",
                request.label_kind, request.epc, request.executor_name, request.print_count
            )
        };
        self.events.lock().unwrap().push(event);
        Ok(ScaleDriverPrintResponse {
            ok: self.ok,
            status: self.status.to_string(),
            epc: request.epc,
            printer: request.printer,
            mode: request.print_mode,
            printer_status: self.status.to_string(),
            detail: "printer rejected".to_string(),
            ..ScaleDriverPrintResponse::default()
        })
    }
}

struct GatedDriver {
    events: Arc<Mutex<Vec<String>>>,
    print_gate: Arc<Notify>,
}

#[async_trait]
impl ScaleDriverPort for GatedDriver {
    async fn print_material_receipt(
        &self,
        request: ScaleDriverPrintRequest,
    ) -> Result<ScaleDriverPrintResponse, GscalePortError> {
        self.events
            .lock()
            .unwrap()
            .push(format!("print:start:{}", request.epc));
        self.print_gate.notified().await;
        self.events
            .lock()
            .unwrap()
            .push(format!("print:done:{}", request.epc));
        Ok(ScaleDriverPrintResponse {
            ok: true,
            status: "done".to_string(),
            epc: request.epc,
            printer: request.printer,
            mode: request.print_mode,
            printer_status: "OK".to_string(),
            ..ScaleDriverPrintResponse::default()
        })
    }
}
