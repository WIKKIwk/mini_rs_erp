pub const ERP_QUANTITY_DECIMAL_PLACES: u32 = 6;

pub const ERP_QUANTITY_FACTOR: i64 = 1_000_000;

const ERP_QUANTITY_INTEGER_LIMIT: f64 = 1_000_000_000_000.0;

pub fn erp_quantity_to_units(value: f64) -> Option<i64> {
    if !value.is_finite() || value.abs() >= ERP_QUANTITY_INTEGER_LIMIT {
        return None;
    }
    let scaled = value * ERP_QUANTITY_FACTOR as f64;
    Some(scaled.round() as i64)
}

pub fn erp_quantity_from_units(units: i64) -> f64 {
    units as f64 / ERP_QUANTITY_FACTOR as f64
}

pub fn normalize_erp_quantity(value: f64) -> Option<f64> {
    erp_quantity_to_units(value).map(erp_quantity_from_units)
}

pub fn positive_erp_quantity(value: f64) -> Option<f64> {
    normalize_erp_quantity(value).filter(|value| *value > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_sub_milligram_inventory_precision() {
        assert_eq!(normalize_erp_quantity(13.00003), Some(13.00003));
        assert_eq!(erp_quantity_to_units(13.00003), Some(13_000_030));
    }

    #[test]
    fn removes_binary_float_noise_at_the_storage_boundary() {
        assert_eq!(normalize_erp_quantity(0.1 + 0.2), Some(0.3));
    }

    #[test]
    fn rejects_non_finite_values_and_positive_values_rounded_to_zero() {
        assert_eq!(normalize_erp_quantity(f64::NAN), None);
        assert_eq!(normalize_erp_quantity(f64::INFINITY), None);
        assert_eq!(positive_erp_quantity(0.0000001), None);
        assert_eq!(erp_quantity_to_units(f64::MAX), None);
        assert_eq!(erp_quantity_to_units(1_000_000_000_000.0), None);
    }

    #[test]
    fn rounds_to_the_database_scale() {
        assert_eq!(normalize_erp_quantity(1.234_567_8), Some(1.234_568));
    }
}
