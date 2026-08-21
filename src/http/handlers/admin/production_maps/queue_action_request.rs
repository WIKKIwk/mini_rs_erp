#[derive(serde::Deserialize)]
struct ApparatusQueueActionRequest {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    material_barcode: String,
    #[serde(default)]
    material_barcodes: Vec<String>,
    #[serde(default)]
    qolip_code: String,
    #[serde(default)]
    qolip_codes: Vec<String>,
    #[serde(default)]
    produced_qty: Option<f64>,
    #[serde(default)]
    qty: Option<f64>,
    #[serde(default)]
    gross_qty: Option<f64>,
    #[serde(default)]
    return_ink_kg: Option<f64>,
    #[serde(default)]
    lamination_print_leftover_rolls: Option<f64>,
    #[serde(default)]
    lamination_film_leftover_rolls: Option<f64>,
    #[serde(default)]
    rezka_bosma_waste: Option<f64>,
    #[serde(default)]
    rezka_lamination_waste: Option<f64>,
    #[serde(default)]
    rezka_edge_waste: Option<f64>,
    #[serde(default)]
    total_waste: Option<f64>,
    #[serde(default)]
    finished_goods_kg: Option<f64>,
    #[serde(default, alias = "babina_kg")]
    bobina_kg: Option<f64>,
    #[serde(default)]
    finished_goods_meter: Option<f64>,
    #[serde(default)]
    diameter: Option<f64>,
    #[serde(default)]
    uom: String,
    #[serde(default)]
    unit: String,
    #[serde(default)]
    progress_batch_id: String,
    #[serde(default)]
    progress_qr: String,
    #[serde(default)]
    qr_payload: String,
    #[serde(default)]
    driver_url: String,
    #[serde(default)]
    printer: String,
    #[serde(default)]
    print_mode: String,
    #[serde(default)]
    customer_name: String,
    #[serde(default)]
    print_count: u32,
    #[serde(default)]
    print_transport: String,
    #[serde(default)]
    completion_request_note: String,
    #[serde(default)]
    full_completion_report_required: bool,
    #[serde(default)]
    worker_handoff: bool,
    #[serde(default)]
    remove_roll_from_apparatus: bool,
    #[serde(default)]
    description: String,
    #[serde(default)]
    returned_paint_items: Vec<ReturnedPaintItem>,
    #[serde(default)]
    returned_paint_image_id: String,
    #[serde(default)]
    freeze_request_id: String,
    #[serde(default)]
    freeze_with_issue: bool,
    #[serde(default)]
    issue_note: String,
    #[serde(default)]
    rezka_frames: Vec<RezkaFrameProgressInput>,
    action: queue_state::ApparatusQueueAction,
}
