#[cfg(test)]
mod tests {
    use super::{
        ApparatusQueueActionRequest, QueueActionCommand, QueueApparatusMetadata,
        QueueActionDecision, canonical_queue_action, parse_canonical_queue_apparatus_id,
        plan_queue_action, returned_paint_queue_error,
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

    fn command_from_json(
        value: serde_json::Value,
        operation: ExecutionOperation,
    ) -> (QueueActionCommand, QueueApparatusMetadata) {
        let request: ApparatusQueueActionRequest =
            serde_json::from_value(value).expect("queue request");
        let apparatus = QueueApparatusMetadata {
            id: ApparatusId::new("apparatus:catalog:test-001").unwrap(),
            display_name: "Test apparatus".to_string(),
            operation,
            qolip_scan_required: false,
        };
        let command = QueueActionCommand::from_request(
            request,
            &apparatus,
            &principal(PrincipalRole::Admin),
        )
        .expect("queue command");
        (command, apparatus)
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

    #[test]
    fn legacy_queue_fields_normalize_once_at_the_http_boundary() {
        let request: ApparatusQueueActionRequest = serde_json::from_value(serde_json::json!({
            "apparatus": "legacy display is replaced by resolved identity",
            "order_id": "zakaz-normalized-command",
            "action": "complete",
            "qty": 12.5,
            "unit": "m",
            "progress_qr": "WIP-LEGACY-1",
            "description": "legacy completion note",
            "material_barcode": "RAW-LEGACY",
            "material_barcodes": ["RAW-1", "RAW-2"],
            "qolip_code": "QOLIP-A",
            "qolip_codes": ["qolip-a", "QOLIP-B"]
        }))
        .expect("legacy request");
        let apparatus = QueueApparatusMetadata {
            id: ApparatusId::new("apparatus:catalog:pechat-001").unwrap(),
            display_name: "Pechat".to_string(),
            operation: ExecutionOperation::Print,
            qolip_scan_required: true,
        };

        let command = QueueActionCommand::from_request(
            request,
            &apparatus,
            &principal(PrincipalRole::Admin),
        )
        .expect("normalized command");

        assert_eq!(command.apparatus, "apparatus:catalog:pechat-001");
        assert_eq!(command.progress.produced_qty, Some(12.5));
        assert_eq!(command.progress.uom, "m");
        assert_eq!(command.print.submitted_uom, "m");
        assert_eq!(command.progress.qr_payload, "WIP-LEGACY-1");
        assert_eq!(command.progress.description, "legacy completion note");
        assert_eq!(command.materials.combined_barcode, "RAW-1,RAW-2");
        assert_eq!(command.materials.qolip_codes, ["qolip-a", "QOLIP-B"]);
    }

    #[test]
    fn queue_decision_requires_rezka_progress_metrics() {
        let (command, apparatus) = command_from_json(
            serde_json::json!({
                "apparatus": "apparatus:catalog:test-001",
                "order_id": "zakaz-rezka-plan",
                "action": "pause"
            }),
            ExecutionOperation::Cut,
        );

        let (status, axum::Json(body)) =
            plan_queue_action(&command, &apparatus, None, false, false)
                .expect_err("rezka metrics required");

        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(body.error, "rezka_progress_metrics_required");
    }

    #[test]
    fn queue_decision_routes_explained_missing_output_to_admin_completion() {
        let (command, apparatus) = command_from_json(
            serde_json::json!({
                "apparatus": "apparatus:catalog:test-001",
                "order_id": "zakaz-completion-plan",
                "action": "complete",
                "description": "output unavailable"
            }),
            ExecutionOperation::Laminate,
        );

        let decision = plan_queue_action(&command, &apparatus, None, false, false)
            .expect("completion decision");

        assert!(matches!(
            decision,
            QueueActionDecision::RequestCompletion { note, zero_metric_codes }
                if note == "output unavailable" && zero_metric_codes.is_empty()
        ));
    }
}
