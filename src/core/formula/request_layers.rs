use super::{CalculateRequest, LayerInput, MicronOverride, parse_micron_parts, split_parts};

const MAX_MICRON_CALCULATIONS: usize = 64;

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
    if !request.effective_layers().is_empty() {
        return;
    }
    let layers = parse_material_layers(request.material_display.as_deref().unwrap_or(""));
    request.layers = layers.clone();
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

pub(crate) fn validate_micron_options(layers: &[LayerInput]) -> Result<(), String> {
    let mut count = 1usize;
    for (index, layer) in layers.iter().enumerate() {
        let options = micron_options(layer).map_err(|error| format!("{}-qavat: {error}", index + 1))?;
        count = count.saturating_mul(options.len().max(1));
        if count > MAX_MICRON_CALCULATIONS {
            return Err(format!("Hisob variantlari {MAX_MICRON_CALCULATIONS} tadan oshmasligi kerak"));
        }
    }
    Ok(())
}

// The selected production plan comes first so order creation and edits use
// that result. The other results still describe the full approved choices.
pub(super) fn request_variants(
    request: &CalculateRequest,
) -> Result<Vec<(CalculateRequest, Vec<MicronOverride>)>, String> {
    let layers = request.effective_layers();
    validate_micron_options(&layers)?;
    let has_micron_options = layers.iter().any(|layer| !layer.alternative_microns.is_empty());
    let mut initial = request.clone();
    initial.layers = layers.clone();
    let mut variants = vec![(initial, Vec::new())];
    for (index, layer) in layers.iter().enumerate() {
        let options = micron_options(layer)?;
        let materials = if options.is_empty() {
            alternatives(&layer.material, &layer.micron)
        } else {
            vec![layer.material.clone()]
        };
        let count = variants.len().saturating_mul(materials.len()).saturating_mul(options.len().max(1));
        if has_micron_options && count > MAX_MICRON_CALCULATIONS {
            return Err(format!("Hisob variantlari {MAX_MICRON_CALCULATIONS} tadan oshmasligi kerak"));
        }
        let mut expanded = Vec::with_capacity(count);
        for (variant, overrides) in variants {
            for material in &materials {
                if options.is_empty() {
                    let mut next = variant.clone();
                    next.layers[index].material.clone_from(material);
                    expanded.push((next, overrides.clone()));
                    continue;
                }
                let primary = layer.micron.trim().parse::<u32>().ok();
                for &micron in &options {
                    let mut next = variant.clone();
                    let mut next_overrides = overrides.clone();
                    next.layers[index].micron = micron.to_string();
                    if Some(micron) != primary {
                        next_overrides.push(MicronOverride { layer_index: index, micron });
                    }
                    expanded.push((next, next_overrides));
                }
            }
        }
        variants = expanded;
    }
    Ok(variants)
}

fn micron_options(layer: &LayerInput) -> Result<Vec<u32>, String> {
    if layer.alternative_microns.is_empty() && layer.planned_micron.is_none() {
        return Ok(Vec::new());
    }
    if layer.alternative_microns.len() >= MAX_MICRON_CALCULATIONS {
        return Err(format!("Hisob variantlari {MAX_MICRON_CALCULATIONS} tadan oshmasligi kerak"));
    }
    let primary = layer.micron.trim().parse::<u32>().ok().filter(|micron| *micron > 0)
        .ok_or("Asosiy mikron musbat butun son bo‘lishi kerak")?;
    let mut options = vec![primary];
    for &micron in &layer.alternative_microns {
        if micron == 0 || options.contains(&micron) {
            return Err("Muqobil mikronlar musbat va takrorlanmas bo‘lishi kerak".into());
        }
        options.push(micron);
    }
    if let Some(planned) = layer.planned_micron {
        let position = options.iter().position(|&micron| micron == planned)
            .ok_or("Reja uchun tanlangan mikron qavat variantlarida yo‘q")?;
        options[..=position].rotate_right(1);
    }
    Ok(options)
}

pub(super) fn visible_layers(request: &CalculateRequest) -> Vec<LayerInput> {
    request
        .effective_layers()
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
    value.split('+').filter_map(parse_material_layer).collect()
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
