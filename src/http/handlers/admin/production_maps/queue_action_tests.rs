#[cfg(test)]
mod tests {
    use super::{
        QueueApparatusMetadata, canonical_queue_action, parse_canonical_queue_apparatus_id,
        returned_paint_queue_error,
    };
    use crate::core::apparatus_standard::{ApparatusId, ExecutionOperation};
    use crate::core::auth::models::{Principal, PrincipalRole};
    use crate::core::production_map::queue_state::ApparatusQueueAction;
    use crate::core::returned_paint::ReturnedPaintError;

    fn principal(role: PrincipalRole) -> Principal {
        Principal {
            role,
            display_name: "Test".to_string(),
            legal_name: String::new(),
            ref_: "test-ref".to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        }
    }

    #[test]
    fn legacy_worker_pause_maps_to_detach_roll_but_admin_and_freeze_pause_do_not() {
        let worker = principal(PrincipalRole::Aparatchi);
        let admin = principal(PrincipalRole::Admin);

        assert_eq!(
            canonical_queue_action(
                ApparatusQueueAction::Pause,
                false,
                false,
                "",
                false,
                &worker
            ),
            ApparatusQueueAction::DetachRoll
        );
        assert_eq!(
            canonical_queue_action(
                ApparatusQueueAction::Pause,
                false,
                false,
                "freeze-request",
                false,
                &worker,
            ),
            ApparatusQueueAction::Pause
        );
        assert_eq!(
            canonical_queue_action(ApparatusQueueAction::Pause, true, false, "", false, &worker),
            ApparatusQueueAction::Pause
        );
        assert_eq!(
            canonical_queue_action(ApparatusQueueAction::Pause, false, false, "", false, &admin),
            ApparatusQueueAction::Pause
        );
        assert_eq!(
            canonical_queue_action(ApparatusQueueAction::Pause, false, false, "", true, &worker),
            ApparatusQueueAction::Freeze
        );
    }

    #[test]
    fn qolip_scan_uses_canonical_tooling_policy_not_pechat_classification() {
        let pechat = QueueApparatusMetadata {
            id: ApparatusId::new("apparatus:catalog:pechat-001").unwrap(),
            display_name: "Pechat".to_string(),
            operation: ExecutionOperation::Print,
            qolip_scan_required: false,
        };
        let scan_required_pechat = QueueApparatusMetadata {
            id: ApparatusId::new("apparatus:catalog:pechat-002").unwrap(),
            display_name: "Pechat 2".to_string(),
            operation: ExecutionOperation::Print,
            qolip_scan_required: true,
        };
        let flexo = QueueApparatusMetadata {
            id: ApparatusId::new("apparatus:catalog:flexo-001").unwrap(),
            display_name: "Flexo".to_string(),
            operation: ExecutionOperation::Print,
            qolip_scan_required: false,
        };
        let laminatsiya = QueueApparatusMetadata {
            id: ApparatusId::new("apparatus:catalog:laminatsiya-001").unwrap(),
            display_name: "Laminatsiya".to_string(),
            operation: ExecutionOperation::Laminate,
            qolip_scan_required: false,
        };
        let rezka = QueueApparatusMetadata {
            id: ApparatusId::new("apparatus:catalog:rezka-001").unwrap(),
            display_name: "Rezka".to_string(),
            operation: ExecutionOperation::Cut,
            qolip_scan_required: false,
        };

        assert!(!pechat.requires_qolip_scan());
        assert!(scan_required_pechat.requires_qolip_scan());
        assert!(!flexo.requires_qolip_scan());
        assert!(!laminatsiya.requires_qolip_scan());
        assert!(!rezka.requires_qolip_scan());
    }

    #[test]
    fn queue_apparatus_identity_rejects_legacy_display_title() {
        assert!(parse_canonical_queue_apparatus_id("7 ta rangli pechat - A").is_err());
        assert_eq!(
            parse_canonical_queue_apparatus_id("apparatus:catalog:pechat-001")
                .ok()
                .map(|id| id.as_str().to_string()),
            Some("apparatus:catalog:pechat-001".to_string())
        );
    }

    #[test]
    fn astatka_exceeding_rasxot_returns_stable_queue_error_code() {
        let (status, axum::Json(body)) =
            returned_paint_queue_error(ReturnedPaintError::NegativeFinalValue);

        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(body.error, "returned_paint_astatka_exceeds_rasxot");
    }
}
