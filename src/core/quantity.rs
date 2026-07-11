pub const ERP_QUANTITY_DECIMAL_PLACES: u32 = 9;

const ERP_QUANTITY_FACTOR: f64 = 1_000_000_000.0;

pub fn normalize_erp_quantity(value: f64) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    let scaled = value * ERP_QUANTITY_FACTOR;
    if !scaled.is_finite() {
        return None;
    }
    Some(scaled.round() / ERP_QUANTITY_FACTOR)
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
    }

    #[test]
    fn removes_binary_float_noise_at_the_storage_boundary() {
        assert_eq!(normalize_erp_quantity(0.1 + 0.2), Some(0.3));
    }

    #[test]
    fn rejects_non_finite_values_and_positive_values_rounded_to_zero() {
        assert_eq!(normalize_erp_quantity(f64::NAN), None);
        assert_eq!(normalize_erp_quantity(f64::INFINITY), None);
        assert_eq!(positive_erp_quantity(0.0000000001), None);
    }
}
