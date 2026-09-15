use super::*;
use crate::core::apparatus_standard::test_support::{TestApparatusSpec, runtime_configuration};
use crate::core::production_map::{
    MemoryProductionMapStore, ProductionMapService, TestCanonicalApparatusResolver, chain,
};
use std::sync::Arc;

fn template(method: PrintMethod, layers: usize, width: f64, frames: f64) -> CalculateOrderTemplate {
    CalculateOrderTemplate {
        name: "Auto order".into(),
        product: "Auto product".into(),
        item_code: "item-1".into(),
        customer_ref: "customer-1".into(),
        customer: "Customer".into(),
        order_number: "9011".into(),
        status: "rulon".into(),
        kg: 500.0,
        frame_product_size_mm: width,
        frame_count: frames,
        roll_count: Some(6),
        waste_percent: 5.0,
        layers: (0..layers)
            .map(|_| crate::core::formula::LayerInput::new("pet", "12"))
            .collect(),
        production_options: Some(OrderProductionOptions {
            print_method: method,
            cold_glue: false,
            diameter_mm: Some(45.5),
        }),
        ..Default::default()
    }
}

fn catalog(lamination_max: u32) -> Vec<Apparatus> {
    let mut flexo = TestApparatusSpec::print(
        "apparatus:test:flexo",
        "Renamed flexo",
        Technology::Flexographic,
        Some(8),
    );
    flexo.max_web_width_mm = Some(1500);
    let mut lam = TestApparatusSpec::laminate("apparatus:test:lam", "Renamed laminator");
    lam.max_web_width_mm = Some(lamination_max);
    let mut all = vec![
        runtime_configuration(flexo),
        runtime_configuration(lam),
        runtime_configuration(TestApparatusSpec::cut("apparatus:test:cut", "Cut")),
        runtime_configuration(TestApparatusSpec::package(
            "apparatus:test:package",
            "Package",
        )),
        runtime_configuration(TestApparatusSpec::operation(
            "apparatus:test:glue",
            "Glue",
            Operation::Glue,
            Technology::ColdGlue,
        )),
    ];
    for color in 7..=9 {
        all.push(runtime_configuration(TestApparatusSpec::print(
            &format!("apparatus:test:print-{color}"),
            "Renamed metal",
            Technology::Rotogravure,
            Some(color),
        )));
    }
    all
}

#[test]
fn single_layer_routes_only_to_selected_print_technology_then_final_rezka() {
    for method in [PrintMethod::Flexo, PrintMethod::Metal] {
        let map = generate(&template(method, 1, 300.0, 2.0), &catalog(1000), &[]).unwrap();
        assert!(!map.nodes.iter().any(|n| n.id.starts_with("laminate")));
        let print: Vec<_> = map
            .nodes
            .iter()
            .filter(|n| n.id.starts_with("print_"))
            .collect();
        if method == PrintMethod::Flexo {
            assert_eq!(print.len(), 1);
            assert_eq!(print[0].apparatus_id, "apparatus:test:flexo");
        } else {
            assert_eq!(print.len(), 2);
            assert!(print.iter().all(|n| n.alternative_group_id == "auto_print"));
            assert!(print.iter().all(|n| !n.apparatus_id.contains("flexo")));
        }
        assert!(
            map.edges
                .iter()
                .any(|e| e.from == "final_cut_0" && e.to == "end")
        );
    }
}

#[tokio::test]
async fn oversized_flexo_is_split_two_plus_one_and_validates_downstream_width() {
    let machines = catalog(1000);
    let t = template(PrintMethod::Flexo, 2, 400.0, 3.0);
    let map = generate(&t, &machines, &[]).unwrap();
    let cut = map
        .nodes
        .iter()
        .find(|n| n.id == "pre_lamination_cut_0")
        .unwrap();
    assert_eq!(cut.rezka_frame_groups, [2, 1]);
    assert_eq!(cut.rezka_kadr_count, Some(3));
    assert_eq!(map.width_mm, Some(1200.0));
    assert!(map.base_length.unwrap() > 0.0);
    let cut_outputs =
        crate::core::production_map::service_progress_support::rezka_output_kadr_counts;
    assert_eq!(
        cut_outputs(&map, "apparatus:test:cut", "pre_lamination_cut_0", None).unwrap(),
        [2, 1]
    );
    assert_eq!(
        cut_outputs(&map, "apparatus:test:cut", "final_cut_0", Some(2)).unwrap(),
        [1, 1]
    );
    assert_eq!(
        cut_outputs(&map, "apparatus:test:cut", "final_cut_0", Some(1)).unwrap(),
        [1]
    );
    let before = chain::previous_work_stages_for_node(&map, "laminate_0");
    assert_eq!(before[0].node_id, "pre_lamination_cut_0");
    let service = ProductionMapService::new(
        Arc::new(MemoryProductionMapStore::new()),
        Arc::new(TestCanonicalApparatusResolver::new(machines)),
    );
    service.prepare_map_for_save(map.clone()).await.unwrap();
    // An unrelated cut must never make an oversized direct lamination route valid.
    let mut bypass = map;
    bypass.edges.push(ProductionMapEdge {
        from: "print_0".into(),
        to: "laminate_0".into(),
        branch: String::new(),
    });
    assert!(service.prepare_map_for_save(bypass).await.is_err());
}

#[test]
fn compatible_extruder_avoids_cut_and_incompatible_alternatives_are_excluded() {
    let mut machines = catalog(1050);
    machines.push(runtime_configuration(TestApparatusSpec::operation(
        "apparatus:test:extruder",
        "Extruder",
        Operation::Laminate,
        Technology::ExtrusionLamination,
    )));
    let map = generate(&template(PrintMethod::Flexo, 2, 400.0, 3.0), &machines, &[]).unwrap();
    assert!(
        !map.nodes
            .iter()
            .any(|n| n.id.starts_with("pre_lamination_cut"))
    );
    let lam: Vec<_> = map
        .nodes
        .iter()
        .filter(|n| n.id.starts_with("laminate_"))
        .collect();
    assert_eq!(lam.len(), 1);
    assert_eq!(lam[0].apparatus_id, "apparatus:test:extruder");
}

#[test]
fn minimum_width_is_respected_without_extra_strips() {
    let mut a = catalog(1000).remove(1);
    a.runtime.execution_profile.min_web_width_mm = Some(500);
    assert_eq!(best_frame_groups(1200.0, 4, &[&a]).unwrap(), [2, 2]);
    assert!(best_frame_groups(1200.0, 1, &[&a]).is_none());
}

#[test]
fn cold_glue_follows_lamination_and_package_precedes_terminal_cut() {
    let mut t = template(PrintMethod::Metal, 2, 300.0, 2.0);
    t.status = "paket".into();
    t.production_options.as_mut().unwrap().cold_glue = true;
    let map = generate(&t, &catalog(1000), &[]).unwrap();
    for (from, to) in [
        ("laminate_0", "cold_glue_0"),
        ("cold_glue_0", "package_0"),
        ("package_0", "final_cut_0"),
    ] {
        assert!(map.edges.iter().any(|e| e.from == from && e.to == to));
    }
}

#[test]
fn limits_missing_choices_and_missing_required_apparatus_do_not_create_empty_maps() {
    let mut t = template(PrintMethod::Flexo, 1, 300.0, 2.0);
    t.roll_count = Some(9);
    assert!(generate(&t, &catalog(1000), &[]).is_err());
    t.roll_count = Some(6);
    t.production_options = None;
    assert!(generate(&t, &catalog(1000), &[]).is_err());
    let t = template(PrintMethod::Metal, 1, 1400.0, 1.0);
    assert!(generate(&t, &catalog(1000), &[]).is_err());
    let mut machines = catalog(1000);
    machines.retain(|a| a.runtime.execution_profile.operation != Operation::Cut);
    assert!(generate(&template(PrintMethod::Metal, 1, 300.0, 2.0), &machines, &[]).is_err());
}
