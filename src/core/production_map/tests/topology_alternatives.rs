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
