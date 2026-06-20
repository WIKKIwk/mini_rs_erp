use super::materials::coefficient_cell;
use super::request_layers::{
    hydrate_layers_from_material_display, request_variants, visible_layers,
};
use super::{
    CalcResult, CalculateRequest, CalculateResponse, DEFAULT_EDGE_ALLOWANCE_MM, MIN_MOLD_EXTRA_MM,
};

pub fn calculate(mut request: CalculateRequest) -> Result<CalculateResponse, String> {
    hydrate_layers_from_material_display(&mut request);
    let kg = require_number(request.kg, "KG")?;
    let frame_product_size_mm =
        require_number(request.frame_product_size_mm, "Kadrdagi mahsulot o'lchami")?;
    let frame_count = require_number(request.frame_count, "Kadr soni")?;
    let edge_allowance_mm = request
        .edge_allowance_mm
        .unwrap_or(DEFAULT_EDGE_ALLOWANCE_MM);
    let width_mm = derive_width_mm(
        Some(frame_product_size_mm),
        Some(frame_count),
        Some(edge_allowance_mm),
    )?;
    if kg <= 0.0 {
        return Err("KG noto'g'ri".to_string());
    }
    let waste_percent = request.waste_percent.unwrap_or(5.0);
    if waste_percent < 0.0 {
        return Err("Atxod foiz noto'g'ri".to_string());
    }
    let results = calculate_variants(&request)?;
    let layers = visible_layers(&request);

    Ok(CalculateResponse {
        ok: true,
        order_number: clean_option(request.order_number),
        date: clean_option(request.date),
        customer: clean_option(request.customer),
        product: clean_option(request.product),
        status: clean_option(request.status),
        material_display: clean_option(request.material_display),
        color: clean_option(request.color),
        kg,
        frame_product_size_mm,
        frame_count,
        edge_allowance_mm,
        width_mm,
        min_mold_size_mm: min_mold_size_mm(frame_product_size_mm, frame_count),
        rubber_size_mm: rubber_size(width_mm),
        waste_percent,
        roll_count: request.roll_count,
        layers,
        results,
        note: clean_option(request.note),
    })
}

fn calculate_variants(request: &CalculateRequest) -> Result<Vec<CalcResult>, String> {
    let mut results = Vec::new();
    for variant in request_variants(request) {
        results.push(calculate_single(&variant)?);
    }
    if results.is_empty() {
        return Err("hisob varianti topilmadi".to_string());
    }
    Ok(results)
}

fn calculate_single(request: &CalculateRequest) -> Result<CalcResult, String> {
    let kg = require_number(request.kg, "KG")?;
    let width_mm = width_mm_from_request(request)?;
    let q1 = require_text(&request.first_layer.material, "1-qavat")?;
    let m1 = require_text(&request.first_layer.micron, "1-mikron")?;
    let q2 = request.second_layer.material.clone();
    let m2 = if request.second_layer.micron.trim().is_empty() {
        "--".to_string()
    } else {
        request.second_layer.micron.clone()
    };
    let q3 = request.third_layer.material.clone();
    let m3 = request.third_layer.micron.clone();
    let (q_other, m_other) = merge_layers(q2, m2, q3, m3)?;
    let first_empty = is_empty_material(&q1);
    let first_micron = if first_empty { 0 } else { parse_micron(&m1)? };
    let other_micron = if is_empty_material(&q_other) {
        0
    } else {
        parse_micron(&m_other)?
    };

    let first_coeff = if first_empty {
        0.0
    } else {
        coefficient_cell(&q1, &m1, first_micron, true)?
    };
    let other_coeff = if is_empty_material(&q_other) {
        0.0
    } else {
        coefficient_cell(&q_other, &m_other, other_micron, false)?
    };
    let coeff_sum = first_coeff + other_coeff;
    if coeff_sum <= 0.0 {
        return Err("kamida bitta qavat materiali kerak".to_string());
    }

    let width_sm = width_mm / 10.0;
    let waste_percent = request.waste_percent.unwrap_or(5.0);
    if waste_percent < 0.0 {
        return Err("Atxod foiz noto'g'ri".to_string());
    }
    let base_length = kg / (coeff_sum * width_sm) * 6000.0;
    let waste_length = base_length * waste_percent / 100.0;
    let rounded_length = round_up(base_length + waste_length, 500.0);

    Ok(CalcResult {
        first_coeff,
        other_coeff,
        coeff_sum,
        width_sm,
        base_length,
        waste_length,
        rounded_length,
    })
}

fn merge_layers(
    q2: String,
    m2: String,
    q3: String,
    m3: String,
) -> Result<(String, String), String> {
    let q2_empty = is_empty_material(&q2);
    let q3_empty = is_empty_material(&q3);
    match (q2_empty, q3_empty) {
        (true, true) => Ok(("--".to_string(), "--".to_string())),
        (true, false) => Ok((q3, m3)),
        (false, true) => Ok((q2, m2)),
        (false, false) => {
            if m3.trim().is_empty() {
                return Err("3-qavat mikroni berilmagan".to_string());
            }
            Ok((format!("{q2}/{q3}"), format!("{m2}/{m3}")))
        }
    }
}

fn parse_micron(value: &str) -> Result<u32, String> {
    parse_micron_parts(value)?
        .into_iter()
        .max()
        .ok_or_else(|| format!("micron noto'g'ri: {value}"))
}

pub(super) fn parse_micron_parts(value: &str) -> Result<Vec<u32>, String> {
    let value = value.trim();
    if value.is_empty() || value == "--" {
        return Err(format!("micron noto'g'ri: {value}"));
    }
    value
        .split('/')
        .map(|part| {
            part.trim()
                .parse::<u32>()
                .map_err(|_| format!("micron noto'g'ri: {value}"))
        })
        .collect()
}

fn require_text(value: &str, name: &str) -> Result<String, String> {
    value
        .trim()
        .is_empty()
        .then(|| format!("{name} to'ldirilmagan"))
        .map_or_else(|| Ok(value.trim().to_string()), Err)
}

fn require_number(value: Option<f64>, name: &str) -> Result<f64, String> {
    value.ok_or_else(|| format!("{name} to'ldirilmagan"))
}

pub fn derive_width_mm(
    frame_product_size_mm: Option<f64>,
    frame_count: Option<f64>,
    edge_allowance_mm: Option<f64>,
) -> Result<f64, String> {
    let frame_product_size_mm =
        require_number(frame_product_size_mm, "Kadrdagi mahsulot o'lchami")?;
    let frame_count = require_number(frame_count, "Kadr soni")?;
    let edge_allowance_mm = edge_allowance_mm.unwrap_or(DEFAULT_EDGE_ALLOWANCE_MM);
    if !frame_product_size_mm.is_finite() || frame_product_size_mm <= 0.0 {
        return Err("Kadrdagi mahsulot o'lchami noto'g'ri".to_string());
    }
    if !frame_count.is_finite() || frame_count <= 0.0 {
        return Err("Kadr soni noto'g'ri".to_string());
    }
    if !edge_allowance_mm.is_finite() || edge_allowance_mm < 0.0 {
        return Err("Qo'shimcha razmer noto'g'ri".to_string());
    }
    Ok(frame_product_size_mm * frame_count + edge_allowance_mm)
}

fn width_mm_from_request(request: &CalculateRequest) -> Result<f64, String> {
    derive_width_mm(
        request.frame_product_size_mm,
        request.frame_count,
        request.edge_allowance_mm,
    )
}

fn min_mold_size_mm(frame_product_size_mm: f64, frame_count: f64) -> f64 {
    frame_product_size_mm * frame_count + MIN_MOLD_EXTRA_MM
}

fn is_empty_material(material: &str) -> bool {
    let n = normalize(material);
    n.is_empty() || n.chars().all(|ch| ch == '-') || matches!(n.as_str(), "yoq" | "yuq")
}

pub(super) fn split_parts(value: &str) -> Vec<&str> {
    value
        .split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

pub(super) fn normalize(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .collect()
}

pub(super) fn close(value: &str, expected: &str) -> bool {
    value == expected || (value.len() == expected.len() && levenshtein(value, expected) <= 1)
}

fn levenshtein(left: &str, right: &str) -> usize {
    let mut costs: Vec<usize> = (0..=right.len()).collect();
    for (i, lc) in left.chars().enumerate() {
        let mut previous = i;
        costs[0] = i + 1;
        for (j, rc) in right.chars().enumerate() {
            let current = costs[j + 1];
            costs[j + 1] = if lc == rc {
                previous
            } else {
                1 + previous.min(current).min(costs[j])
            };
            previous = current;
        }
    }
    costs[right.len()]
}

fn clean_option(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn rubber_size(width_mm: f64) -> u32 {
    ((width_mm / 50.0).ceil() as u32 * 50).clamp(50, 1300)
}

fn round_up(value: f64, step: f64) -> f64 {
    (value / step).ceil() * step
}
