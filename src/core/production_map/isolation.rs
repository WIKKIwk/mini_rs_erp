use super::ProductionMapError;

/// Prefix reserved for the isolated training workspace.
pub(crate) const TRAINING_ORDER_ID_PREFIX: &str = "training-";

/// Returns true when a value is in the reserved training namespace.
///
/// The namespace check is deliberately case-insensitive. Production routes
/// must not be able to bypass the boundary by sending `TRAINING-*` or a
/// mixed-case variant.
pub(crate) fn is_training_order_namespace(value: &str) -> bool {
    let value = value.trim();
    value
        .get(..TRAINING_ORDER_ID_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(TRAINING_ORDER_ID_PREFIX))
}

/// Returns true for a concrete training order id, excluding the bare prefix.
pub(crate) fn is_training_order_id(value: &str) -> bool {
    let value = value.trim();
    is_training_order_namespace(value)
        && value
            .get(TRAINING_ORDER_ID_PREFIX.len()..)
            .is_some_and(|suffix| suffix.chars().any(|character| !character.is_whitespace()))
}

/// Production services use this guard before every order-scoped read/write.
pub(crate) fn reject_training_order_id(value: &str) -> Result<(), ProductionMapError> {
    if is_training_order_namespace(value) {
        Err(ProductionMapError::TrainingOrderIdReserved)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_training_order_id, is_training_order_namespace, reject_training_order_id,
    };
    use crate::core::production_map::ProductionMapError;

    #[test]
    fn recognizes_training_namespace_without_case_bypass() {
        assert!(is_training_order_namespace("training-zakaz-0001"));
        assert!(is_training_order_namespace("TRAINING-zakaz-0001"));
        assert!(is_training_order_namespace(" TrAiNiNg-zakaz-0001 "));
        assert!(is_training_order_id("training-zakaz-0001"));
        assert!(is_training_order_id("TRAINING-zakaz-0001"));
    }

    #[test]
    fn rejects_bare_prefix_but_reserves_it_for_production() {
        assert!(is_training_order_namespace("training-"));
        assert!(!is_training_order_id("training-"));
        assert_eq!(
            reject_training_order_id("training-"),
            Err(ProductionMapError::TrainingOrderIdReserved)
        );
    }

    #[test]
    fn accepts_production_namespace() {
        assert!(!is_training_order_namespace("zakaz-0001"));
        assert!(!is_training_order_id("production-0001"));
        assert_eq!(reject_training_order_id("zakaz-0001"), Ok(()));
    }

    #[test]
    fn does_not_panic_or_classify_non_ascii_prefix_as_training() {
        assert!(!is_training_order_namespace("тренинг-zakaz-0001"));
        assert!(!is_training_order_id("тренинг-zakaz-0001"));
        assert_eq!(reject_training_order_id("тренинг-zakaz-0001"), Ok(()));
    }
}
