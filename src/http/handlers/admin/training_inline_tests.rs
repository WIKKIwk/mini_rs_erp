#[cfg(test)]
mod tests {
    use super::*;

    fn principal() -> Principal {
        Principal {
            role: PrincipalRole::Aparatchi,
            display_name: "Rezka operator".to_string(),
            legal_name: String::new(),
            ref_: "rezka-operator".to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        }
    }

    fn node(id: &str, kind: ProductionMapNodeKind, title: &str) -> ProductionMapNode {
        let (apparatus_id, role_code) = match (kind.clone(), title) {
            (ProductionMapNodeKind::Apparatus, "Laminatsiya aparat") => (
                "apparatus:test:laminatsiya-1".to_string(),
                TRAINING_LAMINATSIYA_ROLE.to_string(),
            ),
            (ProductionMapNodeKind::Apparatus, "Rezka aparat") => (
                "apparatus:test:rezka-1".to_string(),
                TRAINING_REZKA_ROLE.to_string(),
            ),
            _ => (String::new(), String::new()),
        };
        ProductionMapNode {
            id: id.to_string(),
            kind,
            title: title.to_string(),
            apparatus_id,
            formula: None,
            role_code,
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: String::new(),
            alternative_group_label: String::new(),
            alternative_assigned_title: String::new(),
            alternative_assigned_apparatus_id: String::new(),
            rezka_kadr_count: None,
            rezka_frame_groups: Vec::new(),
            rezka_label_length: None,
            x: 0.0,
            y: 0.0,
        }
    }

    fn laminatsiya_training_map() -> ProductionMapDefinition {
        ProductionMapDefinition {
            id: "training-laminatsiya-1".to_string(),
            product_code: "TRAINING-1".to_string(),
            title: "Training laminatsiya".to_string(),
            code: String::new(),
            order_number: String::new(),
            customer_name: String::new(),
            roll_count: None,
            width_mm: None,
            order_kg: Some(12.0),
            base_length: None,
            nodes: vec![
                node("start", ProductionMapNodeKind::Start, "Boshlanish"),
                node(
                    "lam",
                    ProductionMapNodeKind::Apparatus,
                    "Laminatsiya aparat",
                ),
                node("end", ProductionMapNodeKind::End, "Tugash"),
            ],
            edges: vec![
                ProductionMapEdge {
                    from: "start".to_string(),
                    to: "lam".to_string(),
                    branch: String::new(),
                },
                ProductionMapEdge {
                    from: "lam".to_string(),
                    to: "end".to_string(),
                    branch: String::new(),
                },
            ],
        }
    }

    fn training_input_identity() -> TrainingInputBatchIdentity {
        let batch_id = "progress-batch:1770000000000000000:training-input:bosma:training-laminatsiya-1:complete"
            .to_string();
        TrainingInputBatchIdentity {
            order_id: "training-laminatsiya-1".to_string(),
            apparatus: "apparatus:test:laminatsiya-1".to_string(),
            qr_payload: crate::core::production_map::progress_qr_payload(&batch_id),
            batch_id,
            session_id: "training-input-session:progress-batch:1770000000000000000:training-input:bosma:training-laminatsiya-1:complete"
                .to_string(),
        }
    }

    fn rezka_training_map() -> ProductionMapDefinition {
        let mut map = laminatsiya_training_map();
        map.id = "training-rezka-1".to_string();
        map.title = "Training rezka".to_string();
        map.nodes[1].id = "rezka".to_string();
        map.nodes[1].title = "Rezka aparat".to_string();
        map.nodes[1].apparatus_id = "apparatus:test:rezka-1".to_string();
        map.nodes[1].role_code = TRAINING_REZKA_ROLE.to_string();
        map.edges[0].to = "rezka".to_string();
        map.edges[1].from = "rezka".to_string();
        map
    }

    fn rezka_training_input_identity() -> TrainingInputBatchIdentity {
        let batch_id = "progress-batch:1770000000000000000:training-input:laminatsiya:training-rezka-1:complete"
            .to_string();
        TrainingInputBatchIdentity {
            order_id: "training-rezka-1".to_string(),
            apparatus: "apparatus:test:rezka-1".to_string(),
            qr_payload: crate::core::production_map::progress_qr_payload(&batch_id),
            batch_id,
            session_id: "training-input-session:progress-batch:1770000000000000000:training-input:laminatsiya:training-rezka-1:complete"
                .to_string(),
        }
    }

    #[test]
    fn training_order_request_accepts_mobile_decimal_roll_count() {
        let input: TrainingMapSaveWithOrderRequest = serde_json::from_value(serde_json::json!({
            "map": {
                "id": "training-decimal-roll-count",
                "product_code": "TRAINING-7701",
                "title": "Training decimal roll count",
                "roll_count": 7.0,
                "nodes": [],
                "edges": []
            },
            "template": {
                "name": "training mahsulot",
                "product": "training mahsulot",
                "roll_count": 7.0
            }
        }))
        .expect("training order request");

        assert_eq!(input.map.roll_count, Some(7));
        assert_eq!(input.template.roll_count, Some(7));
    }

    #[test]
    fn new_training_order_snapshots_rezka_frame_count() {
        let mut map = rezka_training_map();
        let template = CalculateOrderTemplate {
            frame_count: 4.0,
            ..CalculateOrderTemplate::default()
        };
        let cut_apparatus_ids = BTreeSet::from([
            ApparatusId::new("apparatus:test:rezka-1").expect("canonical test cut id")
        ]);

        super::production_maps::apply_order_rezka_kadr_count(
            &mut map,
            &template,
            &cut_apparatus_ids,
        );

        assert_eq!(map.nodes[1].rezka_kadr_count, Some(4));

        map.order_number = "T-0001".to_string();
        let edited_template = CalculateOrderTemplate {
            frame_count: 8.0,
            ..CalculateOrderTemplate::default()
        };
        if map.order_number.trim().is_empty() {
            super::production_maps::apply_order_rezka_kadr_count(
                &mut map,
                &edited_template,
                &cut_apparatus_ids,
            );
        }

        assert_eq!(map.nodes[1].rezka_kadr_count, Some(4));
    }

    #[test]
    fn laminatsiya_training_map_gets_virtual_bosma_input() {
        let map = laminatsiya_training_map();

        assert_eq!(
            training_input_stage_for_map(&map, "apparatus:test:laminatsiya-1").as_deref(),
            Some(TRAINING_VIRTUAL_INPUT_BOSMA)
        );

        let worker_map = training_worker_map(map);
        let input = worker_map
            .nodes
            .iter()
            .find(|item| is_training_input_node(item))
            .expect("virtual training input node");
        assert_eq!(input.title, TRAINING_LAMINATSIYA_INPUT_APPARATUS);
        assert!(
            worker_map
                .edges
                .iter()
                .any(|edge| { edge.from == "start" && edge.to == input.id })
        );
        assert!(
            worker_map
                .edges
                .iter()
                .any(|edge| { edge.from == input.id && edge.to == "lam" })
        );

        let identity = training_input_identity();
        let batch =
            training_input_progress_batch(&worker_map, "apparatus:test:laminatsiya-1", &identity)
                .expect("virtual training input batch");
        assert_eq!(batch.qr_payload, identity.qr_payload);
        assert_eq!(batch.batch_id, identity.batch_id);
        assert_eq!(
            crate::core::production_map::progress_qr_payload(&batch.batch_id),
            batch.qr_payload,
        );
        assert_eq!(batch.apparatus, "apparatus:test:laminatsiya-1");
        assert_eq!(batch.next_apparatus, "apparatus:test:laminatsiya-1");
        assert_eq!(batch.wip_status, OrderProgressBatchWipStatus::Waiting);
    }

    #[test]
    fn rezka_training_map_gets_virtual_laminatsiya_input() {
        let map = rezka_training_map();

        assert_eq!(
            training_input_stage_for_map(&map, "apparatus:test:rezka-1").as_deref(),
            Some(TRAINING_VIRTUAL_INPUT_LAMINATSIYA)
        );

        let worker_map = training_worker_map(map);
        let input = worker_map
            .nodes
            .iter()
            .find(|item| is_training_input_node(item))
            .expect("virtual rezka input node");
        assert_eq!(input.title, TRAINING_REZKA_INPUT_APPARATUS);
        assert!(
            worker_map
                .edges
                .iter()
                .any(|edge| edge.from == "start" && edge.to == input.id)
        );
        assert!(
            worker_map
                .edges
                .iter()
                .any(|edge| edge.from == input.id && edge.to == "rezka")
        );

        let identity = rezka_training_input_identity();
        let batch = training_input_progress_batch(&worker_map, "apparatus:test:rezka-1", &identity)
            .expect("virtual rezka input batch");
        assert_eq!(batch.qr_payload, identity.qr_payload);
        assert_eq!(batch.batch_id, identity.batch_id);
        assert_eq!(batch.apparatus, "apparatus:test:rezka-1");
        assert_eq!(batch.next_apparatus, "apparatus:test:rezka-1");
        assert_eq!(batch.wip_status, OrderProgressBatchWipStatus::Waiting);
    }

    #[test]
    fn training_input_batch_set_uses_partial_then_full_completion() {
        let worker_map = training_worker_map(laminatsiya_training_map());
        let first_identity = training_input_identity();
        let mut second_identity = training_input_identity();
        second_identity.batch_id = "training-input-batch-2".to_string();
        second_identity.session_id = "training-input-session-2".to_string();
        second_identity.qr_payload = progress_qr_payload(&second_identity.batch_id);
        let first = training_input_progress_batch(
            &worker_map,
            "apparatus:test:laminatsiya-1",
            &first_identity,
        )
        .expect("first training input");
        let second = training_input_progress_batch(
            &worker_map,
            "apparatus:test:laminatsiya-1",
            &second_identity,
        )
        .expect("second training input");
        let claimed_first = training_claim_input_batch(
            &first,
            "apparatus:test:laminatsiya-1",
            "training-laminatsiya-1",
        );

        assert!(training_has_unprocessed_previous_wips(
            &[claimed_first.clone(), second.clone()],
            "training-laminatsiya-1",
            TRAINING_VIRTUAL_INPUT_BOSMA,
            "apparatus:test:laminatsiya-1",
            &claimed_first.batch_id,
        ));
        assert!(!training_complete_requires_full_report(
            &laminatsiya_training_map(),
            "apparatus:test:laminatsiya-1",
            true,
        ));
        assert!(!training_complete_requires_full_report(
            &rezka_training_map(),
            "apparatus:test:rezka-1",
            true
        ));

        let processed_first = training_process_input_batch(
            &claimed_first,
            "apparatus:test:laminatsiya-1",
            "training-laminatsiya-1",
        );
        let claimed_second = training_claim_input_batch(
            &second,
            "apparatus:test:laminatsiya-1",
            "training-laminatsiya-1",
        );
        assert!(!training_has_unprocessed_previous_wips(
            &[processed_first, claimed_second.clone()],
            "training-laminatsiya-1",
            TRAINING_VIRTUAL_INPUT_BOSMA,
            "apparatus:test:laminatsiya-1",
            &claimed_second.batch_id,
        ));
        assert!(training_complete_requires_full_report(
            &laminatsiya_training_map(),
            "apparatus:test:laminatsiya-1",
            false,
        ));
        assert!(training_complete_requires_full_report(
            &rezka_training_map(),
            "apparatus:test:rezka-1",
            false
        ));
    }

    #[test]
    fn training_input_qr_order_id_is_case_insensitive() {
        assert_eq!(
            training_input_order_id_from_qr("TRAINING-INPUT:TRAINING-ZAKAZ-0004").as_deref(),
            Some("training-zakaz-0004"),
        );
        assert_eq!(
            training_input_order_id_from_qr("training-input:training-zakaz-0004").as_deref(),
            Some("training-zakaz-0004"),
        );
        assert_eq!(training_input_order_id_from_qr("GSP:PROGRESS-1"), None);
    }

    #[test]
    fn training_selection_is_independent_of_display_rename() {
        let mut map = laminatsiya_training_map();
        let saved = ProductionMapSaved {
            program: crate::core::production_map::ProductionMapProgram {
                map_id: map.id.clone(),
                product_code: map.product_code.clone(),
                operations: Vec::new(),
            },
            map: map.clone(),
        };
        assert!(training_map_has_apparatus(
            &saved,
            "apparatus:test:laminatsiya-1"
        ));
        map.nodes[1].title = "Renamed display only".to_string();
        let renamed = ProductionMapSaved { map, ..saved };
        assert!(training_map_has_apparatus(
            &renamed,
            "apparatus:test:laminatsiya-1"
        ));
        assert!(!training_map_has_apparatus(
            &renamed,
            "Renamed display only"
        ));
    }

    #[test]
    fn training_virtual_input_cannot_be_a_production_apparatus_id() {
        assert!(ApparatusId::new(TRAINING_VIRTUAL_INPUT_BOSMA).is_err());
        assert!(ApparatusId::new(TRAINING_VIRTUAL_INPUT_LAMINATSIYA).is_err());
        let batch = training_input_progress_batch(
            &training_worker_map(laminatsiya_training_map()),
            "apparatus:test:laminatsiya-1",
            &training_input_identity(),
        )
        .expect("training virtual input batch");
        assert_eq!(batch.apparatus, "apparatus:test:laminatsiya-1");
        assert_eq!(
            batch.payload_json["training_virtual_apparatus"],
            TRAINING_VIRTUAL_INPUT_BOSMA
        );
    }

    #[test]
    fn unsupported_training_map_does_not_get_virtual_input() {
        let mut map = laminatsiya_training_map();
        map.nodes[1].title = "Bosma aparat".to_string();
        map.nodes[1].role_code = "other".to_string();

        let worker_map = training_worker_map(map.clone());
        assert!(!worker_map.nodes.iter().any(is_training_input_node));
        assert!(
            training_input_progress_batch(
                &worker_map,
                "apparatus:test:unsupported-1",
                &training_input_identity(),
            )
            .is_none()
        );
    }

    #[test]
    fn training_output_uses_meter_when_progress_quantity_is_missing() {
        let input = TrainingQueuePrintInput {
            gross_qty: Some(250.0),
            finished_goods_kg: Some(250.0),
            finished_goods_meter: Some(6.0),
            bobina_kg: Some(2.0),
            uom: "m".to_string(),
            ..TrainingQueuePrintInput::default()
        };
        let batches = training_progress_batches(
            &laminatsiya_training_map(),
            "apparatus:test:laminatsiya-1",
            "training-laminatsiya-1",
            queue_state::ApparatusQueueAction::Complete,
            &principal(),
            &input,
            None,
            None,
            "",
        )
        .expect("training output batches");

        let batch = batches.first().expect("training output batch");
        assert_eq!(batch.produced_qty, 6.0);
        assert_eq!(batch.finished_goods_kg, Some(250.0));
        let print_request = training_progress_print_request(batch, &input, "Training apparat");
        assert_eq!(print_request.progress_qty, 6.0);
        assert_eq!(print_request.gross_qty, 250.0);
    }

    #[test]
    fn rezka_training_output_matches_production_frame_fan_out() {
        let mut map = rezka_training_map();
        map.nodes[1].rezka_kadr_count = Some(4);
        map.nodes[1].rezka_label_length = Some(250.0);
        let input = TrainingQueuePrintInput {
            progress_qty: Some(100.0),
            gross_qty: Some(104.0),
            finished_goods_kg: Some(100.0),
            finished_goods_meter: Some(900.0),
            bobina_kg: Some(4.0),
            rezka_bosma_waste: Some(1.0),
            rezka_lamination_waste: Some(2.0),
            rezka_edge_waste: Some(3.0),
            total_waste: Some(6.0),
            diameter: Some(42.0),
            uom: "kg".to_string(),
            ..TrainingQueuePrintInput::default()
        };

        let batches = training_progress_batches(
            &map,
            "apparatus:test:rezka-1",
            "training-rezka-1",
            queue_state::ApparatusQueueAction::Complete,
            &principal(),
            &input,
            None,
            None,
            "input-batch-1",
        )
        .expect("rezka training output batches");

        assert_eq!(batches.len(), 4);
        let batch_ids = batches
            .iter()
            .map(|batch| batch.batch_id.as_str())
            .collect::<BTreeSet<_>>();
        let qr_payloads = batches
            .iter()
            .map(|batch| batch.qr_payload.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(batch_ids.len(), 4);
        assert_eq!(qr_payloads.len(), 4);
        for (index, batch) in batches.iter().enumerate() {
            assert!(batch.batch_id.ends_with(&format!(":frame:{}", index + 1)));
            assert_eq!(progress_qr_payload(&batch.batch_id), batch.qr_payload);
            assert_eq!(batch.parent_batch_id, "input-batch-1");
            assert_eq!(batch.payload_json["rezka_frame_index"], index + 1);
            assert_eq!(batch.payload_json["rezka_frame_count"], 4);
            assert_eq!(batch.payload_json["rezka_label_length"], 250.0);
            assert_eq!(batch.finished_goods_kg, Some(100.0));
            assert_eq!(batch.finished_goods_meter, Some(900.0));
            assert_eq!(
                batch.payload_json["rezka_metrics_owner"],
                serde_json::json!(index == 0),
            );
            let print_request =
                training_progress_print_request(batch, &input, "Training apparat");
            assert_eq!(print_request.qr_payload, batch.qr_payload);
            assert_eq!(print_request.progress_qty, batch.produced_qty);
        }
        assert_eq!(batches[0].diameter, Some(42.0));
        assert_eq!(batches[0].total_waste, Some(6.0));
        assert_eq!(batches[0].bobina_kg, Some(4.0));
        assert_eq!(batches[1].diameter, None);
        assert_eq!(batches[1].total_waste, None);
        assert_eq!(batches[1].bobina_kg, None);
        assert_eq!(batches[1].rezka_bosma_waste, None);
        assert_eq!(batches[1].rezka_lamination_waste, None);
        assert_eq!(batches[1].rezka_edge_waste, None);

        for action in [
            queue_state::ApparatusQueueAction::Pause,
            queue_state::ApparatusQueueAction::DetachRoll,
            queue_state::ApparatusQueueAction::RollComplete,
        ] {
            let action_batches = training_progress_batches(
                &map,
                "apparatus:test:rezka-1",
                "training-rezka-1",
                action,
                &principal(),
                &input,
                None,
                None,
                "input-batch-1",
            )
            .expect("rezka action output batches");
            assert_eq!(action_batches.len(), 4);
            assert!(action_batches.iter().all(|batch| batch.action == action));
            assert!(action_batches.iter().all(|batch| {
                training_progress_print_request(batch, &input, "Training apparat").qr_payload
                    == batch.qr_payload
            }));
        }
    }

    #[test]
    fn intermediate_training_rezka_uses_configured_output_groups() {
        let mut map = rezka_training_map();
        map.nodes[1].rezka_kadr_count = Some(3);
        map.nodes[1].rezka_frame_groups = vec![1, 2];
        map.nodes.insert(
            2,
            node(
                "lamination-after",
                ProductionMapNodeKind::Apparatus,
                "Laminatsiya aparat",
            ),
        );
        map.edges = vec![
            ProductionMapEdge {
                from: "start".to_string(),
                to: "rezka".to_string(),
                branch: String::new(),
            },
            ProductionMapEdge {
                from: "rezka".to_string(),
                to: "lamination-after".to_string(),
                branch: String::new(),
            },
            ProductionMapEdge {
                from: "lamination-after".to_string(),
                to: "end".to_string(),
                branch: String::new(),
            },
        ];
        let input = TrainingQueuePrintInput {
            progress_qty: Some(100.0),
            gross_qty: Some(10.0),
            diameter: Some(42.0),
            total_waste: Some(1.0),
            ..TrainingQueuePrintInput::default()
        };

        let batches = training_progress_batches(
            &map,
            "apparatus:test:rezka-1",
            "training-rezka-1",
            queue_state::ApparatusQueueAction::Complete,
            &principal(),
            &input,
            None,
            None,
            "input-batch-1",
        )
        .expect("grouped training Rezka outputs");

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].payload_json["contained_kadr_count"], 1);
        assert_eq!(batches[1].payload_json["contained_kadr_count"], 2);
        assert!(batches.iter().all(|batch| {
            batch.payload_json["rezka_output_kind"] == "grouped_roll"
                && batch.payload_json["rezka_frame_count"] == 2
        }));
    }

    #[test]
    fn rezka_training_output_requires_kadr_count_before_state_change() {
        let error = training_progress_batches(
            &rezka_training_map(),
            "apparatus:test:rezka-1",
            "training-rezka-1",
            queue_state::ApparatusQueueAction::DetachRoll,
            &principal(),
            &TrainingQueuePrintInput::default(),
            None,
            None,
            "",
        )
        .expect_err("missing rezka kadr count");

        assert!(matches!(
            error,
            TrainingWorkspaceError::InvalidInput(code)
                if code == "rezka_kadr_count_required"
        ));
    }
}
