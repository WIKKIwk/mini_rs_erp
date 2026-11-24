fn upgrade_stored_material(
    mut material: CalculateMaterial,
    defaults: &[CalculateMaterial],
) -> CalculateMaterial {
    if material.density_g_cm3 <= 0.0
        && let Some(default) = defaults.iter().find(|default| default.id == material.id)
    {
        material.density_g_cm3 = default.density_g_cm3;
    }
    for variant in &mut material.variants {
        if material.density_g_cm3 <= 0.0
            && variant.actual_gsm.is_none()
            && variant.coefficient.is_finite()
            && variant.coefficient > 0.0
        {
            variant.actual_gsm = Some(legacy_coefficient_to_gsm(variant.coefficient));
        }
        if let Some(gsm) = effective_variant_gsm(material.density_g_cm3, variant) {
            variant.coefficient = gsm_to_legacy_coefficient(gsm);
            variant.first_layer_coefficient = None;
        }
    }
    material
}

pub fn effective_variant_gsm(
    density_g_cm3: f64,
    variant: &CalculateMaterialVariant,
) -> Option<f64> {
    variant.actual_gsm.or_else(|| {
        (density_g_cm3.is_finite() && density_g_cm3 > 0.0)
            .then(|| f64::from(variant.micron) * density_g_cm3)
            .or_else(|| {
                (variant.coefficient.is_finite() && variant.coefficient > 0.0)
                    .then(|| legacy_coefficient_to_gsm(variant.coefficient))
            })
    })
}

pub fn legacy_coefficient_to_gsm(coefficient: f64) -> f64 {
    coefficient * (1_000_000.0 / 60_000.0)
}

pub fn gsm_to_legacy_coefficient(gsm: f64) -> f64 {
    gsm * (60_000.0 / 1_000_000.0)
}

fn default_active() -> bool {
    true
}

fn unix_micros() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default()
}
