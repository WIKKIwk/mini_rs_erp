use super::*;

#[test]
fn qolip_in_use_checkout_error_is_returned_as_business_error() {
    assert_eq!(
        production_map_qolip_checkout_error(QolipError::QolipInUse),
        ProductionMapError::QolipAlreadyInUse
    );
}
