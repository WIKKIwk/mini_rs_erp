use super::{close, normalize, parse_micron_parts, split_parts};

pub(super) fn coefficient_cell(
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
