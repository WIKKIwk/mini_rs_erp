use super::*;

fn valid_template() -> CalculateOrderTemplate {
    CalculateOrderTemplate {
        name: "Bir qavatli zakaz".to_string(),
        product: "Mahsulot".to_string(),
        frame_product_size_mm: 100.0,
        frame_count: 1.0,
        first_layer_material: "pet".to_string(),
        first_layer_micron: "12".to_string(),
        ..CalculateOrderTemplate::default()
    }
}

#[test]
fn accepts_single_layer_template() {
    validate_template(&valid_template()).expect("single-layer template");
}

#[test]
fn print_val_size_preserves_frame_dimensions_and_legacy_defaults() {
    let template = hydrate_template_dimensions(CalculateOrderTemplate {
        frame_product_size_mm: 320.0,
        frame_count: 2.0,
        edge_allowance_mm: 15.0,
        print_val_size_mm: Some(850.0),
        ..valid_template()
    });
    validate_template(&template).expect("valid val override");
    let mut json = serde_json::to_value(template).expect("serialize");
    let restored: CalculateOrderTemplate =
        serde_json::from_value(json.clone()).expect("decode");
    assert_eq!(restored.print_val_size_mm, Some(850.0));
    assert_eq!(restored.width_mm, 655.0);
    assert_eq!(restored.frame_product_size_mm, 320.0);
    assert_eq!(restored.frame_count, 2.0);
    json.as_object_mut().unwrap().remove("print_val_size_mm");
    let legacy: CalculateOrderTemplate = serde_json::from_value(json).expect("legacy decode");
    assert_eq!(legacy.print_val_size_mm, None);
    assert_eq!(legacy.width_mm, 655.0);
}

#[test]
fn print_val_size_must_be_finite_and_positive() {
    for size in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let template = CalculateOrderTemplate {
            print_val_size_mm: Some(size),
            ..valid_template()
        };
        assert!(
            validate_template(&template).is_err(),
            "invalid size: {size}"
        );
    }
}

#[test]
fn rejects_partially_filled_optional_layer() {
    let mut template = valid_template();
    template.second_layer_material = "pe oq".to_string();

    let error = validate_template(&template).expect_err("incomplete second layer");

    assert_eq!(
        error.to_string(),
        "invalid input: 2-qavat materiali va mikroni birga kiritilishi kerak"
    );
}

#[test]
fn accepts_template_with_arbitrary_layer_count() {
    let mut template = valid_template();
    template.layers = (1..=8)
        .map(|number| LayerInput::new(format!("pet{number}"), "12"))
        .collect();

    validate_template(&template).expect("arbitrary layer template");
}
