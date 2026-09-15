//! Deterministic Telegram routing using canonical, active apparatus configurations.
use serde::{Deserialize, Serialize};

use super::{ProductionMapDefinition, ProductionMapEdge, ProductionMapNode, pechat};
use crate::core::apparatus_standard::{
    EquipmentCapabilityCode as Capability, ExecutionOperation as Operation,
    ProcessTechnology as Technology, RuntimeApparatusConfiguration as Apparatus,
};
use crate::core::calculate_materials::CalculateMaterial;
use crate::core::calculate_orders::{CalculateOrderTemplate, validate_template};
use crate::core::formula::{CalculateRequest, calculate_with_material_catalog};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintMethod {
    Flexo,
    Metal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderProductionOptions {
    pub print_method: PrintMethod,
    pub cold_glue: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter_mm: Option<f64>,
}

pub fn generate(
    template: &CalculateOrderTemplate,
    apparatus: &[Apparatus],
    materials: &[CalculateMaterial],
) -> Result<ProductionMapDefinition, String> {
    validate_template(template).map_err(|e| e.to_string())?;
    let options = template
        .production_options
        .as_ref()
        .ok_or("Bosma usuli kerak")?;
    let form = template.status.trim().to_lowercase();
    if !matches!(form.as_str(), "rulon" | "paket") {
        return Err("Rulon yoki Paket tanlang".into());
    }
    let frames = template.frame_count;
    if !frames.is_finite() || frames <= 0.0 || frames.fract() != 0.0 || frames > 1024.0 {
        return Err("Avtomatik map uchun kadr soni 1–1024 oralig‘ida bo‘lsin".into());
    }
    let rolls = template
        .roll_count
        .filter(|v| *v > 0)
        .ok_or("Val soni kerak")?;
    let calculation = calculate_with_material_catalog(
        CalculateRequest {
            kg: Some(template.kg),
            frame_product_size_mm: Some(template.frame_product_size_mm),
            frame_count: Some(frames),
            edge_allowance_mm: Some(template.edge_allowance_mm),
            waste_percent: Some(template.waste_percent),
            roll_count: Some(rolls),
            layers: template.effective_layers(),
            ..Default::default()
        },
        materials,
    )?;
    let width = calculation.width_mm;
    let mut catalog: Vec<_> = apparatus
        .iter()
        .filter(|a| a.has_coherent_source() && a.is_active())
        .collect();
    catalog.sort_by(|a, b| a.runtime.apparatus_id.cmp(&b.runtime.apparatus_id));
    let candidates = |operation, capability| {
        catalog
            .iter()
            .copied()
            .filter(move |a| {
                a.runtime.execution_profile.operation == operation && a.supports(capability)
            })
            .collect::<Vec<_>>()
    };
    let print_width = template.print_val_size_mm.unwrap_or(width);
    let profile_width = template
        .print_val_size_mm
        .unwrap_or(template.frame_product_size_mm * frames);
    let printers = candidates(Operation::Print, Capability::Print)
        .into_iter()
        .filter(|a| {
            let p = &a.runtime.execution_profile;
            if !pechat::apparatus_profile_can_handle_order(a, Some(rolls), Some(profile_width)) {
                return false;
            }
            match options.print_method {
                PrintMethod::Flexo => {
                    p.technology == Technology::Flexographic
                        && p.color_station_count
                            .is_some_and(|count| rolls <= i64::from(count))
                }
                PrintMethod::Metal => {
                    p.technology == Technology::Rotogravure
                        && print_width <= 1350.0
                        && p.color_station_count.is_some_and(|count| {
                            matches!(count, 7..=9)
                                && pechat::pechat_can_handle_order(
                                    count as u8,
                                    Some(rolls),
                                    Some(print_width),
                                )
                        })
                }
            }
        })
        .collect::<Vec<_>>();
    let mut graph = Graph::new(template);
    graph.stage("print", printers, None, &[])?;
    let mut widths = vec![width];
    if template.effective_layers().len() > 1 {
        let laminators = candidates(Operation::Laminate, Capability::Laminate);
        let groups = best_frame_groups(width, frames as i64, &laminators)
            .ok_or("Kadrni bo‘lmasdan laminatsiyaga joylab bo‘lmaydi: mos apparat topilmadi")?;
        if groups.len() > 1 {
            let cutters = candidates(Operation::Cut, Capability::Cut)
                .into_iter()
                .filter(|a| width_fits(a, width))
                .collect();
            graph.stage("pre_lamination_cut", cutters, Some(frames as i64), &groups)?;
            widths = groups.iter().map(|n| width * *n as f64 / frames).collect();
        }
        let compatible = laminators
            .into_iter()
            .filter(|a| widths.iter().all(|w| lamination_fits(a, *w)))
            .collect();
        graph.stage("laminate", compatible, None, &[])?;
    }
    if options.cold_glue {
        let glue = candidates(Operation::Glue, Capability::Glue)
            .into_iter()
            .filter(|a| {
                a.runtime.execution_profile.technology == Technology::ColdGlue
                    && widths.iter().all(|w| width_fits(a, *w))
            })
            .collect();
        graph.stage("cold_glue", glue, None, &[])?;
    }
    if form == "paket" {
        let packers = candidates(Operation::Package, Capability::Package)
            .into_iter()
            .filter(|a| widths.iter().all(|w| width_fits(a, *w)))
            .collect();
        graph.stage("package", packers, None, &[])?;
    }
    let cutters = candidates(Operation::Cut, Capability::Cut)
        .into_iter()
        .filter(|a| widths.iter().all(|w| width_fits(a, *w)))
        .collect();
    graph.stage("final_cut", cutters, Some(frames as i64), &[])?;
    let mut map = graph.finish();
    map.width_mm = Some(width);
    map.print_val_size_mm = template.print_val_size_mm;
    map.order_kg = Some(calculation.kg);
    map.base_length = Some(
        calculation
            .results
            .first()
            .ok_or("Hisob natijasi topilmadi")?
            .rounded_length,
    );
    map.roll_count = Some(rolls);
    super::compile_map(&map).map_err(|e| e.to_string())?;
    Ok(map)
}

fn width_fits(a: &Apparatus, width: f64) -> bool {
    let p = &a.runtime.execution_profile;
    width.is_finite()
        && width > 0.0
        && p.min_web_width_mm.is_none_or(|min| width >= f64::from(min))
        && p.max_web_width_mm.is_none_or(|max| width <= f64::from(max))
}

pub(crate) fn lamination_fits(a: &Apparatus, width: f64) -> bool {
    let p = &a.runtime.execution_profile;
    let limit = p.max_web_width_mm.unwrap_or(match p.technology {
        Technology::AdhesiveLamination => 1050,
        Technology::ExtrusionLamination => 1300,
        _ => return false,
    });
    width_fits(a, width) && (width / 50.0).ceil() * 50.0 <= f64::from(limit)
}

/// Widths at this occurrence, stopping at the nearest upstream physical cut.
/// Every incoming route is checked; a cut on an unrelated branch cannot help.
pub(crate) fn stage_input_widths(
    map: &ProductionMapDefinition,
    node_id: &str,
    cut_ids: &std::collections::BTreeSet<crate::core::apparatus_standard::ApparatusId>,
) -> Vec<f64> {
    let Some(width) = map.width_mm else {
        return Vec::new();
    };
    let mut todo: Vec<_> = map
        .edges
        .iter()
        .filter(|e| e.to == node_id)
        .map(|e| e.from.as_str())
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut widths = Vec::new();
    while let Some(id) = todo.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = map.nodes.iter().find(|n| n.id == id) else {
            widths.push(width);
            continue;
        };
        if node
            .canonical_apparatus_id()
            .is_some_and(|id| cut_ids.contains(&id))
            && let Some(frames) = node.rezka_kadr_count.filter(|n| *n > 0)
        {
            if node.rezka_frame_groups.is_empty() {
                widths.push(width / frames as f64);
            } else {
                widths.extend(
                    node.rezka_frame_groups
                        .iter()
                        .map(|n| width * *n as f64 / frames as f64),
                );
            }
            continue;
        }
        let previous: Vec<_> = map
            .edges
            .iter()
            .filter(|e| e.to == id)
            .map(|e| e.from.as_str())
            .collect();
        if previous.is_empty() {
            widths.push(width);
        }
        todo.extend(previous);
    }
    if widths.is_empty() {
        widths.push(width);
    }
    widths
}

// Minimize the number of physical strips while preserving whole frames.
// Allow a minimum apparatus width too: e.g. [3,1] may need to become [2,2].
fn best_frame_groups(width: f64, frames: i64, laminators: &[&Apparatus]) -> Option<Vec<i64>> {
    let mut best: Option<Vec<i64>> = None;
    for a in laminators {
        let fits = |count| lamination_fits(a, width * count as f64 / frames as f64);
        let Some(min) = (1..=frames).find(|n| fits(*n)) else {
            continue;
        };
        let max = (min..=frames).rev().find(|n| fits(*n)).unwrap();
        let parts = (frames + max - 1) / max;
        if parts * min > frames {
            continue;
        }
        let mut left = frames;
        let groups: Vec<_> = (0..parts)
            .map(|i| {
                let count = max.min(left - (parts - i - 1) * min);
                left -= count;
                count
            })
            .collect();
        if best
            .as_ref()
            .is_none_or(|b| groups.len() < b.len() || (groups.len() == b.len() && groups > *b))
        {
            best = Some(groups);
        }
    }
    best
}

struct Graph {
    map: ProductionMapDefinition,
    previous: Vec<String>,
    row: usize,
}
impl Graph {
    fn new(t: &CalculateOrderTemplate) -> Self {
        let map = serde_json::from_value(serde_json::json!({
            "id": format!("zakaz-{}", t.order_number), "code": t.order_number,
            "order_number": t.order_number, "product_code": t.item_code,
            "title": t.product, "customer_name": t.customer, "image_id": t.image_id,
            "nodes": [{"id":"start","kind":"start","title":"Start","x":420,"y":32}]
        }))
        .expect("static production map shape");
        Self {
            map,
            previous: vec!["start".into()],
            row: 1,
        }
    }
    fn stage(
        &mut self,
        key: &str,
        candidates: Vec<&Apparatus>,
        frames: Option<i64>,
        groups: &[i64],
    ) -> Result<(), String> {
        let label = match key {
            "print" => "Bosma",
            "pre_lamination_cut" => "Laminatsiyadan oldingi Rezka",
            "laminate" => "Laminatsiya",
            "cold_glue" => "Holodniy kley",
            "package" => "Paket",
            "final_cut" => "Yakuniy Rezka",
            _ => key,
        };
        if candidates.is_empty() {
            return Err(format!("{label}: mos faol apparat topilmadi"));
        }
        let alternative = candidates.len() > 1;
        let mut next = Vec::new();
        for (index, a) in candidates.iter().enumerate() {
            let id = format!("{key}_{index}");
            let node: ProductionMapNode = serde_json::from_value(serde_json::json!({
                "id": id, "kind":"apparatus", "title":a.runtime.display.display_name,
                "apparatus_id":a.runtime.apparatus_id,
                "alternative_group_id": if alternative {format!("auto_{key}")} else {String::new()},
                "alternative_group_label":label,
                "rezka_kadr_count":frames, "rezka_frame_groups":groups,
                "x": 420.0 + (index as f64 - (candidates.len()-1) as f64/2.0)*220.0,
                "y":32 + self.row * 164
            }))
            .expect("static production node shape");
            for from in self.previous.clone() {
                self.edge(from, id.clone());
            }
            next.push(id);
            self.map.nodes.push(node);
        }
        self.previous = next;
        self.row += 1;
        Ok(())
    }
    fn edge(&mut self, from: String, to: String) {
        self.map.edges.push(ProductionMapEdge {
            from,
            to,
            branch: String::new(),
        });
    }
    fn finish(mut self) -> ProductionMapDefinition {
        self.map.nodes.push(
            serde_json::from_value(serde_json::json!({
                "id":"end","kind":"end","title":self.map.title,"item_code":self.map.product_code,
                "x":420,"y":32+self.row*164
            }))
            .expect("static end node shape"),
        );
        for from in self.previous.clone() {
            self.edge(from, "end".into());
        }
        self.map
    }
}

#[cfg(test)]
mod tests;
