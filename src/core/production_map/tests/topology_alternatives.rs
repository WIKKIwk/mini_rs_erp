use super::*;

fn map(order: &str) -> ProductionMapDefinition {
    serde_json::from_value(serde_json::json!({
        "id": order, "product_code": "P", "title": "Parallel lamination",
        "nodes": [
            {"id":"start", "kind":"start", "title":"Start"},
            {"id":"print", "kind":"apparatus", "title":"Print", "apparatus_id":FLOW_PECHAT_ID},
            {"id":"lam1", "kind":"apparatus", "title":"Laminatsiya 1", "apparatus_id":LAMINATION_1_ID},
            {"id":"lam2", "kind":"apparatus", "title":"Laminatsiya 2", "apparatus_id":LAMINATION_2_ID},
            {"id":"cut", "kind":"apparatus", "title":"Rezka", "apparatus_id":REZKA_ID, "rezka_kadr_count":1},
            {"id":"end", "kind":"end", "title":"End"}
        ],
        "edges": [
            {"from":"start", "to":"print"},
            {"from":"print", "to":"lam2"},
            {"from":"print", "to":"lam1"},
            {"from":"lam2", "to":"cut"},
            {"from":"lam1", "to":"cut"},
            {"from":"cut", "to":"end"}
        ]
    })).unwrap()
}

fn assert_grouped(map: &ProductionMapDefinition) {
    let left = map.nodes.iter().find(|n| n.id == "lam1").unwrap();
    let right = map.nodes.iter().find(|n| n.id == "lam2").unwrap();
    assert!(
        !left.alternative_group_id.is_empty(),
        "same-stage matching apparatuses need a group"
    );
    assert_eq!(left.alternative_group_id, right.alternative_group_id);
    assert!(chain::stage_node_ids_match_for_map(map, "lam1", "lam2"));
}

#[tokio::test]
async fn topology_alternatives_exact_0073_qr_validates_without_claiming_then_starts_once() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    // Same graph identities and WIP destination as the reported 9900 m roll.
    let mut definition = map("zakaz-0073");
    for (old, new) in [
        ("print", "apparatus_1"),
        ("lam2", "apparatus_2"),
        ("lam1", "apparatus_2_alt_lam1"),
        ("cut", "rezka_3"),
    ] {
        for node in &mut definition.nodes {
            if node.id == old {
                node.id = new.into();
            }
        }
        for edge in &mut definition.edges {
            if edge.from == old {
                edge.from = new.into();
            }
            if edge.to == old {
                edge.to = new.into();
            }
        }
    }
    definition.nodes[1].apparatus_id = PECHAT_9_ID.into();
    for node in &mut definition.nodes {
        if [LAMINATION_1_ID, LAMINATION_2_ID].contains(&node.apparatus_id.as_str()) {
            node.alternative_group_id = "topology_alt:apparatus_2".into();
        }
    }
    store.put_map(definition.clone()).await.unwrap();
    let original: OrderProgressBatch = serde_json::from_value(serde_json::json!({
        "batch_id":"reported-roll-0073", "session_id":"print-session",
        "started_at_unix":1789514707, "completed_at_unix":1789514707,
        "apparatus":PECHAT_9_ID, "order_id":"zakaz-0073", "action":"detach_roll",
        "status":"roll_detached", "produced_qty":9900, "uom":"m",
        "qr_payload":"400118D5A225166D31898C1F", "label_item_code":"P",
        "label_item_name":"Reported roll", "executor_name":"Worker",
        "worker_role":"aparatchi", "worker_ref":"test-worker", "worker_display_name":"Worker",
        "wip_status":"waiting", "current_apparatus":PECHAT_9_ID,
        "next_apparatus":LAMINATION_2_ID,
        "payload_json":{"stage_node_id":"apparatus_1","next_stage_node_id":"apparatus_2"}
    }))
    .unwrap();
    store
        .put_order_progress_batch(original.clone())
        .await
        .unwrap();
    for machine in [LAMINATION_1_ID, LAMINATION_2_ID] {
        let validated = service
            .start_input_for_qr(machine, &definition.id, "", &original.qr_payload)
            .await
            .unwrap();
        assert_eq!(validated.batch_id, original.batch_id);
        assert_eq!(validated.wip_status, OrderProgressBatchWipStatus::Waiting);
    }
    assert_eq!(
        store
            .progress_batch(&original.batch_id)
            .await
            .unwrap()
            .unwrap(),
        original
    );
    for case in [
        "claimed",
        "processed",
        "wrong_order",
        "wrong_stage",
        "wrong_source",
        "wrong_qr",
    ] {
        let mut batch = original.clone();
        match case {
            "claimed" => batch.wip_status = OrderProgressBatchWipStatus::InUse,
            "processed" => {
                batch.wip_status = OrderProgressBatchWipStatus::Processed;
                batch.processed_by_apparatus = LAMINATION_2_ID.into();
            }
            "wrong_order" => batch.order_id = "another-order".into(),
            "wrong_stage" => {
                batch.payload_json["next_stage_node_id"] = serde_json::json!("rezka_3")
            }
            "wrong_source" => batch.payload_json["stage_node_id"] = serde_json::json!("rezka_3"),
            "wrong_qr" => batch.qr_payload = "another-qr".into(),
            _ => unreachable!(),
        }
        store.put_order_progress_batch(batch).await.unwrap();
        assert!(
            service
                .start_input_for_qr(
                    LAMINATION_1_ID,
                    &definition.id,
                    &original.batch_id,
                    &original.qr_payload
                )
                .await
                .is_err(),
            "{case} must stay rejected"
        );
    }
    store
        .put_order_progress_batch(original.clone())
        .await
        .unwrap();
    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "lam1-worker".into(),
        display_name: "Worker".into(),
    };
    service
        .apply_apparatus_queue_action_with_progress(
            LAMINATION_1_ID,
            &definition.id,
            queue_state::ApparatusQueueAction::Start,
            &[LAMINATION_1_ID.into()],
            actor,
            QueueProgressInput {
                qr_payload: original.qr_payload.clone(),
                ..Default::default()
            },
        )
        .await
        .expect("Lam1 must start the actual migrated topology");
    let claimed = store
        .progress_batch(&original.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.used_by_apparatus, LAMINATION_1_ID);
    assert_eq!(claimed.wip_status, OrderProgressBatchWipStatus::InUse);
    assert!(
        service
            .start_input_for_qr(LAMINATION_2_ID, &definition.id, "", &original.qr_payload)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn topology_alternatives_preserves_distinct_work_and_explicit_groups() {
    let service = default_service_with_store(Arc::new(MemoryProductionMapStore::new())).await;
    for case in [
        "technology",
        "sequential",
        "different_input",
        "different_output",
        "quantity",
        "same_machine",
        "explicit",
        "conditional",
    ] {
        let mut input = map("zakaz-topology-negative");
        match case {
            "technology" => input.nodes[3].apparatus_id = "apparatus:default:asset-004".into(),
            "sequential" => {
                input.edges.retain(|e| {
                    !(e.from == "print" && e.to == "lam2") && !(e.from == "lam1" && e.to == "cut")
                });
                input.edges.push(ProductionMapEdge {
                    from: "lam1".into(),
                    to: "lam2".into(),
                    branch: "".into(),
                });
            }
            "different_input" => {
                input
                    .edges
                    .iter_mut()
                    .find(|e| e.to == "lam1")
                    .unwrap()
                    .from = "start".into()
            }
            "different_output" => {
                input
                    .edges
                    .iter_mut()
                    .find(|e| e.from == "lam1")
                    .unwrap()
                    .to = "end".into()
            }
            "quantity" => input.nodes[3].qty_formula = "2".into(),
            "same_machine" => input.nodes[3].apparatus_id = LAMINATION_1_ID.into(),
            "explicit" => {
                input.nodes[2].alternative_group_id = "separate-left".into();
                input.nodes[3].alternative_group_id = "separate-right".into();
            }
            "conditional" => {
                input.nodes[1].kind = ProductionMapNodeKind::Condition;
                input.nodes[1].apparatus_id.clear();
                input.nodes[1].formula = Some(ProductionFormula {
                    target: "".into(),
                    expression: "1 > 0".into(),
                });
                for edge in input.edges.iter_mut().filter(|e| e.from == "print") {
                    edge.branch = if edge.to == "lam1" { "true" } else { "false" }.into();
                }
            }
            _ => unreachable!(),
        }
        let before = input.clone();
        let saved = service
            .prepare_map_for_save(input)
            .await
            .unwrap_or_else(|e| panic!("{case}: {e:?}"));
        assert_eq!(saved.map, before, "{case}: do not merge separate work");
    }
}

#[tokio::test]
async fn topology_alternatives_uses_canonical_class_and_parameters_not_names() {
    let standard = crate::core::apparatus_standard::test_support::standard_runtime_configurations();
    for case in ["matching", "class", "width", "capability", "rename"] {
        let mut configurations = standard.clone();
        let second = configurations
            .iter_mut()
            .find(|a| a.runtime.apparatus_id.as_str() == LAMINATION_2_ID)
            .unwrap();
        match case {
            "class" => {
                second.runtime.equipment_class_id =
                    crate::core::apparatus_standard::EquipmentClassId::new(
                        "equipment-class:test:different",
                    )
                    .unwrap()
            }
            "width" => second.runtime.execution_profile.max_web_width_mm = Some(900),
            "capability" => {
                second.runtime.capabilities.insert(
                    crate::core::apparatus_standard::EquipmentCapabilityCode::Laminate,
                    2,
                );
            }
            "rename" => {
                second.runtime.display.display_name = "Completely different display name".into()
            }
            _ => {}
        }
        let resolver = Arc::new(TestCanonicalApparatusResolver::new(configurations));
        let service =
            ProductionMapService::new(Arc::new(MemoryProductionMapStore::new()), resolver);
        let mut input = map("zakaz-topology-profile");
        input.nodes[1].apparatus_id = "apparatus:default:flexo_pechat".into();
        let saved = service
            .prepare_map_for_save(input)
            .await
            .unwrap_or_else(|error| panic!("{case}: {error:?}"));
        if matches!(case, "matching" | "rename") {
            assert_grouped(&saved.map);
        } else {
            assert!(
                saved
                    .map
                    .nodes
                    .iter()
                    .all(|n| n.alternative_group_id.is_empty()),
                "{case}"
            );
        }
    }
}

#[tokio::test]
async fn topology_alternatives_save_groups_matching_siblings_and_is_stable() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let original = map("zakaz-topology");
    let saved = service.upsert_map(original.clone()).await.unwrap();
    assert_grouped(&saved.map);
    assert_eq!(saved.map.edges, original.edges);
    assert_eq!(
        store.map_by_id(&original.id).await.unwrap().unwrap(),
        saved.map
    );
    assert_eq!(service.upsert_map(saved.map.clone()).await.unwrap(), saved);
    let mut reordered = original;
    reordered.nodes.reverse();
    reordered.edges.reverse();
    let reversed = service.prepare_map_for_save(reordered).await.unwrap();
    for node in saved.map.nodes.iter().filter(|n| n.id.starts_with("lam")) {
        assert_eq!(
            node.alternative_group_id,
            reversed
                .map
                .nodes
                .iter()
                .find(|n| n.id == node.id)
                .unwrap()
                .alternative_group_id
        );
    }
    let batch = service
        .upsert_maps_batch(vec![map("zakaz-topology-batch")])
        .await
        .unwrap();
    assert_grouped(&batch[0].map);
    assert_grouped(
        &service
            .map("zakaz-topology-batch")
            .await
            .unwrap()
            .unwrap()
            .map,
    );
}

#[tokio::test]
async fn topology_alternatives_does_not_rewrite_an_already_started_stage() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let original = map("zakaz-topology-locked");
    store.put_map(original.clone()).await.unwrap();
    store
        .put_apparatus_queue_states(
            LAMINATION_2_ID,
            BTreeMap::from([(original.id.clone(), "in_progress".into())]),
        )
        .await
        .unwrap();
    assert!(matches!(
        service.upsert_map(original.clone()).await,
        Err(ProductionMapError::StartedProductionMapStageLocked)
    ));
    assert_eq!(
        store.map_by_id(&original.id).await.unwrap().unwrap(),
        original
    );
}

#[tokio::test]
async fn topology_alternatives_resave_accepts_existing_peer_targeted_qr_once() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let order = "zakaz-topology-existing";
    // Reproduce 0141: old map has both arrows but no group; the producer's
    // first successor is lam2, so its detached roll is persisted for lam2.
    let original = map(order);
    store.put_map(original.clone()).await.unwrap();
    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "topology-worker".into(),
        display_name: "Worker".into(),
    };
    start_first_stage(&service, order, FLOW_PECHAT_ID, actor.clone())
        .await
        .unwrap();
    let batch = service
        .apply_apparatus_queue_action_with_progress(
            FLOW_PECHAT_ID,
            order,
            queue_state::ApparatusQueueAction::DetachRoll,
            &[FLOW_PECHAT_ID.into()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(9810.0),
                uom: "m".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .progress_batch
        .unwrap();
    assert_eq!(batch.next_apparatus, LAMINATION_2_ID);
    assert_eq!(batch.payload_json["next_stage_node_id"], "lam2");
    assert_eq!(
        service.queue_action_controls().await.unwrap()[LAMINATION_1_ID][order]
            .interaction
            .blocking_reason_code,
        "waiting_previous_stage"
    );

    // Saving the same topology repairs the missing metadata, including when
    // printing has started; the locked producer and QR history stay intact.
    let saved = service.upsert_map(original).await.unwrap();
    assert_grouped(&saved.map);
    let snapshot = service.live_snapshot_shared().await.unwrap();
    for machine in [LAMINATION_1_ID, LAMINATION_2_ID] {
        let control = &snapshot.queue_action_controls[machine][order];
        assert_eq!(
            control.interaction.previous_wip_mode,
            ApparatusQueuePreviousWipMode::ScanRequired
        );
        assert!(
            control
                .allowed_actions
                .contains(&queue_state::ApparatusQueueAction::Start)
        );
        let inputs = service
            .wip_progress_batches(WipProgressBatchQuery::new(
                FLOW_PECHAT_ID,
                machine,
                "",
                Some(OrderProgressBatchWipStatus::Waiting),
                false,
                order,
                250,
            ))
            .await
            .unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].qr_payload, batch.qr_payload);
    }
    assert_eq!(
        store
            .progress_batch(&batch.batch_id)
            .await
            .unwrap()
            .unwrap(),
        batch
    );
    service
        .apply_apparatus_queue_action_with_progress(
            LAMINATION_1_ID,
            order,
            queue_state::ApparatusQueueAction::Start,
            &[LAMINATION_1_ID.into()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: batch.qr_payload.clone(),
                ..Default::default()
            },
        )
        .await
        .expect("Lam1 can claim a matching sibling's QR");
    assert!(
        service
            .apply_apparatus_queue_action_with_progress(
                LAMINATION_2_ID,
                order,
                queue_state::ApparatusQueueAction::Start,
                &[LAMINATION_2_ID.into()],
                actor,
                QueueProgressInput {
                    qr_payload: batch.qr_payload,
                    ..Default::default()
                },
            )
            .await
            .is_err(),
        "one roll cannot be consumed by both machines"
    );
}
