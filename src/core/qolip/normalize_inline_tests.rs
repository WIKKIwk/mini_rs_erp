#[cfg(test)]
mod tests {
    use crate::core::auth::models::{Principal, PrincipalRole};

    use super::super::models::{QolipBlock, QolipError, QolipLocationUpsert};
    use super::{normalize_location, resolve_cell_qr_from_payload};

    fn principal() -> Principal {
        Principal {
            role: PrincipalRole::Qolipchi,
            display_name: "Ali".to_string(),
            legal_name: "Ali".to_string(),
            ref_: "worker-1".to_string(),
            phone: "+998901234567".to_string(),
            avatar_url: String::new(),
        }
    }

    #[test]
    fn normalize_location_requires_numeric_size_and_column_range() {
        let base = QolipLocationUpsert {
            block: "A".to_string(),
            item_code: "VELONA".to_string(),
            item_name: "Velona".to_string(),
            qolip_code: "Q-1".to_string(),
            size: 12,
            quantity: 9,
            row_letter: "a".to_string(),
            column_number: Some(1),
            ..QolipLocationUpsert::default()
        };
        let normalized = normalize_location(base.clone(), &principal()).expect("valid location");
        assert_eq!(normalized.row_letter, "A");
        assert_eq!(normalized.location_label, "A1");

        let invalid = QolipLocationUpsert {
            column_number: Some(14),
            ..base
        };
        assert_eq!(
            normalize_location(invalid, &principal()),
            Err(QolipError::InvalidLocation)
        );
    }

    #[test]
    fn resolve_cell_qr_from_payload_matches_deterministic_qr() {
        use super::super::models::QolipCellQrInput;
        use super::normalize_cell_qr;

        let blocks = vec![QolipBlock {
            name: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
        }];
        let cell = resolve_cell_qr_from_payload("INVALID", &blocks, &principal());
        assert!(cell.is_none());

        let seed = normalize_cell_qr(
            QolipCellQrInput {
                block: "A".to_string(),
                warehouse: "Qolip ombor".to_string(),
                row_letter: "B".to_string(),
                column_number: Some(13),
            },
            &principal(),
        )
        .expect("cell");
        let resolved = resolve_cell_qr_from_payload(&seed.qr_payload, &blocks, &principal())
            .expect("resolved");
        assert_eq!(resolved.location_label, "B13");
        assert_eq!(resolved.block, "A");
    }
}
