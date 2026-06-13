use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CalculateRequest {
    #[serde(default)]
    pub order_number: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub customer: Option<String>,
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub material_display: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub kg: Option<f64>,
    #[serde(default)]
    pub width_mm: Option<f64>,
    #[serde(default)]
    pub waste_percent: Option<f64>,
    #[serde(default)]
    pub roll_count: Option<f64>,
    #[serde(default)]
    pub first_layer: LayerInput,
    #[serde(default)]
    pub second_layer: LayerInput,
    #[serde(default)]
    pub third_layer: LayerInput,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LayerInput {
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub micron: String,
}

impl LayerInput {
    pub fn new(material: impl Into<String>, micron: impl Into<String>) -> Self {
        Self {
            material: material.into(),
            micron: micron.into(),
        }
    }

    fn is_empty(&self) -> bool {
        self.material.trim().is_empty() && self.micron.trim().is_empty()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CalculateResponse {
    pub ok: bool,
    pub order_number: Option<String>,
    pub date: Option<String>,
    pub customer: Option<String>,
    pub product: Option<String>,
    pub status: Option<String>,
    pub material_display: Option<String>,
    pub color: Option<String>,
    pub kg: f64,
    pub width_mm: f64,
    pub rubber_size_mm: u32,
    pub waste_percent: f64,
    pub roll_count: Option<f64>,
    pub layers: Vec<LayerInput>,
    pub results: Vec<CalcResult>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalcResult {
    pub first_coeff: f64,
    pub other_coeff: f64,
    pub coeff_sum: f64,
    pub width_sm: f64,
    pub base_length: f64,
    pub waste_length: f64,
    pub rounded_length: f64,
}

pub fn calculate(mut request: CalculateRequest) -> Result<CalculateResponse, String> {
    hydrate_layers_from_material_display(&mut request);
    let kg = require_number(request.kg, "KG")?;
    let width_mm = require_number(request.width_mm, "RAZMER")?;
    if kg <= 0.0 {
        return Err("KG noto'g'ri".to_string());
    }
    if width_mm <= 0.0 {
        return Err("RAZMER noto'g'ri".to_string());
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
        width_mm,
        rubber_size_mm: rubber_size(width_mm),
        waste_percent,
        roll_count: request.roll_count,
        layers,
        results,
        note: clean_option(request.note),
    })
}

fn hydrate_layers_from_material_display(request: &mut CalculateRequest) {
    if request
        .material_display
        .as_deref()
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        return;
    }
    if !request.first_layer.is_empty()
        || !request.second_layer.is_empty()
        || !request.third_layer.is_empty()
    {
        return;
    }
    let layers = parse_material_layers(request.material_display.as_deref().unwrap_or(""));
    if let Some(layer) = layers.first() {
        request.first_layer = layer.clone();
    }
    if let Some(layer) = layers.get(1) {
        request.second_layer = layer.clone();
    }
    if let Some(layer) = layers.get(2) {
        request.third_layer = layer.clone();
    }
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
    let width_mm = require_number(request.width_mm, "RAZMER")?;
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

fn request_variants(request: &CalculateRequest) -> Vec<CalculateRequest> {
    let first_materials = alternatives(&request.first_layer.material, &request.first_layer.micron);
    let second_materials =
        alternatives(&request.second_layer.material, &request.second_layer.micron);
    let third_materials = alternatives(&request.third_layer.material, &request.third_layer.micron);
    let mut variants = Vec::new();
    for first_material in &first_materials {
        for second_material in &second_materials {
            for third_material in &third_materials {
                let mut variant = request.clone();
                variant.first_layer.material = first_material.clone();
                variant.second_layer.material = second_material.clone();
                variant.third_layer.material = third_material.clone();
                variants.push(variant);
            }
        }
    }
    variants
}

fn alternatives(value: &str, micron_text: &str) -> Vec<String> {
    let parts = value
        .split("yoki")
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .flat_map(|part| slash_alternatives(part, micron_text))
        .map(str::to_string)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        vec![value.to_string()]
    } else {
        parts
    }
}

fn slash_alternatives<'a>(value: &'a str, micron_text: &str) -> Vec<&'a str> {
    let parts = split_parts(value);
    if parts.len() <= 1 || slash_matches_microns(parts.len(), micron_text) {
        return vec![value];
    }
    parts
}

fn slash_matches_microns(material_count: usize, micron_text: &str) -> bool {
    parse_micron_parts(micron_text)
        .ok()
        .is_some_and(|microns| microns.len() == material_count)
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

fn coefficient_cell(
    material: &str,
    micron_text: &str,
    micron: u32,
    is_first: bool,
) -> Result<f64, String> {
    let materials = split_parts(material);
    let microns = parse_micron_parts(micron_text)?;
    if materials.len() == 1 {
        return coefficient_single(materials[0], micron, is_first);
    }
    if materials.len() != microns.len() {
        return Err(format!(
            "material/mikron mos emas: {material} / {micron_text}"
        ));
    }
    materials
        .iter()
        .zip(microns)
        .map(|(material, micron)| coefficient_single(material, micron, is_first))
        .sum()
}

fn coefficient_single(material: &str, micron: u32, is_first: bool) -> Result<f64, String> {
    let family = material_family(material)?;
    if is_first && !matches!(family, Family::Empty | Family::Twist) && micron <= 20 {
        return Ok(1.0);
    }
    if family == Family::First && micron <= 20 {
        return Ok(1.0);
    }

    let value = match family {
        Family::First | Family::McpCpp => mcp_cpp(micron),
        Family::Jem => jem(micron),
        Family::Pe => pe(micron),
        Family::Twist => Some(2.0),
        Family::Empty => None,
    };
    value.ok_or_else(|| coefficient_error(material, micron, family))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    First,
    McpCpp,
    Jem,
    Pe,
    Twist,
    Empty,
}

fn material_family(material: &str) -> Result<Family, String> {
    let n = normalize(material);
    if n.is_empty() || matches!(n.as_str(), "--" | "-" | "yoq" | "yuq") {
        return Ok(Family::Empty);
    }
    if n.contains("twis") || n.contains("tuisim") {
        return Ok(Family::Twist);
    }
    if n.starts_with("pet") || n.starts_with("mpet") || close(&n, "pet") {
        return Ok(Family::First);
    }
    if n.starts_with("opp") || n.starts_with("popp") || n == "st01" || close(&n, "opp") {
        return Ok(Family::First);
    }
    if matches!(n.as_str(), "map" | "mcpp" | "msr" | "msp") {
        return Ok(Family::McpCpp);
    }
    if n.starts_with("mat") || n.starts_with("pff") || n.starts_with("pf") || close(&n, "mat") {
        return Ok(Family::First);
    }
    if n.starts_with("pe") || close(&n, "pe") {
        return Ok(Family::Pe);
    }
    if n.starts_with("cpp") || n.starts_with("mcp") || close(&n, "cpp") || close(&n, "mcp") {
        return Ok(Family::McpCpp);
    }
    if n.starts_with("jem") || close(&n, "jem") {
        return Ok(Family::Jem);
    }
    Err(format!("noma'lum material: {material}"))
}

fn mcp_cpp(micron: u32) -> Option<f64> {
    interpolate(
        micron,
        &[
            (20, 1.07),
            (25, 1.3),
            (30, 1.6),
            (35, 2.0),
            (40, 2.15),
            (45, 2.7),
            (50, 2.8),
            (60, 3.2),
        ],
    )
}

fn jem(micron: u32) -> Option<f64> {
    interpolate(micron, &[(25, 1.0), (30, 1.5)])
}

fn pe(micron: u32) -> Option<f64> {
    interpolate(
        micron,
        &[
            (30, 2.0),
            (35, 2.3),
            (40, 2.6),
            (45, 3.0),
            (50, 3.3),
            (55, 3.6),
            (60, 4.0),
            (65, 4.3),
            (70, 4.6),
            (75, 5.0),
            (80, 5.3),
            (85, 5.6),
            (90, 6.0),
        ],
    )
}

fn interpolate(micron: u32, table: &[(u32, f64)]) -> Option<f64> {
    let [
        (first_micron, first_value),
        (second_micron, second_value),
        ..,
    ] = table
    else {
        return None;
    };
    if micron < *first_micron {
        return Some(project(
            micron,
            *first_micron,
            *first_value,
            *second_micron,
            *second_value,
        ));
    }
    for window in table.windows(2) {
        let (left_micron, left_value) = window[0];
        let (right_micron, right_value) = window[1];
        if micron == left_micron {
            return Some(left_value);
        }
        if micron > left_micron && micron < right_micron {
            let ratio = (micron - left_micron) as f64 / (right_micron - left_micron) as f64;
            return Some(left_value + (right_value - left_value) * ratio);
        }
    }
    let (left_micron, left_value) = table[table.len() - 2];
    let (right_micron, right_value) = table[table.len() - 1];
    Some(project(
        micron,
        left_micron,
        left_value,
        right_micron,
        right_value,
    ))
}

fn project(
    micron: u32,
    left_micron: u32,
    left_value: f64,
    right_micron: u32,
    right_value: f64,
) -> f64 {
    let ratio = (micron as f64 - left_micron as f64) / (right_micron - left_micron) as f64;
    left_value + (right_value - left_value) * ratio
}

fn parse_micron(value: &str) -> Result<u32, String> {
    parse_micron_parts(value)?
        .into_iter()
        .max()
        .ok_or_else(|| format!("micron noto'g'ri: {value}"))
}

fn parse_micron_parts(value: &str) -> Result<Vec<u32>, String> {
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

fn parse_material_layers(value: &str) -> Vec<LayerInput> {
    value
        .split('+')
        .filter_map(parse_material_layer)
        .take(3)
        .collect()
}

fn parse_material_layer(value: &str) -> Option<LayerInput> {
    let value = value.trim();
    let micron_start = value
        .char_indices()
        .rev()
        .find(|(_, ch)| ch.is_ascii_digit())
        .map(|(index, _)| index)?;
    let mut start = micron_start;
    for (index, ch) in value[..micron_start].char_indices().rev() {
        if ch.is_ascii_digit() || matches!(ch, '/' | ',' | '.') {
            start = index;
        } else {
            break;
        }
    }

    let material = value[..start].trim();
    let micron = value[start..]
        .chars()
        .filter(|ch| ch.is_ascii_digit() || *ch == '/')
        .collect::<String>();
    if material.is_empty() || micron.is_empty() {
        return None;
    }
    Some(LayerInput::new(normalize_material_name(material), micron))
}

fn normalize_material_name(value: &str) -> String {
    let lower = value.trim().to_lowercase();
    let compact = lower.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.contains("metall") && compact.contains("bopp") {
        return "oppm".to_string();
    }
    if compact == "bopp" {
        return "opp".to_string();
    }
    if compact.starts_with("bopp ") {
        return compact.replacen("bopp", "opp", 1);
    }
    compact
}

fn visible_layers(request: &CalculateRequest) -> Vec<LayerInput> {
    [
        request.first_layer.clone(),
        request.second_layer.clone(),
        request.third_layer.clone(),
    ]
    .into_iter()
    .filter(|layer| !layer.is_empty())
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

fn is_empty_material(material: &str) -> bool {
    let n = normalize(material);
    n.is_empty() || n.chars().all(|ch| ch == '-') || matches!(n.as_str(), "yoq" | "yuq")
}

fn split_parts(value: &str) -> Vec<&str> {
    value
        .split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

fn normalize(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .collect()
}

fn close(value: &str, expected: &str) -> bool {
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

fn coefficient_error(material: &str, micron: u32, family: Family) -> String {
    let available = match family {
        Family::First | Family::McpCpp => "20, 25, 30, 35, 40, 45, 50, 60",
        Family::Jem => "25, 30",
        Family::Pe => "30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90",
        Family::Twist => "twist uchun jadval kerak emas",
        Family::Empty => "bo'sh material",
    };
    format!("'{material}' uchun {micron} mikron topilmadi. Bor mikronlar: {available}")
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
