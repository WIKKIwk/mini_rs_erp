use serde::Deserialize;
use serde::de::Error as _;

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

pub fn deserialize_optional_integer_count<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };
    let serde_json::Value::Number(number) = value else {
        return Err(D::Error::custom("count must be an integer"));
    };
    if let Some(value) = number.as_i64() {
        return Ok(Some(value));
    }
    if let Some(value) = number.as_u64().and_then(|value| i64::try_from(value).ok()) {
        return Ok(Some(value));
    }
    let Some(value) = number.as_f64() else {
        return Err(D::Error::custom("count must be an integer"));
    };
    if !value.is_finite()
        || value.fract() != 0.0
        || value < i64::MIN as f64
        || value >= i64::MAX as f64
    {
        return Err(D::Error::custom("count must be an integer"));
    }
    Ok(Some(value as i64))
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

    #[derive(Debug, Deserialize, PartialEq)]
    struct CountPayload {
        #[serde(default, deserialize_with = "deserialize_optional_integer_count")]
        count: Option<i64>,
    }

    #[test]
    fn integer_count_accepts_legacy_integral_decimal_json() {
        let payload: CountPayload = serde_json::from_str(r#"{"count":7.0}"#).expect("payload");
        assert_eq!(payload.count, Some(7));
    }

    #[test]
    fn integer_count_rejects_fractional_json() {
        let error = serde_json::from_str::<CountPayload>(r#"{"count":7.5}"#)
            .expect_err("fractional count must fail");
        assert!(error.to_string().contains("count must be an integer"));
    }
}
