#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rasxot_and_astatka_stay_separate() {
        let service = ReturnedPaintService::new(Arc::new(MemoryReturnedPaintStore::new()));
        let sender = Principal {
            role: PrincipalRole::Aparatchi,
            display_name: "Bosmachi".to_string(),
            legal_name: "Bosmachi".to_string(),
            ref_: "worker-1".to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        };
        let request = service
            .create(
                ReturnedPaintRequestCreate {
                    order_id: "order-1".to_string(),
                    order_code: "1212".to_string(),
                    order_name: "Mahsulot".to_string(),
                    apparatus: "7 ta rangli bosma".to_string(),
                    image_id: String::new(),
                    items: vec![
                        ReturnedPaintItem {
                            usage: "rasxot".to_string(),
                            category: "colors".to_string(),
                            name: "Oq".to_string(),
                            values: BTreeMap::from([
                                ("Mix".to_string(), "3".to_string()),
                                ("Oq".to_string(), "1".to_string()),
                                ("Qora".to_string(), "0".to_string()),
                            ]),
                        },
                        ReturnedPaintItem {
                            usage: "astatka".to_string(),
                            category: "colors".to_string(),
                            name: "Oq".to_string(),
                            values: BTreeMap::from([
                                ("Mix".to_string(), "1".to_string()),
                                ("Oq".to_string(), "0".to_string()),
                                ("Qora".to_string(), "0".to_string()),
                            ]),
                        },
                    ],
                },
                &sender,
            )
            .await
            .expect("create request");

        assert_eq!(request.items[0].usage, "rasxot");
        assert_eq!(request.items[0].values["Mix"], "3");
        assert_eq!(request.items[1].usage, "astatka");
        assert_eq!(request.items[1].values["Mix"], "1");
        assert_eq!(returned_paint_astatka_total(&request.items), Ok(1.0));
    }

    #[test]
    fn minimum_returned_paint_fields_are_checked_per_usage() {
        let rasxot_only = vec![item(
            "rasxot",
            "colors",
            [("Mix", "1"), ("Oq", "1"), ("Qora", "0")],
        )];
        let both_usages = vec![
            item(
                "rasxot",
                "colors",
                [("Mix", "1"), ("Oq", "1"), ("Qora", "0")],
            ),
            item(
                "astatka",
                "colors",
                [("Mix", "0.5"), ("Oq", "0"), ("Qora", "0")],
            ),
        ];

        assert_eq!(
            returned_paint_value_count_for_usage(&rasxot_only, "rasxot"),
            3
        );
        assert_eq!(
            returned_paint_value_count_for_usage(&rasxot_only, "astatka"),
            0
        );
        assert!(!returned_paint_report_can_close(&rasxot_only, false));
        assert!(returned_paint_report_can_close(&both_usages, false));
        assert!(returned_paint_report_can_close(&[], true));
    }

    #[test]
    fn calculates_color_mix_and_all_solvent_fields_as_pure_alcohol() {
        let items = vec![
            item(
                "rasxot",
                "colors",
                [("Mix", "10"), ("Oq", "2"), ("Spirt", "1")],
            ),
            item("rasxot", "colors", [("Mix", "2.5"), ("Qora", "0.5")]),
            item("astatka", "colors", [("Mix", "4"), ("Oq", "1")]),
            item("astatka", "colors", [("Mix", "1"), ("Qora", "0.25")]),
            item("rasxot", "lacquers", [("OPV lak", "100")]),
            item(
                "rasxot",
                "solvents",
                [("Etil", "10"), ("Metoxil", "2"), ("Rasvavitel", "0.5")],
            ),
            item(
                "astatka",
                "solvents",
                [("Etil", "1"), ("Aralashmalar", "0.25")],
            ),
        ];

        let result = calculate_returned_paint(&items).expect("calculation");

        assert_eq!(result.rasxot_mix_total, "12.5");
        assert_eq!(result.astatka_mix_total, "5");
        assert_eq!(result.rasxot_alcohol, "16.25");
        assert_eq!(result.astatka_alcohol, "2.75");
        assert_eq!(result.final_used_alcohol, "13.5");
        assert_eq!(result.rasxot_pure_paint, "12.25");
        assert_eq!(result.astatka_pure_paint, "4.75");
        assert_eq!(result.final_used_paint, "7.5");
    }

    #[test]
    fn treats_named_values_inside_mix_item_as_mix() {
        let items = vec![
            item(
                "rasxot",
                "colors",
                [("Batch A", "10"), ("Blue recipe", "2")],
            ),
            item("astatka", "colors", [("Batch A", "4")]),
        ];

        let result = calculate_returned_paint(
            &items
                .into_iter()
                .map(|mut value| {
                    value.name = "Mix".to_string();
                    value
                })
                .collect::<Vec<_>>(),
        )
        .expect("named mix calculation");

        assert_eq!(result.rasxot_mix_total, "12");
        assert_eq!(result.astatka_mix_total, "4");
        assert_eq!(result.rasxot_alcohol, "3.6");
        assert_eq!(result.astatka_alcohol, "1.2");
        assert_eq!(result.final_used_paint, "5.6");
    }

    #[test]
    fn rejects_astatka_that_would_make_a_final_value_negative() {
        let alcohol_negative = vec![
            item("rasxot", "colors", [("Mix", "1")]),
            item("astatka", "colors", [("Mix", "2")]),
        ];
        let solvent_negative = vec![
            item("rasxot", "solvents", [("Etil", "1")]),
            item("astatka", "solvents", [("Metoxil", "2")]),
        ];
        let paint_negative = vec![
            item("rasxot", "colors", [("Mix", "1")]),
            item("astatka", "colors", [("Oq", "1")]),
        ];

        assert_eq!(
            calculate_returned_paint(&alcohol_negative),
            Err(ReturnedPaintError::NegativeFinalValue)
        );
        assert_eq!(
            calculate_returned_paint(&solvent_negative),
            Err(ReturnedPaintError::NegativeFinalValue)
        );
        assert_eq!(
            calculate_returned_paint(&paint_negative),
            Err(ReturnedPaintError::NegativeFinalValue)
        );
    }

    #[test]
    fn keeps_eleven_digit_input_precision_without_floating_point_rounding() {
        let items = serde_json::from_str::<Vec<ReturnedPaintItem>>(
            r#"[
                {"usage":"rasxot","category":"colors","name":"Oq","values":{"Mix":3e-11}},
                {"usage":"astatka","category":"colors","name":"Oq","values":{"Mix":1e-11}}
            ]"#,
        )
        .expect("decimal JSON");
        let items = normalize_items(items).expect("normalized items");

        let result = calculate_returned_paint(&items).expect("calculation");

        assert_eq!(result.rasxot_alcohol, "0.000000000009");
        assert_eq!(result.astatka_alcohol, "0.000000000003");
        assert_eq!(result.final_used_alcohol, "0.000000000006");
        assert_eq!(result.final_used_paint, "0.000000000014");
    }

    fn item<const N: usize>(
        usage: &str,
        category: &str,
        values: [(&str, &str); N],
    ) -> ReturnedPaintItem {
        ReturnedPaintItem {
            usage: usage.to_string(),
            category: category.to_string(),
            name: "card".to_string(),
            values: values
                .into_iter()
                .map(|(label, value)| (label.to_string(), value.to_string()))
                .collect(),
        }
    }
}
