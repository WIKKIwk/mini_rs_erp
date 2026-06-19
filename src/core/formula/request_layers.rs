use super::{CalculateRequest, LayerInput, parse_micron_parts, split_parts};

pub(super) fn hydrate_layers_from_material_display(request: &mut CalculateRequest) {
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

pub(super) fn request_variants(request: &CalculateRequest) -> Vec<CalculateRequest> {
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

pub(super) fn visible_layers(request: &CalculateRequest) -> Vec<LayerInput> {
    [
        request.first_layer.clone(),
        request.second_layer.clone(),
        request.third_layer.clone(),
    ]
    .into_iter()
    .filter(|layer| !layer.is_empty())
    .collect()
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
