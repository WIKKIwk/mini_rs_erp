use super::*;
use crate::core::formula::LayerInput;

fn source(materials: &[&str], assignments: &[&[&str]]) -> OrderEditSource {
    let template = CalculateOrderTemplate {
        layers: materials.iter().map(|name| LayerInput::new(*name, "20")).collect(),
        frame_product_size_mm: 300.0,
        frame_count: 2.0,
        edge_allowance_mm: 15.0,
        ..Default::default()
    };
    let identities = assignments.iter().map(|row| row.iter().map(|s| s.to_string()).collect()).collect::<Vec<_>>();
    let restrictions = OrderEditRestrictions::for_materials(&template, &identities);
    OrderEditSource {
        map: serde_json::from_value(serde_json::json!({"id":"order", "title":"Product", "product_code":"item"})).unwrap(),
        template,
        revision: 0,
        restrictions,
    }
}

#[test]
fn order_edit_material_identity_is_exact_or_strict_roll_dimensions() {
    for (inventory, family, expected) in [
        (" pe ", "PE", true),
        ("PET", "PE", false),
        ("BOPP metal", "BOPP", false),
        (" CPP  1000/35 ", "cpp", true),
        ("CPP 1000,5 / 35", "CPP", true),
        ("CPP unknown/35", "CPP", false),
        ("CPP 1000/35/40", "CPP", false),
        ("CPP 1000/35 extra", "CPP", false),
        ("CPP 1000", "CPP", false),
        ("CPP 1000/NaN", "CPP", false),
        ("CPP 1000/0", "CPP", false),
        ("", "", false),
    ] {
        assert_eq!(material_identity_matches(inventory, family), expected, "{inventory} / {family}");
    }
}

#[test]
fn order_edit_material_policy_locks_all_layers_of_attached_families() {
    let source = source(&["PET", "PE", "PET", "BOPP metal"], &[&["raw-123", "pet 615/20"]]);
    assert_eq!(source.restrictions.locked_layer_indices, vec![0, 2]);
    assert!(source.restrictions.dimensions_locked);
    assert!(source.restrictions.layer_structure_locked);
    assert!(!source.restrictions.unresolved_materials);
    let policy = OrderEditRestrictions::for_materials(&source.template, &[vec!["PET".into(), "PE".into()]]);
    assert_eq!(policy.locked_layer_indices, vec![0, 1, 2]);
    assert_eq!(OrderEditRestrictions::for_materials(&source.template, &[]), OrderEditRestrictions::default());
}

#[test]
fn order_edit_unknown_material_locks_every_layer_without_blocking_safe_edits() {
    let source = source(&["PET", "PE"], &[&["unknown-roll"]]);
    assert!(source.restrictions.unresolved_materials);
    assert_eq!(source.restrictions.locked_layer_indices, vec![0, 1]);
    let mut edited = source.template.clone();
    edited.kg = 600.0;
    edited.note = "Updated".into();
    assert!(validate_material_edit(&source, &edited).is_ok());
    edited.layers[1].micron = "30".into();
    assert!(validate_material_edit(&source, &edited).is_err());
}

#[test]
fn order_edit_unassigned_layer_can_change_but_linked_fields_cannot() {
    let source = source(&["PET", "PE", "CPP"], &[&["PET"]]);
    let mut edited = source.template.clone();
    edited.layers[1] = LayerInput::new("BOPP", "35");
    edited.kg = 600.0;
    edited.waste_percent = 7.0;
    assert!(validate_material_edit(&source, &edited).is_ok());
    let allowed = edited.clone();
    for field in ["material", "material_id", "micron", "remove", "add", "move", "width", "count", "edge"] {
        let mut edited = allowed.clone();
        match field {
            "material" => edited.layers[0].material = "CPP".into(),
            "material_id" => edited.layers[0].material_id = "another-id".into(),
            "micron" => edited.layers[0].micron = "35".into(),
            "remove" => { edited.layers.pop(); },
            "add" => edited.layers.push(LayerInput::new("PE", "10")),
            "move" => edited.layers.swap(0, 2),
            "width" => edited.frame_product_size_mm += 10.0,
            "count" => edited.frame_count += 1.0,
            "edge" => edited.edge_allowance_mm += 1.0,
            _ => unreachable!(),
        }
        assert!(validate_material_edit(&source, &edited).is_err(), "{field}");
    }
}

#[test]
fn order_edit_material_ids_and_legacy_layer_fields_are_protected() {
    let mut source = source(&["Custom film", "PE"], &[&["material-123"]]);
    source.template.layers[0].material_id = "material-123".into();
    source.restrictions = OrderEditRestrictions::for_materials(&source.template, &[vec!["material-123".into()]]);
    assert_eq!(source.restrictions.locked_layer_indices, vec![0]);
    let mut edited = source.template.clone();
    edited.first_layer_material = "CPP".into();
    edited.first_layer_micron = "35".into();
    edited.layers.clear();
    assert!(validate_material_edit(&source, &edited).is_err());
}
