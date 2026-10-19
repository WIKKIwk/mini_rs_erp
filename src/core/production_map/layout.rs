use super::{OrderProgressBatch, ProductionMapDefinition, ProductionMapError};
use crate::core::order_layout::OrderLane;

pub(super) fn payload_lanes(payload: &serde_json::Value) -> Vec<OrderLane> {
    payload
        .get("layout_lanes")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

pub(super) fn cut_counts(
    map: &ProductionMapDefinition,
    stage_node_id: &str,
    input: &[OrderLane],
) -> Result<Vec<usize>, ProductionMapError> {
    let all = map.layout_lanes();
    let lanes = if input.is_empty() { &all } else { input };
    let first = lanes
        .first()
        .ok_or(ProductionMapError::InvalidRezkaFrameGroups)?;
    let start = all
        .iter()
        .position(|lane| lane == first)
        .ok_or(ProductionMapError::InvalidRezkaFrameGroups)?;
    let end = start + lanes.len();
    if all.get(start..end) != Some(lanes) {
        return Err(ProductionMapError::InvalidRezkaFrameGroups);
    }
    let node = map
        .nodes
        .iter()
        .find(|node| node.id == stage_node_id)
        .ok_or(ProductionMapError::InvalidRezkaFrameGroups)?;
    if super::chain::is_final_work_stage_node(map, stage_node_id)
        || node.rezka_frame_groups.is_empty()
    {
        return Ok(vec![1; lanes.len()]);
    }
    let mut cursor = 0_usize;
    let mut counts = Vec::new();
    for group in &node.rezka_frame_groups {
        let count = usize::try_from(*group)
            .ok()
            .filter(|n| *n > 0)
            .ok_or(ProductionMapError::InvalidRezkaFrameGroups)?;
        let boundary = cursor
            .checked_add(count)
            .ok_or(ProductionMapError::InvalidRezkaFrameGroups)?;
        let intersect_start = cursor.max(start);
        let intersect_end = boundary.min(end);
        if intersect_end > intersect_start {
            counts.push(intersect_end - intersect_start);
        }
        cursor = boundary;
    }
    if cursor != all.len() || counts.iter().sum::<usize>() != lanes.len() {
        return Err(ProductionMapError::InvalidRezkaFrameGroups);
    }
    Ok(counts)
}

pub(super) fn apply_output_layout(
    batch: &mut OrderProgressBatch,
    map: &ProductionMapDefinition,
    input_lanes: &[OrderLane],
    has_input: bool,
    lane_offset: Option<usize>,
    lane_count: Option<usize>,
) -> Result<(), ProductionMapError> {
    if map.products.is_empty() {
        return Ok(());
    }
    let original = map.layout_lanes();
    let input = if input_lanes.is_empty() {
        if has_input {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        &original
    } else {
        input_lanes
    };
    if input.is_empty() || input.iter().any(|lane| !original.contains(lane)) {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    let lanes = if let Some(offset) = lane_offset {
        let end = offset
            .checked_add(lane_count.ok_or(ProductionMapError::ProgressInputInvalid)?)
            .ok_or(ProductionMapError::ProgressInputInvalid)?;
        input
            .get(offset..end)
            .ok_or(ProductionMapError::ProgressInputInvalid)?
    } else {
        input
    };
    if lanes.is_empty() {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    batch.payload_json["layout_lanes"] = serde_json::json!(lanes);
    batch.payload_json["contained_kadr_count"] = serde_json::json!(lanes.len());
    batch.payload_json["layout_width_mm"] =
        serde_json::json!(lanes.iter().map(|l| l.width_mm).sum::<f64>());
    if lanes
        .iter()
        .all(|lane| lane.product_id == lanes[0].product_id)
    {
        let product = &lanes[0];
        batch.label_item_code = product.item_code.clone();
        batch.label_item_name = product.product.clone();
        batch.payload_json["product_id"] = serde_json::json!(product.product_id);
        batch.payload_json["product_order_number"] = serde_json::json!(product.order_number);
        batch.payload_json["product_code"] = serde_json::json!(product.item_code);
        batch.payload_json["product_name"] = serde_json::json!(product.product);
    }
    Ok(())
}

pub(super) fn finished_product(
    map: &ProductionMapDefinition,
    batch: &OrderProgressBatch,
) -> Result<(String, String), ProductionMapError> {
    if map.products.is_empty() {
        return Ok((map.product_code.clone(), map.title.clone()));
    }
    let lanes = payload_lanes(&batch.payload_json);
    if lanes.len() != 1 {
        return Err(ProductionMapError::InvalidRezkaFrameGroups);
    }
    let first = lanes
        .first()
        .ok_or(ProductionMapError::ProgressInputInvalid)?;
    let expected = map.layout_lanes();
    if lanes
        .iter()
        .any(|lane| lane.product_id != first.product_id || !expected.contains(lane))
    {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    Ok((first.item_code.clone(), first.product.clone()))
}

pub(super) fn output_layout(
    map: &ProductionMapDefinition,
    session: &super::OrderRunSession,
    counts: &[i64],
) -> serde_json::Value {
    if map.products.is_empty() {
        return serde_json::Value::Null;
    }
    let stored = payload_lanes(&session.payload_json);
    let lanes = if stored.is_empty() {
        map.layout_lanes()
    } else {
        stored
    };
    let mut offset = 0_usize;
    let mut outputs = Vec::new();
    for count in counts {
        let Ok(count) = usize::try_from(*count) else {
            return serde_json::Value::Null;
        };
        let Some(group) = lanes.get(offset..offset.saturating_add(count)) else {
            return serde_json::Value::Null;
        };
        if group.is_empty() {
            return serde_json::Value::Null;
        }
        offset += count;
        let mut names = Vec::new();
        for lane in group {
            let label = format!("{} · №{}", lane.product, lane.order_number);
            if !names.contains(&label) {
                names.push(label);
            }
        }
        outputs.push(serde_json::json!({
            "name": names.join(" + "),
            "width_mm": group.iter().map(|lane| lane.width_mm).sum::<f64>(),
            "lanes": group,
        }));
    }
    if offset != lanes.len() {
        return serde_json::Value::Null;
    }
    serde_json::json!(outputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map() -> ProductionMapDefinition {
        serde_json::from_value(json!({
            "id":"zakaz-1001", "order_number":"1001", "product_code":"SKU-0", "title":"Mixed",
            "products":crate::core::order_layout::tests::products(), "width_mm":575,
            "nodes":[
                {"id":"start","kind":"start","title":"Start"},
                {"id":"cut1","kind":"apparatus","title":"Cut","apparatus_id":"apparatus:test:cut1","rezka_kadr_count":6,"rezka_frame_groups":[2,2,2]},
                {"id":"lam","kind":"apparatus","title":"Lam","apparatus_id":"apparatus:test:lam"},
                {"id":"cut2","kind":"apparatus","title":"Cut","apparatus_id":"apparatus:test:cut2","rezka_kadr_count":6},
                {"id":"end","kind":"end","title":"End"}
            ],
            "edges":[{"from":"start","to":"cut1"},{"from":"cut1","to":"lam"},{"from":"lam","to":"cut2"},{"from":"cut2","to":"end"}]
        })).unwrap()
    }

    fn batch() -> OrderProgressBatch {
        serde_json::from_value(json!({
            "batch_id":"roll", "session_id":"session", "started_at_unix":0, "completed_at_unix":0,
            "apparatus":"apparatus:test:cut2", "order_id":"zakaz-1001", "action":"roll_complete",
            "status":"completed", "produced_qty":100.0, "uom":"m", "qr_payload":"QR",
            "label_item_code":"", "label_item_name":"", "executor_name":"", "worker_role":"aparatchi",
            "worker_ref":"worker", "worker_display_name":"Worker", "wip_status":"waiting", "payload_json":{}
        })).unwrap()
    }

    #[test]
    fn multi_product_cut_keeps_sku_after_pre_cut_and_lamination() {
        let map = map();
        let mut grouped = batch();
        apply_output_layout(&mut grouped, &map, &[], false, Some(2), Some(2)).unwrap();
        let lanes = payload_lanes(&grouped.payload_json);
        assert_eq!(
            lanes.iter().map(|l| l.width_mm).collect::<Vec<_>>(),
            vec![80.0, 80.0]
        );
        assert_eq!(cut_counts(&map, "cut2", &lanes).unwrap(), vec![1, 1]);
        let mut laminated = batch();
        apply_output_layout(&mut laminated, &map, &lanes, true, None, Some(2)).unwrap();
        let mut final_roll = batch();
        apply_output_layout(
            &mut final_roll,
            &map,
            &payload_lanes(&laminated.payload_json),
            true,
            Some(1),
            Some(1),
        )
        .unwrap();
        assert_eq!(final_roll.label_item_code, "SKU-2");
        assert_eq!(
            final_roll.payload_json["product_id"],
            "zakaz-1001:product:p2"
        );
        assert_eq!(
            finished_product(&map, &final_roll).unwrap(),
            ("SKU-2".into(), "Collagen".into())
        );
        assert!(finished_product(&map, &grouped).is_err());
    }

    #[test]
    fn multi_product_partial_input_uses_original_lane_positions() {
        let map = map();
        let lanes = map.layout_lanes();
        assert_eq!(cut_counts(&map, "cut1", &lanes[4..]).unwrap(), vec![2]);
        assert_eq!(cut_counts(&map, "cut2", &lanes[4..]).unwrap(), vec![1, 1]);
        assert!(cut_counts(&map, "cut2", &[lanes[0].clone(), lanes[3].clone()]).is_err());
        let mut roll = batch();
        assert!(apply_output_layout(&mut roll, &map, &[], true, Some(0), Some(1)).is_err());
    }
}
