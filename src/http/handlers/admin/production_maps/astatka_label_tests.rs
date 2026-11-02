use super::*;
use crate::config::AppConfig;
use crate::core::production_map::*;
use serde_json::{Value, json};
use std::time::Duration;

fn config(path: &std::path::Path) -> AppConfig {
    AppConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        default_target_warehouse: String::new(),
        http_timeout: Duration::from_secs(1),
        session_store_path: path.join("sessions.json"),
        profile_store_path: path.join("profiles.json"),
        push_token_store_path: path.join("push.json"),
        session_ttl_seconds: Some(60),
        supplier_prefix: "10".into(),
        werka_prefix: "20".into(),
        werka_code: String::new(),
        werka_name: String::new(),
        werka_phone: String::new(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: String::new(),
        material_taminotchi_phone: String::new(),
        admin_phone: String::new(),
        admin_name: String::new(),
        admin_code: String::new(),
    }
}

fn result() -> LaminatsiyaAstatkaCommitResult {
    let batch: OrderProgressBatch = serde_json::from_value(json!({
        "batch_id":"printed-remainder","session_id":"original-print","started_at_unix":1,"completed_at_unix":2,
        "apparatus":"apparatus:default:bosma_8","order_id":"zakaz-label","action":"complete","status":"completed",
        "produced_qty":600,"uom":"m","qr_payload":"40010000000000000000ABCD",
        "label_item_code":"PRODUCT","label_item_name":"Printed PET","executor_name":"Original Printer",
        "worker_role":"aparatchi","worker_ref":"printer","worker_display_name":"Original Printer",
        "wip_status":"waiting","current_apparatus":"apparatus:default:asset-007","current_location":"Lamination",
        "next_apparatus":"apparatus:default:asset-007","finished_goods_kg":22,"bobina_kg":2,"finished_goods_meter":600,
        "payload_json":{"origin":"layer_remainder","gross_qty":22,"customer_name":"Original Customer","order_code":"0042"}
    })).unwrap();
    let record: LayerRemainderRecord = serde_json::from_value(json!({
        "operation_id":"printed-return","apparatus":"apparatus:default:asset-007","order_id":"zakaz-label",
        "session_id":"lam-session","source_kind":"progress_batch","source_id":"original-printed",
        "qr_payload":batch.qr_payload,"source_net_kg":50,"source_meter":1500,"tare_kg":2,
        "producer_name":"Original Printer","remaining_net_kg":20,"remaining_meter":600,
        "recorded_by":{"role":"aparatchi","ref_":"laminator","display_name":"Laminator"},
        "created_at_unix":3,"progress_batch":batch,"raw_material_stock":null
    })).unwrap();
    let mut raw = record.clone();
    raw.operation_id = "film-return".into();
    raw.source_kind = "raw_material".into();
    raw.source_id = "original-film".into();
    raw.producer_name = "Original Supplier".into();
    raw.qr_payload = "20010000000000000000ABCD".into();
    raw.progress_batch = None;
    raw.raw_material_stock = Some(
        serde_json::from_value(json!({
            "id":"raw-child","warehouse":"Film Warehouse","item_code":"CPP","item_name":"CPP",
            "barcode":raw.qr_payload,"qty":20,"width_mm":1000,"micron":25,"length_m":600,
            "uom":"kg","status":"available","source_receipt_id":"original-receipt","payload_json":{}
        }))
        .unwrap(),
    );
    let session = serde_json::from_value(json!({
        "session_id":"lam-session","apparatus":"apparatus:default:asset-007","order_id":"zakaz-label",
        "status":"completed","worker_role":"aparatchi","worker_ref":"laminator","worker_display_name":"Laminator",
        "started_at_unix":1,"updated_at_unix":3,"payload_json":{}
    })).unwrap();
    LaminatsiyaAstatkaCommitResult {
        action: queue_state::ApparatusQueueAction::Complete,
        request_id: "stable-request".into(),
        report: None,
        session,
        progress_batches: vec![],
        remainders: vec![record, raw],
    }
}

async fn response(state: &AppState, result: LaminatsiyaAstatkaCommitResult) -> Value {
    let response = laminatsiya_astatka_shift_response(
        state,
        result,
        &super::super::layer_remainders::LayerRemainderPrintOptions {
            print_transport: "offline".into(),
            print_count: 1,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn independent_label_policy_preserves_receipts_and_source_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::new(config(directory.path()));
    state.apparatus.bootstrap_factory_defaults().await.unwrap();
    for wip in [false, true] {
        for film in [false, true] {
            state
                .production_maps
                .update_lamination_label_settings(
                    LaminationLabelSettingsPatch {
                        print_wip_remainder_labels: Some(wip),
                        print_film_remainder_labels: Some(film),
                    },
                    &QueueActionActor {
                        role: "admin".into(),
                        ref_: "admin".into(),
                        display_name: "Admin".into(),
                    },
                )
                .await
                .unwrap();
            let value = response(&state, result()).await;
            assert_eq!(value["ok"], true);
            assert_eq!(value["remainders"].as_array().unwrap().len(), 2);
            let prints = value["prints"].as_array().unwrap();
            assert_eq!(prints.len(), usize::from(wip) + usize::from(film));
            for print in prints {
                assert_eq!(print["ok"], true, "{print}");
                assert_eq!(print["gross_qty"], 22.0);
                assert_eq!(print["net_qty"], 20.0);
                assert_eq!(print["progress_qty"], 600.0);
                if print["label_kind"] == "progress" {
                    assert_eq!(print["executor_name"], "Original Printer");
                    assert_eq!(print["customer_name"], "Original Customer");
                    assert_eq!(print["order_code"], "0042");
                } else {
                    assert_eq!(print["label_kind"], "material_product");
                    assert_eq!(print["executor_name"], "Original Supplier");
                    assert_eq!(print["item_name"], "CPP 1000/25");
                    assert_eq!(print["warehouse"], "Film Warehouse");
                }
            }
        }
    }
}

#[tokio::test]
async fn zero_returns_and_label_failure_do_not_change_successful_receipts() {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::new(config(directory.path()));
    let mut saved = result();
    saved.remainders[0].remaining_net_kg = 0.0;
    saved.remainders[0].qr_payload.clear();
    saved.remainders[1]
        .raw_material_stock
        .as_mut()
        .unwrap()
        .item_code
        .clear();
    let value = response(&state, saved).await;
    assert_eq!(value["ok"], true);
    assert_eq!(value["remainders"].as_array().unwrap().len(), 2);
    assert_eq!(value["prints"].as_array().unwrap().len(), 1);
    assert_eq!(value["prints"][0]["ok"], false);
}

#[tokio::test]
async fn disabling_remainder_labels_keeps_the_ordinary_output_label() {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::new(config(directory.path()));
    state
        .production_maps
        .update_lamination_label_settings(
            LaminationLabelSettingsPatch {
                print_wip_remainder_labels: Some(false),
                print_film_remainder_labels: Some(false),
            },
            &QueueActionActor {
                role: "admin".into(),
                ref_: "admin".into(),
                display_name: "Admin".into(),
            },
        )
        .await
        .unwrap();
    let mut saved = result();
    let mut output = saved.remainders[0].progress_batch.clone().unwrap();
    output.batch_id = "ordinary-output".into();
    output.qr_payload = "40010000000000000000DCBA".into();
    let output_qr = output.qr_payload.clone();
    saved.progress_batches.push(output);
    let value = response(&state, saved).await;
    assert_eq!(value["ok"], true);
    assert_eq!(value["prints"].as_array().unwrap().len(), 1);
    assert_eq!(value["prints"][0]["ok"], true);
    assert_eq!(value["prints"][0]["qr_payload"], output_qr);
    assert_eq!(value["remainders"].as_array().unwrap().len(), 2);
}
