use super::*;
#[test]
fn preparation_material_responsibility_keys_are_trimmed_and_bounded() {
    let ok = MaterialResponsibilityAssign {
        principal_ref: "  prep-1 ".into(),
        material_id: " builtin-pet ".into(),
    };
    assert_eq!(ok.principal_key().unwrap(), "prep-1");
    assert_eq!(ok.material_key().unwrap(), "builtin-pet");
    let blank = MaterialResponsibilityAssign {
        principal_ref: "   ".into(),
        material_id: "builtin-pet".into(),
    };
    assert!(blank.principal_key().is_err());
    let long = MaterialResponsibilityDelete {
        principal_ref: "prep-1".into(),
        material_id: "x".repeat(129),
    };
    assert!(long.material_key().is_err());
}

#[test]
fn preparation_decimal_and_rounding_are_exact() {
    assert_eq!(
        required_kg(decimal("1250.5").unwrap(), decimal("2.5").unwrap()).unwrap(),
        31_262_500
    );
    assert_eq!(required_kg(1, 50_000_000).unwrap(), 1);
    assert!(required_kg(1, 1).is_err());
    assert_eq!(decimal_text(31_262_500), "31.262500");
    assert_eq!(required_kg(MAX, 100 * SCALE).unwrap(), MAX);
    for value in [
        "NaN",
        "inf",
        "1e3",
        "-1",
        "0",
        "1.0000001",
        "1..2",
        "9999999999999",
        "",
    ] {
        assert!(decimal(value).is_err(), "{value}");
    }
}
#[test]
fn preparation_child_warehouse_names_are_validated() {
    let ok = PreparationWarehouseCreate {
        parent_warehouse: "Asosiy".into(),
        name: "  Asosiy-1 ".into(),
    };
    assert_eq!(ok.parent_key().unwrap(), "Asosiy");
    assert_eq!(ok.warehouse_name().unwrap(), "Asosiy-1");
    let same = PreparationWarehouseCreate {
        parent_warehouse: "Asosiy".into(),
        name: "asosiy".into(),
    };
    assert!(same.warehouse_name().is_err());
    let blank = PreparationWarehouseCreate {
        parent_warehouse: "Asosiy".into(),
        name: "   ".into(),
    };
    assert!(blank.warehouse_name().is_err());
}
#[test]
fn preparation_recipe_rejects_duplicate_and_invalid_percent() {        let mut input = ConsumptionCreate {
        request_id: "test".into(),
        warehouse: "W".into(),
        order_id: "O".into(),
        expected_order_kg: "100".into(),
        lines: vec![ConsumptionLine {
            item_code: "A".into(),
            percent: "2".into(),
        }],
    };
    assert_eq!(input.quantities(100 * SCALE).unwrap()[0].2, 2 * SCALE);
    input.lines.push(input.lines[0].clone());
    assert!(input.quantities(100 * SCALE).is_err());
    input.lines.pop();
    input.lines[0].percent = "100.000001".into();
    assert!(input.quantities(100 * SCALE).is_err());
}
#[test]
fn preparation_formula_sorts_alphabetically_and_validates() {
    let input = FormulaUpsert {
        product_code: "  PC-1 ".into(),
        name: "Asosiy".into(),
        material_id: "builtin-pe".into(),
        lines: vec![
            FormulaLine {
                item_code: "B".into(),
                percent: "30".into(),
            },
            FormulaLine {
                item_code: "A".into(),
                percent: "70".into(),
            },
        ],
    };
    assert_eq!(input.product_key().unwrap(), "PC-1");
    let lines = input.normalized_lines().unwrap();
    assert_eq!(lines[0].0, "A");
    assert_eq!(lines[1].0, "B");
    // Jami 100% bo'lmasa (kam yoki ortiqcha) — rad etiladi.
    for percents in [["10", "80"], ["60", "50"], ["100", "1"]] {
        let bad_total = FormulaUpsert {
            product_code: "PC-1".into(),
            name: "Asosiy".into(),
            material_id: "builtin-pe".into(),
            lines: vec![
                FormulaLine {
                    item_code: "A".into(),
                    percent: percents[0].into(),
                },
                FormulaLine {
                    item_code: "B".into(),
                    percent: percents[1].into(),
                },
            ],
        };
        assert!(
            bad_total.normalized_lines().is_err(),
            "{percents:?} jami 100 emas"
        );
    }
    let dup = FormulaUpsert {
        product_code: "PC-1".into(),
        name: "Asosiy".into(),
        material_id: "builtin-pe".into(),
        lines: vec![
            FormulaLine {
                item_code: "A".into(),
                percent: "50".into(),
            },
            FormulaLine {
                item_code: "A".into(),
                percent: "50".into(),
            },
        ],
    };
    assert!(dup.normalized_lines().is_err());
    let bad = FormulaUpsert {
        product_code: "".into(),
        name: "Asosiy".into(),
        material_id: "builtin-pe".into(),
        lines: vec![FormulaLine {
            item_code: "A".into(),
            percent: "10".into(),
        }],
    };
    assert!(bad.product_key().is_err());
    // Formula nomi: bo'sh yoki 80 dan uzun — rad etiladi.
    let unnamed = FormulaUpsert {
        product_code: "PC-1".into(),
        name: "   ".into(),
        material_id: "builtin-pe".into(),
        lines: vec![FormulaLine {
            item_code: "A".into(),
            percent: "100".into(),
        }],
    };
    assert!(unnamed.formula_name().is_err());
    let long = FormulaUpsert {
        product_code: "PC-1".into(),
        name: "x".repeat(81),
        material_id: "builtin-pe".into(),
        lines: vec![FormulaLine {
            item_code: "A".into(),
            percent: "100".into(),
        }],
    };
    assert!(long.formula_name().is_err());
    // name tushirib qoldirilsa — 'Asosiy' default.
    let defaulted: FormulaUpsert =
        serde_json::from_str(r#"{"product_code":"PC-1","lines":[]}"#).unwrap();
    assert_eq!(defaulted.formula_name().unwrap(), "Asosiy");
    // material_id bo'sh bo'lsa — rad etiladi.
    assert!(defaulted.material_key().is_err());
    assert_eq!(input.material_key().unwrap(), "builtin-pe");
}
