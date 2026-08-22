
#[derive(Default, Deserialize)]
pub struct TrainingMapsQuery {
    #[serde(default)]
    id: String,
}

#[derive(Deserialize)]
struct TrainingMapSaveWithOrderRequest {
    map: ProductionMapDefinition,
    template: CalculateOrderTemplate,
}

async fn snapshot_new_training_order_rezka_kadr_count(
    state: &AppState,
    map: &mut ProductionMapDefinition,
    template: &CalculateOrderTemplate,
) -> Result<(), AdminError> {
    if map.order_number.trim().is_empty() {
        let cut_apparatus_ids = super::production_maps::canonical_cut_apparatus_ids(state).await?;
        super::production_maps::apply_order_rezka_kadr_count(map, template, &cut_apparatus_ids);
    }
    Ok(())
}

#[derive(Default, Deserialize)]
struct TrainingApparatusModeInput {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    enabled: bool,
}

#[derive(Default, Deserialize)]
struct TrainingRestartInput {
    #[serde(default)]
    apparatus: String,
}

#[derive(Default, Deserialize)]
pub struct TrainingRawMaterialAssignmentsQuery {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    barcode: String,
}

#[derive(Default, Deserialize)]
pub struct TrainingInputBatchesQuery {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    qr_payload: String,
}

#[derive(Default, Deserialize)]
struct TrainingInputBatchRequest {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    count: Option<usize>,
}

#[derive(Default)]
pub(super) struct WorkerTrainingOverlay {
    pub active_apparatuses: Vec<String>,
    pub maps: Vec<ProductionMapSaved>,
    pub sequences: BTreeMap<String, Vec<String>>,
    pub visible_order_ids: BTreeMap<String, Vec<String>>,
    pub queue_states: BTreeMap<String, BTreeMap<String, String>>,
    pub queue_policies: Vec<ApparatusQueuePolicyRecord>,
    pub queue_action_controls: BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
    pub input_progress_batches: BTreeMap<String, Vec<OrderProgressBatch>>,
    pub order_statuses: BTreeMap<String, ProductionOrderStatusDetail>,
    pub order_customers: BTreeMap<String, String>,
}

#[derive(Default)]
pub(super) struct TrainingQueuePrintInput {
    pub driver_url: String,
    pub printer: String,
    pub print_mode: String,
    pub print_transport: String,
    pub progress_qty: Option<f64>,
    pub gross_qty: Option<f64>,
    pub finished_goods_kg: Option<f64>,
    pub bobina_kg: Option<f64>,
    pub return_ink_kg: Option<f64>,
    pub lamination_print_leftover_rolls: Option<f64>,
    pub lamination_film_leftover_rolls: Option<f64>,
    pub rezka_bosma_waste: Option<f64>,
    pub rezka_lamination_waste: Option<f64>,
    pub rezka_edge_waste: Option<f64>,
    pub total_waste: Option<f64>,
    pub finished_goods_meter: Option<f64>,
    pub diameter: Option<f64>,
    pub returned_paint_items: Vec<ReturnedPaintItem>,
    pub returned_paint_image_id: String,
    pub description: String,
    pub uom: String,
    pub customer_name: String,
    pub print_count: u32,
}

const TRAINING_INPUT_NODE_ROLE: &str = "training_input";
const TRAINING_LAMINATSIYA_INPUT_APPARATUS: &str = "Bosma aparat";
const TRAINING_REZKA_INPUT_APPARATUS: &str = "Laminatsiya aparat";
const TRAINING_INPUT_QR_PREFIX: &str = "TRAINING-INPUT:";

fn canonical_training_apparatus(value: &str) -> Result<String, TrainingWorkspaceError> {
    ApparatusId::new(value.trim().to_string())
        .map(|id| id.to_string())
        .map_err(|_| {
            TrainingWorkspaceError::InvalidInput("canonical apparatus id kerak".to_string())
        })
}

fn training_input_order_id_from_qr(qr_payload: &str) -> Option<String> {
    let (prefix, order_id) = qr_payload.trim().split_once(':')?;
    if !prefix.eq_ignore_ascii_case(TRAINING_INPUT_QR_PREFIX.trim_end_matches(':')) {
        return None;
    }
    let order_id = order_id.trim().to_ascii_lowercase();
    order_id.starts_with("training-").then_some(order_id)
}

const TRAINING_LAMINATSIYA_ROLE: &str = "laminatsiya";
const TRAINING_REZKA_ROLE: &str = "rezka";

fn canonical_apparatus_matches(left: &str, right: &str) -> bool {
    let (Ok(left), Ok(right)) = (
        ApparatusId::new(left.trim()),
        ApparatusId::new(right.trim()),
    ) else {
        return false;
    };
    left == right
}

fn training_apparatus_role<'a>(
    map: &'a ProductionMapDefinition,
    apparatus_id: &str,
) -> Option<&'a str> {
    map.nodes.iter().find_map(|node| {
        (node.kind == ProductionMapNodeKind::Apparatus
            && canonical_apparatus_matches(&node.apparatus_id, apparatus_id))
        .then_some(node.role_code.trim())
    })
}

fn is_laminatsiya_apparatus(map: &ProductionMapDefinition, apparatus_id: &str) -> bool {
    training_apparatus_role(map, apparatus_id)
        .is_some_and(|role| role.eq_ignore_ascii_case(TRAINING_LAMINATSIYA_ROLE))
}

fn is_rezka_apparatus(map: &ProductionMapDefinition, apparatus_id: &str) -> bool {
    training_apparatus_role(map, apparatus_id)
        .is_some_and(|role| role.eq_ignore_ascii_case(TRAINING_REZKA_ROLE))
}

fn is_training_input_node(node: &ProductionMapNode) -> bool {
    node.kind == ProductionMapNodeKind::Apparatus
        && node
            .role_code
            .trim()
            .eq_ignore_ascii_case(TRAINING_INPUT_NODE_ROLE)
}

fn virtual_training_input_id_for_role(role: &str) -> Option<&'static str> {
    if role.eq_ignore_ascii_case(TRAINING_LAMINATSIYA_ROLE) {
        Some(TRAINING_VIRTUAL_INPUT_BOSMA)
    } else if role.eq_ignore_ascii_case(TRAINING_REZKA_ROLE) {
        Some(TRAINING_VIRTUAL_INPUT_LAMINATSIYA)
    } else {
        None
    }
}

fn virtual_training_input_display(input_id: &str) -> Option<&'static str> {
    match input_id {
        TRAINING_VIRTUAL_INPUT_BOSMA => Some(TRAINING_LAMINATSIYA_INPUT_APPARATUS),
        TRAINING_VIRTUAL_INPUT_LAMINATSIYA => Some(TRAINING_REZKA_INPUT_APPARATUS),
        _ => None,
    }
}

fn training_input_stage_for_map(map: &ProductionMapDefinition, apparatus: &str) -> Option<String> {
    let target_node_id = map.nodes.iter().find_map(|node| {
        (node.kind == ProductionMapNodeKind::Apparatus
            && canonical_apparatus_matches(&node.apparatus_id, apparatus))
        .then_some(node.id.as_str())
    });
    if let Some(target_node_id) = target_node_id
        && let Some(input) = map.nodes.iter().find(|node| {
            is_training_input_node(node)
                && !node.item_code.trim().is_empty()
                && map
                    .edges
                    .iter()
                    .any(|edge| edge.from == node.id && edge.to == target_node_id)
        })
    {
        return Some(input.item_code.trim().to_string());
    }
    if chain::previous_work_stage_station(map, apparatus).is_some() {
        return None;
    }
    training_apparatus_role(map, apparatus)
        .and_then(virtual_training_input_id_for_role)
        .map(str::to_string)
}

fn training_input_target_apparatus(map: &ProductionMapDefinition) -> Option<String> {
    map.nodes
        .iter()
        .find(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !is_training_input_node(node)
                && !node.apparatus_id.trim().is_empty()
                && virtual_training_input_id_for_role(&node.role_code).is_some()
                && training_input_stage_for_map(map, &node.apparatus_id).is_some()
        })
        .map(|node| node.apparatus_id.trim().to_string())
        .filter(|id| !id.is_empty())
}

fn training_worker_map(mut map: ProductionMapDefinition) -> ProductionMapDefinition {
    let Some(target_id) = training_input_target_apparatus(&map) else {
        return map;
    };
    let Some(target_index) = map.nodes.iter().position(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && !is_training_input_node(node)
            && canonical_apparatus_matches(&node.apparatus_id, &target_id)
    }) else {
        return map;
    };
    let target = map.nodes[target_index].clone();
    let Some(input_apparatus) = virtual_training_input_id_for_role(&target.role_code) else {
        return map;
    };
    if chain::previous_work_stage_station(&map, &target_id).is_some()
        || map.nodes.iter().any(is_training_input_node)
    {
        return map;
    }

    let mut input_id = "training-input-apparatus".to_string();
    let mut suffix = 2;
    while map.nodes.iter().any(|node| node.id == input_id) {
        input_id = format!("training-input-apparatus-{suffix}");
        suffix += 1;
    }
    let input_node = ProductionMapNode {
        id: input_id.clone(),
        kind: ProductionMapNodeKind::Apparatus,
        title: virtual_training_input_display(input_apparatus)
            .unwrap_or(input_apparatus)
            .to_string(),
        apparatus_id: String::new(),
        formula: None,
        role_code: TRAINING_INPUT_NODE_ROLE.to_string(),
        item_code: input_apparatus.to_string(),
        qty_formula: String::new(),
        from_location: String::new(),
        to_location: String::new(),
        alternative_group_id: String::new(),
        alternative_group_label: String::new(),
        alternative_assigned_title: String::new(),
        alternative_assigned_apparatus_id: String::new(),
        rezka_kadr_count: None,
        rezka_label_length: None,
        x: target.x,
        y: target.y - 132.0,
    };
    let mut edges = Vec::with_capacity(map.edges.len() + 1);
    let mut had_incoming_edge = false;
    for edge in &map.edges {
        if edge.to == target.id {
            had_incoming_edge = true;
            edges.push(ProductionMapEdge {
                from: edge.from.clone(),
                to: input_id.clone(),
                branch: edge.branch.clone(),
            });
        } else {
            edges.push(edge.clone());
        }
    }
    if had_incoming_edge {
        edges.push(ProductionMapEdge {
            from: input_id,
            to: target.id,
            branch: String::new(),
        });
        map.nodes.insert(target_index, input_node);
        map.edges = edges;
    }
    map
}

fn training_input_progress_batch(
    map: &ProductionMapDefinition,
    apparatus: &str,
    identity: &TrainingInputBatchIdentity,
) -> Option<OrderProgressBatch> {
    let order_id = map.id.trim();
    let previous_stage = training_input_stage_for_map(map, apparatus)?;
    if order_id.is_empty()
        || !identity.order_id.eq_ignore_ascii_case(order_id)
        || !canonical_apparatus_matches(&identity.apparatus, apparatus)
    {
        return None;
    }
    let item_code = if map.product_code.trim().is_empty() {
        if map.order_number.trim().is_empty() {
            order_id.to_string()
        } else {
            map.order_number.trim().to_string()
        }
    } else {
        map.product_code.trim().to_string()
    };
    let title = if map.title.trim().is_empty() {
        item_code.clone()
    } else {
        map.title.trim().to_string()
    };
    let produced_qty = map
        .order_kg
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(1.0);
    let mut batch = OrderProgressBatch {
        batch_id: identity.batch_id.clone(),
        revision: 1,
        session_id: identity.session_id.clone(),
        started_at_unix: 0,
        completed_at_unix: 0,
        apparatus: apparatus.trim().to_string(),
        order_id: order_id.to_string(),
        action: queue_state::ApparatusQueueAction::Complete,
        status: OrderProgressBatchStatus::Completed,
        produced_qty,
        uom: "kg".to_string(),
        qr_payload: identity.qr_payload.clone(),
        label_item_code: item_code,
        label_item_name: format!("{title}, apparat: {previous_stage}, training input"),
        executor_name: format!("Training {previous_stage}"),
        worker_role: "training".to_string(),
        worker_ref: "training-input".to_string(),
        worker_display_name: format!("Training {previous_stage}"),
        wip_status: OrderProgressBatchWipStatus::Waiting,
        status_detail: OrderProgressBatchStatusDetail::default(),
        current_apparatus: apparatus.trim().to_string(),
        current_apparatus_key: queue_state::apparatus_search_key(&previous_stage),
        current_location: format!("{previous_stage} chiqim"),
        next_apparatus: apparatus.trim().to_string(),
        parent_batch_id: String::new(),
        used_by_session_id: String::new(),
        used_by_apparatus: String::new(),
        processed_by_session_id: String::new(),
        processed_by_apparatus: String::new(),
        return_ink_kg: None,
        lamination_print_leftover_rolls: None,
        lamination_film_leftover_rolls: None,
        rezka_bosma_waste: None,
        rezka_lamination_waste: None,
        rezka_edge_waste: None,
        total_waste: None,
        finished_goods_kg: Some(produced_qty),
        bobina_kg: None,
        finished_goods_meter: None,
        diameter: None,
        description: format!("Training uchun generatsiya qilingan {previous_stage} input batch"),
        payload_json: serde_json::json!({
            "training": true,
            "training_input": true,
            "source": "generated_training_order_batch",
            "source_apparatus": previous_stage,
            "training_virtual_apparatus": previous_stage,
        }),
    };
    batch.refresh_status_detail();
    Some(batch)
}

fn training_input_batch_matches(
    batch: &OrderProgressBatch,
    order_id: &str,
    previous_stage: &str,
    apparatus: &str,
) -> bool {
    batch.order_id.trim().eq_ignore_ascii_case(order_id.trim())
        && batch
            .payload_json
            .get("training_virtual_apparatus")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case(previous_stage))
        && (batch.next_apparatus.trim().is_empty()
            || canonical_apparatus_matches(&batch.next_apparatus, apparatus))
}

fn training_input_batch_is_available(
    batch: &OrderProgressBatch,
    order_id: &str,
    previous_stage: &str,
    apparatus: &str,
) -> bool {
    training_input_batch_matches(batch, order_id, previous_stage, apparatus)
        && (batch.wip_status == OrderProgressBatchWipStatus::Waiting
            || (batch.wip_status == OrderProgressBatchWipStatus::InUse
                && canonical_apparatus_matches(&batch.used_by_apparatus, apparatus)))
}

fn training_claim_input_batch(
    batch: &OrderProgressBatch,
    apparatus: &str,
    order_id: &str,
) -> OrderProgressBatch {
    let mut claimed = batch.clone();
    claimed.wip_status = OrderProgressBatchWipStatus::InUse;
    claimed.current_apparatus = apparatus.trim().to_string();
    claimed.current_apparatus_key = queue_state::apparatus_search_key(apparatus);
    claimed.current_location = apparatus.trim().to_string();
    claimed.used_by_session_id = format!(
        "training-input-use:{}:{}:{}",
        apparatus.trim(),
        order_id.trim(),
        claimed.batch_id.trim()
    );
    claimed.used_by_apparatus = apparatus.trim().to_string();
    claimed.refresh_status_detail();
    claimed
}
