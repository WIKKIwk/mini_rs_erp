use std::collections::{BTreeMap, BTreeSet};

use crate::core::auth::models::PrincipalRole;
use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::production_map::{ProductionMapDefinition, ProductionMapSaved};

use super::*;

pub(super) fn effective_progress_qr_payload<'a>(
    qr_payload: &'a str,
    legacy_progress_qr: &'a str,
) -> &'a str {
    if qr_payload.trim().is_empty() {
        legacy_progress_qr
    } else {
        qr_payload
    }
}

include!("helpers_parts/part_01.rs");
include!("helpers_parts/part_02.rs");

#[cfg(test)]
mod progress_qr_payload_tests {
    use super::effective_progress_qr_payload;

    #[test]
    fn canonical_qr_payload_precedes_the_legacy_alias() {
        assert_eq!(effective_progress_qr_payload("qr:new", "qr:old"), "qr:new");
        assert_eq!(effective_progress_qr_payload("", "qr:old"), "qr:old");
        assert_eq!(effective_progress_qr_payload("  ", "qr:old"), "qr:old");
    }
}
