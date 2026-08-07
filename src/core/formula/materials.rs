use super::{close, normalize, parse_micron_parts, split_parts};
use crate::core::calculate_materials::{
    CalculateMaterial, CalculateMaterialVariant, effective_variant_gsm, normalize_key,
};

pub(super) fn gsm_cell_with_catalog(
    material: &str,
    material_id: &str,
    micron_text: &str,
    micron: u32,
    catalog: &[CalculateMaterial],
) -> Result<f64, String> {
    let catalog_material = if material_id.trim().is_empty() {
        let material_key = normalize_key(material);
        let Some(catalog_material) = catalog
            .iter()
            .find(|candidate| normalize_key(&candidate.name) == material_key)
        else {
            if !catalog.is_empty() {
                return Err(format!("material katalogdan tanlanmagan: {material}"));
            }
            return gsm_cell(material, micron_text, micron);
        };
        catalog_material
    } else {
        catalog
            .iter()
            .find(|candidate| candidate.id.trim() == material_id.trim())
            .ok_or_else(|| format!("material topilmadi: {material}"))?
    };
    let microns = parse_micron_parts(micron_text)?;
    if microns.len() != 1 {
        return Err(format!(
            "material/mikron mos emas: {material} / {micron_text}"
        ));
    }
    let variant = catalog_material
        .variants
        .iter()
        .find(|variant| variant.micron == micron)
        .ok_or_else(|| {
            let available = catalog_material
                .variants
                .iter()
                .map(|variant| variant.micron.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "'{}' uchun {} mikron topilmadi. Bor mikronlar: {}",
                catalog_material.name, micron, available
            )
        })?;
    variant_gsm(catalog_material, variant)
}

fn variant_gsm(
    material: &CalculateMaterial,
    variant: &CalculateMaterialVariant,
) -> Result<f64, String> {
    effective_variant_gsm(material.density_g_cm3, variant)
        .filter(|gsm| gsm.is_finite() && *gsm > 0.0)
        .ok_or_else(|| {
            format!(
                "'{}' uchun zichlik yoki actual GSM kiritilmagan",
                material.name
            )
        })
}

pub(super) fn gsm_cell(material: &str, micron_text: &str, micron: u32) -> Result<f64, String> {
    let materials = split_parts(material);
    let microns = parse_micron_parts(micron_text)?;
    if materials.len() == 1 {
        return gsm_single(materials[0], micron);
    }
    if materials.len() != microns.len() {
        return Err(format!(
            "material/mikron mos emas: {material} / {micron_text}"
        ));
    }
    materials
        .iter()
        .zip(microns)
        .map(|(material, micron)| gsm_single(material, micron))
        .sum()
}

fn gsm_single(material: &str, micron: u32) -> Result<f64, String> {
    let family = material_family(material)?;
    let gsm = match family {
        Family::Pet => Some(f64::from(micron) * 1.40),
        Family::Opp => Some(f64::from(micron) * 0.91),
        Family::Cpp => Some(f64::from(micron) * 0.90),
        Family::Pe => Some(f64::from(micron) * 0.92),
        Family::Jem => legacy_jem_gsm(micron),
        Family::Twist => Some(1_000_000.0 / 30_000.0),
        Family::Empty => None,
    };
    gsm.filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| format!("'{material}' uchun zichlik yoki actual GSM kerak"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Pet,
    Opp,
    Cpp,
    Pe,
    Jem,
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
        return Ok(Family::Pet);
    }
    if n.starts_with("opp")
        || n.starts_with("popp")
        || n == "st01"
        || n.starts_with("mat")
        || n.starts_with("pff")
        || n.starts_with("pf")
        || close(&n, "opp")
    {
        return Ok(Family::Opp);
    }
    if matches!(n.as_str(), "map" | "mcpp" | "msr" | "msp")
        || n.starts_with("cpp")
        || n.starts_with("mcp")
        || close(&n, "cpp")
        || close(&n, "mcp")
    {
        return Ok(Family::Cpp);
    }
    if n.starts_with("pe") || close(&n, "pe") {
        return Ok(Family::Pe);
    }
    if n.starts_with("jem") || close(&n, "jem") {
        return Ok(Family::Jem);
    }
    Err(format!("noma'lum material: {material}"))
}

fn legacy_jem_gsm(micron: u32) -> Option<f64> {
    interpolate(
        micron,
        &[(25, 1_000_000.0 / 60_000.0), (30, 1_000_000.0 / 40_000.0)],
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
    if micron <= *first_micron {
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
            return Some(project(
                micron,
                left_micron,
                left_value,
                right_micron,
                right_value,
            ));
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
