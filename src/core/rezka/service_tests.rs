use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::core::gscale::models::{ScaleDriverPrintRequest, ScaleDriverPrintResponse};
use crate::core::gscale::ports::{EpcSource, GscalePortError, ScaleDriverPort};
use crate::core::rezka::models::{
    CreateRezkaRepackDraftInput, RezkaRepackDraft, RezkaSourceEntry, RezkaSplitOutputRequest,
    RezkaSplitRequest,
};
use crate::core::rezka::ports::{RezkaPortError, RezkaRepackStorePort};
use crate::core::rezka::service::RezkaService;

#[tokio::test]
async fn split_creates_repack_prints_each_output_and_submits() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service = RezkaService::new()
        .with_repack_store(Arc::new(FakeRezkaRepackStore {
            events: events.clone(),
        }))
        .with_driver(Arc::new(FakeDriver {
            events: events.clone(),
        }))
        .with_epc_source(Arc::new(SeqEpc::new(["EPC-400", "EPC-200"])));

    let response = service
        .split(
            source(),
            RezkaSplitRequest {
                source_barcode: "SRC-600".to_string(),
                source_stock_entry: "MAT-STE-001".to_string(),
                source_line_index: 1,
                reason: "Zakaz uchun".to_string(),
                driver_url: "http://127.0.0.1:39117".to_string(),
                printer: "godex".to_string(),
                print_mode: "label".to_string(),
                outputs: vec![
                    RezkaSplitOutputRequest {
                        item_code: "FLEXO-400".to_string(),
                        item_name: "Flexo 400".to_string(),
                        qty: 400.0,
                        uom: "m".to_string(),
                        target_warehouse: "Work In Progress - A".to_string(),
                        reason: "400 metr zakazga".to_string(),
                        print_qr: true,
                    },
                    RezkaSplitOutputRequest {
                        item_code: "FLEXO-200".to_string(),
                        item_name: "Flexo 200".to_string(),
                        qty: 200.0,
                        uom: "m".to_string(),
                        target_warehouse: "Stores - A".to_string(),
                        reason: "qoldiq qaytdi".to_string(),
                        print_qr: true,
                    },
                ],
            },
        )
        .await
        .expect("split");

    assert!(response.ok);
    assert_eq!(response.stock_entry_name, "MAT-STE-REPACK-1");
    assert_eq!(response.outputs.len(), 2);
    assert_eq!(response.outputs[0].epc, "EPC-400");
    assert_eq!(response.outputs[1].epc, "EPC-200");
    assert_eq!(response.outputs[0].reason, "400 metr zakazga");
    assert_eq!(response.outputs[1].reason, "qoldiq qaytdi");
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            "draft:SRC-600:2:Zakaz uchun:400 metr zakazga,qoldiq qaytdi",
            "print:EPC-400:FLEXO-400:400.000:m",
            "print:EPC-200:FLEXO-200:200.000:m",
            "submit:MAT-STE-REPACK-1"
        ]
    );
}

#[tokio::test]
async fn split_keeps_scrap_in_repack_without_printing_qr() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service = RezkaService::new()
        .with_repack_store(Arc::new(FakeRezkaRepackStore {
            events: events.clone(),
        }))
        .with_driver(Arc::new(FakeDriver {
            events: events.clone(),
        }))
        .with_epc_source(Arc::new(SeqEpc::new(["EPC-550"])));

    let response = service
        .split(
            source(),
            RezkaSplitRequest {
                source_barcode: "SRC-600".to_string(),
                source_stock_entry: "MAT-STE-001".to_string(),
                source_line_index: 1,
                reason: "Zakaz uchun".to_string(),
                driver_url: "http://127.0.0.1:39117".to_string(),
                printer: "godex".to_string(),
                print_mode: "label".to_string(),
                outputs: vec![
                    RezkaSplitOutputRequest {
                        item_code: "FLEXO-550".to_string(),
                        item_name: "Flexo 550".to_string(),
                        qty: 550.0,
                        uom: "m".to_string(),
                        target_warehouse: "Work In Progress - A".to_string(),
                        reason: "550 metr zakazga".to_string(),
                        print_qr: true,
                    },
                    RezkaSplitOutputRequest {
                        qty: 50.0,
                        uom: "m".to_string(),
                        target_warehouse: "brak - ombori - A".to_string(),
                        reason: "Atxot".to_string(),
                        print_qr: false,
                        ..RezkaSplitOutputRequest::default()
                    },
                ],
            },
        )
        .await
        .expect("split");

    assert_eq!(response.outputs.len(), 1);
    assert_eq!(response.outputs[0].epc, "EPC-550");
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            "draft:SRC-600:2:Zakaz uchun:550 metr zakazga,Atxot",
            "print:EPC-550:FLEXO-550:550.000:m",
            "submit:MAT-STE-REPACK-1"
        ]
    );
}

#[tokio::test]
async fn split_does_not_print_brak_warehouse_even_when_client_requests_qr() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service = RezkaService::new()
        .with_repack_store(Arc::new(FakeRezkaRepackStore {
            events: events.clone(),
        }))
        .with_driver(Arc::new(FakeDriver {
            events: events.clone(),
        }))
        .with_epc_source(Arc::new(SeqEpc::new(["EPC-550"])));

    let response = service
        .split(
            source(),
            RezkaSplitRequest {
                source_barcode: "SRC-600".to_string(),
                source_stock_entry: "MAT-STE-001".to_string(),
                source_line_index: 1,
                reason: "Zakaz uchun".to_string(),
                driver_url: "http://127.0.0.1:39117".to_string(),
                printer: "godex".to_string(),
                print_mode: "label".to_string(),
                outputs: vec![
                    RezkaSplitOutputRequest {
                        item_code: "FLEXO-550".to_string(),
                        item_name: "Flexo 550".to_string(),
                        qty: 550.0,
                        uom: "m".to_string(),
                        target_warehouse: "Work In Progress - A".to_string(),
                        reason: "550 metr zakazga".to_string(),
                        print_qr: true,
                    },
                    RezkaSplitOutputRequest {
                        item_code: "FLEXO-RAW".to_string(),
                        item_name: "Flexo raw".to_string(),
                        qty: 50.0,
                        uom: "m".to_string(),
                        target_warehouse: "brak - ombori - A".to_string(),
                        reason: "Atxot / brak mahsulot".to_string(),
                        print_qr: true,
                    },
                ],
            },
        )
        .await
        .expect("split");

    assert_eq!(response.outputs.len(), 1);
    assert_eq!(response.outputs[0].epc, "EPC-550");
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            "draft:SRC-600:2:Zakaz uchun:550 metr zakazga,Atxot / brak mahsulot",
            "print:EPC-550:FLEXO-550:550.000:m",
            "submit:MAT-STE-REPACK-1"
        ]
    );
}

#[tokio::test]
async fn split_rejects_output_total_that_does_not_match_source_qty() {
    let service = RezkaService::new()
        .with_repack_store(Arc::new(FakeRezkaRepackStore {
            events: Arc::new(Mutex::new(Vec::new())),
        }))
        .with_driver(Arc::new(FakeDriver {
            events: Arc::new(Mutex::new(Vec::new())),
        }))
        .with_epc_source(Arc::new(SeqEpc::new(["EPC-1", "EPC-2"])));

    let error = service
        .split(
            source(),
            RezkaSplitRequest {
                source_barcode: "SRC-600".to_string(),
                outputs: vec![
                    RezkaSplitOutputRequest {
                        item_code: "FLEXO-300".to_string(),
                        qty: 300.0,
                        target_warehouse: "Stores - A".to_string(),
                        ..RezkaSplitOutputRequest::default()
                    },
                    RezkaSplitOutputRequest {
                        item_code: "FLEXO-200".to_string(),
                        qty: 200.0,
                        target_warehouse: "Stores - A".to_string(),
                        ..RezkaSplitOutputRequest::default()
                    },
                ],
                ..RezkaSplitRequest::default()
            },
        )
        .await
        .expect_err("invalid total");

    assert_eq!(error.code(), "invalid_input");
}

fn source() -> RezkaSourceEntry {
    RezkaSourceEntry {
        barcode: "SRC-600".to_string(),
        stock_entry_name: "MAT-STE-001".to_string(),
        line_index: 1,
        item_code: "FLEXO-RAW".to_string(),
        item_name: "Flexo raw".to_string(),
        qty: 600.0,
        uom: "m".to_string(),
        warehouse: "Stores - A".to_string(),
        company: "Accord".to_string(),
    }
}

struct FakeRezkaRepackStore {
    events: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl RezkaRepackStorePort for FakeRezkaRepackStore {
    async fn create_rezka_repack_draft(
        &self,
        input: CreateRezkaRepackDraftInput,
    ) -> Result<RezkaRepackDraft, RezkaPortError> {
        self.events.lock().unwrap().push(format!(
            "draft:{}:{}:{}:{}",
            input.source.barcode,
            input.outputs.len(),
            input.reason,
            input
                .outputs
                .iter()
                .map(|output| output.reason.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ));
        Ok(RezkaRepackDraft {
            name: "MAT-STE-REPACK-1".to_string(),
        })
    }

    async fn submit_rezka_repack_draft(&self, name: &str) -> Result<(), RezkaPortError> {
        self.events.lock().unwrap().push(format!("submit:{name}"));
        Ok(())
    }

    async fn delete_rezka_repack_draft(&self, name: &str) -> Result<(), RezkaPortError> {
        self.events.lock().unwrap().push(format!("delete:{name}"));
        Ok(())
    }
}

struct FakeDriver {
    events: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ScaleDriverPort for FakeDriver {
    async fn print_material_receipt(
        &self,
        request: ScaleDriverPrintRequest,
    ) -> Result<ScaleDriverPrintResponse, GscalePortError> {
        self.events.lock().unwrap().push(format!(
            "print:{}:{}:{:.3}:{}",
            request.epc, request.item_code, request.gross_qty, request.unit
        ));
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

struct SeqEpc {
    values: Mutex<Vec<String>>,
}

impl SeqEpc {
    fn new<const N: usize>(values: [&str; N]) -> Self {
        Self {
            values: Mutex::new(values.into_iter().rev().map(str::to_string).collect()),
        }
    }
}

impl EpcSource for SeqEpc {
    fn next_epc(&self) -> String {
        self.values.lock().unwrap().pop().unwrap_or_default()
    }
}
