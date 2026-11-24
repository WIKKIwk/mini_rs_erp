use super::*;
#[test]
fn raw_material_split_recorded_issue_requires_unchanged_measurements() {
    let mut report = SplitIssueCreate {
        command: sample(),
        note: "Tarozi xatosi".into(),
    };
    report.command.waste_kg = "1".into();
    let mut command = report.command.clone();
    command.issue_id = Some("raw-issue:test".into());
    command.request_id = "split-complete-01".into();
    command.waste_kg = "1.000000".into();
    assert!(command.validate().is_err());
    assert!(command.validate_recorded_issue(&report).is_ok());
    command.outputs[0].gross_kg = Some("41".into());
    command.outputs[0].kg = "40".into();
    assert!(command.validate_recorded_issue(&report).is_err());
    command = report.command.clone();
    assert!(command.validate_recorded_issue(&report).is_err()); // No saved issue ID.
    command.issue_id = Some("raw-issue:test".into());
    command.expected_revision = "2".into();
    assert!(command.validate_recorded_issue(&report).is_err());
    for waste in ["0", "2.000001", "bad"] {
        report.command.waste_kg = waste.into();
        command = report.command.clone();
        command.issue_id = Some("raw-issue:test".into());
        assert_eq!(
            command.validate_recorded_issue(&report).is_ok(),
            waste != "bad"
        );
    }
}
#[test]
fn raw_material_split_issues_record_exact_discrepancy_without_accepting_split() {
    let mut issue = SplitIssueCreate {
        command: sample(),
        note: "Tarozi qayta tekshirilsin".into(),
    };
    for (waste, kind, difference) in [
        ("1", "missing_weight", Some(1_000_000)),
        ("0", "zero_waste", Some(2_000_000)),
        ("00,000000", "zero_waste", Some(2_000_000)),
        ("2.000001", "excess_weight", Some(-1)),
        ("1,999999", "missing_weight", Some(1)),
        ("", "invalid_waste", None),
        ("bad", "invalid_waste", None),
        ("-1", "invalid_waste", None),
        ("1.", "invalid_waste", None),
    ] {
        issue.command.waste_kg = waste.into();
        let balance = issue.validate().unwrap();
        assert_eq!(balance.kind, kind, "{waste}");
        assert_eq!(balance.difference, difference, "{waste}");
        assert_eq!(balance.output, 98_000_000);
        assert!(issue.command.validate().is_err());
    }
    assert_eq!(split_issue_decimal(-1), "-0.000001");
    issue.command.waste_kg = "2".into();
    assert!(issue.validate().is_err()); // No discrepancy to report.
    assert!(issue.command.validate().is_ok());
    issue.command.waste_kg = "0".into();
    issue.note.clear();
    assert!(issue.validate().is_err());
    issue.note = "Reason".into();
    issue.command.outputs[0].kg = "40".into();
    assert!(issue.validate().is_err()); // Caller cannot invent net weight.
}
pub fn sample() -> SplitCreate {
    SplitCreate {
        request_id: "split-test-01".into(),
        issue_id: None,
        source_barcode: "parent".into(),
        expected_revision: "1.000000".into(),
        expected_kg: "100".into(),
        expected_width_mm: "1000".into(),
        expected_micron: "20".into(),
        waste_kg: "2".into(),
        outputs: vec![
            SplitOutput {
                kg: "39".into(),
                width_mm: "400".into(),
                gross_kg: Some("40".into()),
                bobina_kg: Some("1".into()),
                length_m: None,
            },
            SplitOutput {
                kg: "59".into(),
                width_mm: "600".into(),
                gross_kg: Some("60".into()),
                bobina_kg: Some("1".into()),
                length_m: None,
            },
        ],
    }
}
#[test]
fn raw_material_split_minimum_width_and_exact_cut_plan() {
    let mut input = sample();
    input.expected_width_mm = "700".into();
    for width in ["350", "354.999999", "355"] {
        input.outputs[0].width_mm = width.into();
        input.outputs[1].width_mm = width.into();
        assert!(input.validate().is_err(), "700 cannot yield 2 x {width}");
    }
    input.expected_width_mm = "710".into();
    assert!(input.validate().is_ok());
    input.outputs[1].width_mm = "355.000001".into();
    assert!(input.validate().is_err());
    input.outputs.truncate(1);
    input.outputs[0].kg = "98".into();
    input.outputs[0].gross_kg = Some("99".into());
    input.expected_width_mm = "700".into();
    for width in ["355", "400", "700"] {
        input.outputs[0].width_mm = width.into();
        assert!(input.validate().is_ok());
    }
    input.expected_width_mm = "1000".into();
    input.outputs[0].width_mm = "645".into();
    assert!(input.validate().is_err()); // 355 mm is still usable.
    input.outputs[0].width_mm = "645.000001".into();
    assert!(input.validate().is_ok());
}
#[test]
fn raw_material_split_exact_balance() {
    let mut input = sample();
    assert_eq!(
        input.validate().unwrap(),
        (
            100_000_000,
            2_000_000,
            vec![
                WeighedOutput {
                    kg: 39_000_000,
                    width_mm: 400_000_000,
                    gross_kg: 40_000_000,
                    bobina_kg: 1_000_000,
                    length_m: None,
                },
                WeighedOutput {
                    kg: 59_000_000,
                    width_mm: 600_000_000,
                    gross_kg: 60_000_000,
                    bobina_kg: 1_000_000,
                    length_m: None,
                },
            ]
        )
    );
    input.outputs[0].kg = "39.000001".into();
    assert!(input.validate().is_err());
    input.outputs[0].kg = "38.999999".into();
    input.outputs[0].gross_kg = Some("39.999999".into());
    input.waste_kg = "2.000001".into();
    assert!(input.validate().is_ok());
}
#[test]
fn raw_material_split_rejects_invalid_sizes_and_quantities() {
    for invalid in ["NaN", "-1", "0", "1e2", "1.0000001", "9999999999999"] {
        let mut input = sample();
        input.outputs[0].kg = invalid.into();
        assert!(input.validate().is_err(), "{invalid}");
    }
    let mut input = sample();
    input.outputs[0].width_mm = "1001".into();
    assert!(input.validate().is_err());
    input = sample();
    input.outputs.clear();
    assert!(input.validate().is_err());
}
#[test]
fn raw_material_split_requires_positive_waste_even_with_balanced_net() {
    let mut input = sample();
    input.outputs[0].kg = "41".into();
    input.outputs[0].gross_kg = Some("42".into());
    for waste in ["", "0", "0.0", "0.000000", "00", "-1", "NaN"] {
        input.waste_kg = waste.into();
        assert!(input.validate().is_err(), "waste={waste}");
    }
    input.waste_kg = "0.000001".into();
    input.outputs[0].kg = "40.999999".into();
    input.outputs[0].gross_kg = Some("41.999999".into());
    assert!(input.validate().is_ok());
}
#[test]
fn raw_material_split_requires_measured_weights_and_explicit_waste() {
    let mut input = sample();
    input.waste_kg.clear();
    assert!(input.validate().is_err());
    for missing in ["waste_kg", "outputs"] {
        let mut json = serde_json::to_value(sample()).unwrap();
        json.as_object_mut().unwrap().remove(missing);
        assert!(serde_json::from_value::<SplitCreate>(json).is_err());
    }
    for invalid in [
        None,
        Some(""),
        Some("-1"),
        Some("40"),
        Some("NaN"),
        Some("1.000001"),
    ] {
        let mut input = sample();
        input.outputs[0].bobina_kg = invalid.map(str::to_owned);
        assert!(input.validate().is_err());
    }
    let mut input = sample();
    input.outputs[0].bobina_kg = Some("0".into());
    input.outputs[0].gross_kg = Some("39".into());
    assert!(input.validate().is_ok());
    input.outputs[0].gross_kg = None;
    assert!(input.validate().is_err());
}
#[test]
fn raw_material_split_name_uses_child_width_and_inherited_micron() {
    for name in [
        "BOPP METAL 700/12",
        "BOPP METAL 700 / 12",
        "BOPP METAL 350/12",
        "BOPP METAL",
    ] {
        assert_eq!(
            split_item_name(name, "350.000000", "12"),
            "BOPP METAL 350/12"
        );
    }
    assert_eq!(
        split_item_name("Plyonka 3 qatlam 700/12", "349.5", "12.500000"),
        "Plyonka 3 qatlam 349.5/12.5"
    );
}
#[test]
fn raw_material_split_length_m_validation() {
    let mut input = sample();
    input.outputs[0].length_m = Some("1000".into());
    input.outputs[1].length_m = Some("2500.5".into());
    let (_, _, outputs) = input.validate().unwrap();
    assert_eq!(outputs[0].length_m, Some(1_000_000_000));
    assert_eq!(outputs[1].length_m, Some(2_500_500_000));

    input.outputs[0].length_m = Some("0".into());
    assert!(input.validate().is_err());
    input.outputs[0].length_m = Some("-10".into());
    assert!(input.validate().is_err());
}
