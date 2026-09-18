use super::*;

const ORDER_ID: &str = "zakaz-material-alternatives";

fn print_alternatives_map(assigned: &str) -> ProductionMapDefinition {
    let mut map = canonical_apparatus_stage_map(ORDER_ID, PECHAT_7_ID, "Bosma 7");
    let template = map.nodes.remove(1);
    map.edges.clear();
    for (index, apparatus) in [PECHAT_7_ID, PECHAT_8_ID, PECHAT_9_ID].iter().enumerate() {
        let mut node = template.clone();
        node.id = format!("print_{index}");
        node.apparatus_id = apparatus.to_string();
        node.alternative_group_id = "auto_print".into();
        node.alternative_assigned_apparatus_id = assigned.into();
        map.edges.extend([
            ProductionMapEdge {
                from: "start".into(),
                to: node.id.clone(),
                branch: String::new(),
            },
            ProductionMapEdge {
                from: node.id.clone(),
                to: "end".into(),
                branch: String::new(),
            },
        ]);
        map.nodes.insert(index + 1, node);
    }
    map
}

async fn material_service(map: ProductionMapDefinition, allowed: &[&str]) -> ProductionMapService {
    let (service, apparatus) = service_with_apparatus(&[
        (PECHAT_7_ID, "Bosma 7"),
        (PECHAT_8_ID, "Bosma 8"),
        (PECHAT_9_ID, "Bosma 9"),
    ])
    .await;
    for id in allowed {
        set_test_material_rule(
            &apparatus,
            ApparatusMaterialRuleUpsert {
                apparatus: id.to_string(),
                requires_material: false,
                start_policy: RawMaterialStartPolicy::StateAll,
                item_groups: vec!["Rulon".into()],
                requirement_groups: Vec::new(),
            },
        )
        .await
        .expect("optional material rule");
    }
    service.upsert_map(map).await.expect("map");
    service
}

fn assignment(apparatus: &str) -> RawMaterialAssignmentInput {
    RawMaterialAssignmentInput {
        order_id: ORDER_ID.into(),
        barcode: "ROLL-ALTERNATIVE".into(),
        item_code: "BOPP".into(),
        item_group: "Rulon".into(),
        apparatus: apparatus.into(),
        ..Default::default()
    }
}

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "admin".into(),
        ref_: "admin".into(),
        display_name: "Admin".into(),
    }
}

#[tokio::test]
async fn assigned_print_is_the_only_material_option_and_rejects_other_candidates() {
    let service = material_service(
        print_alternatives_map(PECHAT_8_ID),
        &[PECHAT_7_ID, PECHAT_8_ID, PECHAT_9_ID],
    )
    .await;
    assert_eq!(
        service
            .raw_material_assignment_apparatus_options(ORDER_ID, &["Rulon".into()])
            .await
            .unwrap(),
        vec![PECHAT_8_ID]
    );
    for unselected in [PECHAT_7_ID, PECHAT_9_ID] {
        assert_eq!(
            service
                .assign_raw_material_to_order(assignment(unselected), &actor())
                .await,
            Err(ProductionMapError::RawMaterialGroupNotAllowed)
        );
    }
    let assigned = service
        .assign_raw_material_to_order(assignment(""), &actor())
        .await
        .expect("infer the order's selected apparatus");
    assert_eq!(assigned.apparatus_id.as_str(), PECHAT_8_ID);
}

#[tokio::test]
async fn missing_selected_material_rule_does_not_fall_back_to_other_candidates() {
    let service = material_service(
        print_alternatives_map(PECHAT_8_ID),
        &[PECHAT_7_ID, PECHAT_9_ID],
    )
    .await;
    assert!(
        service
            .raw_material_assignment_apparatus_options(ORDER_ID, &["Rulon".into()])
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        service
            .assign_raw_material_to_order(assignment(""), &actor())
            .await,
        Err(ProductionMapError::RawMaterialGroupNotAllowed)
    );
}

#[tokio::test]
async fn unassigned_group_keeps_material_choices() {
    let service = material_service(
        print_alternatives_map(""),
        &[PECHAT_7_ID, PECHAT_8_ID, PECHAT_9_ID],
    )
    .await;
    assert_eq!(
        service
            .assign_raw_material_to_order(assignment(""), &actor())
            .await,
        Err(ProductionMapError::RawMaterialGroupAmbiguous(vec![
            PECHAT_7_ID.into(),
            PECHAT_8_ID.into(),
            PECHAT_9_ID.into(),
        ]))
    );
    let assigned = service
        .assign_raw_material_to_order(assignment(PECHAT_9_ID), &actor())
        .await
        .expect("explicit choice remains available");
    assert_eq!(assigned.apparatus_id.as_str(), PECHAT_9_ID);
}

#[tokio::test]
async fn group_selection_on_one_node_preserves_an_independent_stage_of_the_same_apparatus() {
    let mut map = print_alternatives_map(PECHAT_8_ID);
    for node in &mut map.nodes {
        if node.apparatus_id != PECHAT_8_ID {
            node.alternative_assigned_apparatus_id.clear();
        }
    }
    let mut downstream = map.nodes[1].clone();
    downstream.id = "downstream".into();
    downstream.alternative_group_id.clear();
    for edge in &mut map.edges {
        if edge.to == "end" {
            edge.to = downstream.id.clone();
        }
    }
    map.edges.push(ProductionMapEdge {
        from: downstream.id.clone(),
        to: "end".into(),
        branch: String::new(),
    });
    map.nodes.insert(4, downstream);
    let service = material_service(map, &[PECHAT_7_ID, PECHAT_8_ID, PECHAT_9_ID]).await;
    assert_eq!(
        service
            .raw_material_assignment_apparatus_options(ORDER_ID, &["Rulon".into()])
            .await
            .unwrap(),
        vec![PECHAT_7_ID, PECHAT_8_ID]
    );
}
