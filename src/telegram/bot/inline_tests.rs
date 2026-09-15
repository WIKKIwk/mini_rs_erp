#[cfg(test)]
mod tests {
    #[test]
    fn order_form_is_separate_from_print_method_and_cold_glue() {
        let keyboard = super::status_keyboard();
        let choices = keyboard["inline_keyboard"][0].as_array().unwrap();
        let callbacks: Vec<_> = choices
            .iter()
            .map(|c| c["callback_data"].as_str().unwrap())
            .collect();
        assert_eq!(
            callbacks,
            ["order:status:roll", "order:status:package"]
        );
        assert_eq!(super::print_method_keyboard()["inline_keyboard"][0][0]["callback_data"], "order:print:flexo");
        assert_eq!(super::print_method_keyboard()["inline_keyboard"][0][1]["callback_data"], "order:print:metal");
        assert_eq!(super::cold_glue_keyboard()["inline_keyboard"][0][0]["callback_data"], "order:cold:yes");
        assert_eq!(super::parse_edge_allowance("0"), Some(0.0));
        assert_eq!(super::parse_edge_allowance(" 40,5 "), Some(40.5));
        for value in ["", "-1", "NaN", "inf", "abc"] {
            assert_eq!(super::parse_edge_allowance(value), None, "{value}");
        }
    }

    #[test]
    fn pending_order_frame_count_is_a_positive_integer() {
        assert_eq!(super::parse_frame_count(" 3 "), Some(3.0));
        assert_eq!(super::parse_frame_count("2,0"), Some(2.0));
        for value in ["0", "-1", "1.5", "NaN", "inf", "2147483648", "abc"] {
            assert_eq!(super::parse_frame_count(value), None, "{value}");
        }
    }

    #[test]
    fn micron_is_a_positive_integer_text_input_not_an_inline_choice() {
        assert!(super::parse_order_inline_query("n7 19").is_none());
        assert_eq!(super::parse_micron("19"), Some("19".to_string()));
        assert_eq!(super::parse_micron(" 019 "), Some("19".to_string()));
        for value in ["", "0", "-1", "1.5", "12a", "abc", "1/2"] {
            assert_eq!(super::parse_micron(value), None, "{value}");
        }
    }

    #[test]
    fn diameter_accepts_a_positive_decimal_and_rejects_invalid_input() {
        assert_eq!(super::parse_diameter("45.5"), Some(45.5));
        assert_eq!(super::parse_diameter(" 45,5 "), Some(45.5));
        for value in ["", "0", "-1", "NaN", "inf", "abc"] {
            assert_eq!(super::parse_diameter(value), None, "{value}");
        }
    }

    #[test]
    fn roll_count_is_a_positive_integer() {
        assert_eq!(super::parse_roll_count(" 6 "), Some(6));
        for value in ["", "0", "-1", "1.5", "2,0", "NaN", "abc"] {
            assert_eq!(super::parse_roll_count(value), None, "{value}");
        }
    }
    use super::{
        InlineLoginInput, TelegramMessage, TelegramOrderNotification, is_login_code,
        number_or_dash, order_media_from_message, parse_command, parse_inline_login_input,
        role_guide,
    };
    use crate::core::calculate_orders::{CalculateOrderImage, CalculateOrderTemplate};
    use crate::core::production_map::ProductionMapDefinition;
    use crate::telegram::TelegramAccountRole;

    #[test]
    fn parse_command_accepts_bot_mention_and_start_parameter() {
        assert_eq!(
            parse_command("/start@accord_bot invite123"),
            Some(("start".to_string(), "invite123".to_string()))
        );
        assert_eq!(
            parse_command("/code 12345"),
            Some(("code".to_string(), "12345".to_string()))
        );
        assert!(is_login_code("12345"));
        assert!(!is_login_code("12 345"));
    }

    #[test]
    fn role_guide_explains_role_and_commands() {
        let admin_guide = role_guide(TelegramAccountRole::Admin);
        assert!(admin_guide.contains("Sizning rolingiz: Admin"));
        assert!(admin_guide.contains("invite link yaratish"));
        assert!(admin_guide.contains("/help"));
        assert!(admin_guide.contains("/connect"));
        assert!(!admin_guide.contains("/user_mode"));

        let manager_guide = role_guide(TelegramAccountRole::SalesManager);
        assert!(manager_guide.contains("Sizning rolingiz: Sotuv manageri"));
        assert!(manager_guide.contains("Yangi orderlar yuborilgan guruhni kuzatish"));
        assert!(manager_guide.contains("/commands"));
        assert!(manager_guide.contains("/user_mode"));
        assert!(manager_guide.contains("inline"));
        assert!(!manager_guide.contains("/password <parol>"));
    }

    #[test]
    fn inline_login_input_keeps_code_and_password_out_of_normal_messages() {
        assert_eq!(
            parse_inline_login_input("q7 47989"),
            Some(InlineLoginInput::Code("47989".to_string()))
        );
        assert_eq!(
            parse_inline_login_input("p4 my secret password"),
            Some(InlineLoginInput::Password("my secret password".to_string()))
        );
        assert_eq!(parse_inline_login_input("47989"), None);
        assert_eq!(
            parse_inline_login_input("q7 123456"),
            Some(InlineLoginInput::Code("123456".to_string()))
        );
        assert_eq!(parse_inline_login_input("q7 4924"), None);
        assert_eq!(parse_inline_login_input("q7 1234567"), None);
    }

    #[test]
    fn order_media_accepts_photos_and_image_documents_only() {
        let photo_message: TelegramMessage = serde_json::from_value(serde_json::json!({
            "chat": {"id": 1, "type": "private"},
            "photo": [
                {"file_id": "small", "width": 100, "height": 100},
                {"file_id": "large", "width": 1000, "height": 1000, "file_size": 42}
            ]
        }))
        .expect("photo message");
        let photo = order_media_from_message(&photo_message).expect("photo media");
        assert_eq!(photo.file_id, "large");
        assert_eq!(photo.mime_type, "image/jpeg");

        let document_message: TelegramMessage = serde_json::from_value(serde_json::json!({
            "chat": {"id": 1, "type": "private"},
            "document": {"file_id": "design", "file_name": "design.png"}
        }))
        .expect("document message");
        let document = order_media_from_message(&document_message).expect("image document");
        assert_eq!(document.file_id, "design");
        assert_eq!(document.mime_type, "image/png");

        let other_document: TelegramMessage = serde_json::from_value(serde_json::json!({
            "chat": {"id": 1, "type": "private"},
            "document": {"file_id": "spec", "file_name": "spec.pdf", "mime_type": "application/pdf"}
        }))
        .expect("other document message");
        assert!(order_media_from_message(&other_document).is_none());
    }

    #[test]
    fn telegram_delivery_does_not_treat_webp_as_a_photo() {
        let mut image = CalculateOrderImage {
            image_mime: "image/webp".to_string(),
            image_name: "order.webp".to_string(),
            body: vec![1, 2, 3],
            ..CalculateOrderImage::default()
        };
        assert!(!super::can_send_as_telegram_photo(&image));

        image.image_mime = "image/jpeg".to_string();
        assert!(super::can_send_as_telegram_photo(&image));
    }

    #[test]
    fn order_notification_contains_core_order_fields() {
        let notification = TelegramOrderNotification::from_order(
            ProductionMapDefinition {
                print_val_size_mm: None,
                id: "zakaz-2731".to_string(),
                product_code: "MOLLY".to_string(),
                title: "Molly".to_string(),
                code: "2731".to_string(),
                order_number: "2731".to_string(),
                customer_name: "Freshboll".to_string(),
                image_id: String::new(),
                roll_count: None,
                width_mm: Some(680.0),
                order_kg: Some(1000.0),
                base_length: None,
                nodes: Vec::new(),
                edges: Vec::new(),
            },
            CalculateOrderTemplate {
                name: "Molly order".to_string(),
                product: "Molly 70 gr Sour Pencil mix".to_string(),
                material_display: "PET 12 + CPP 35".to_string(),
                color: "faylga".to_string(),
                kg: 1000.0,
                frame_product_size_mm: 220.0,
                frame_count: 3.0,
                ..CalculateOrderTemplate::default()
            },
            None,
            "Valiyev Abdulla".to_string(),
        );
        assert!(notification.caption.contains("№2731"));
        assert!(notification.caption.contains("Freshboll"));
        assert!(notification.caption.contains("Valiyev Abdulla"));
    }

    #[test]
    fn number_format_is_compact() {
        assert_eq!(number_or_dash(1000.0), "1000");
        assert_eq!(number_or_dash(12.5), "12.5");
        assert_eq!(number_or_dash(0.0), "—");
    }
}
