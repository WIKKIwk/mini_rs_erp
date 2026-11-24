use super::{
    TelegramOrderDraft, TelegramOrderLayer, TelegramOrderStep, normalize_order_text,
    order_caption, order_prompt,
};

#[test]
fn flexo_draft_retains_allowance_and_older_drafts_still_load() {
    let old: TelegramOrderDraft =
        serde_json::from_str(r#"{"status":"rulon","step":"material"}"#).unwrap();
    assert_eq!(old.edge_allowance_mm, None);
    assert_eq!(old.side, None);
    assert_eq!(old.side_prompt_message_id, None);
    let draft = TelegramOrderDraft {
        status: "flexo".into(),
        edge_allowance_mm: Some(40.5),
        step: super::TelegramOrderStep::EdgeAllowance,
        ..Default::default()
    };
    let restored: TelegramOrderDraft =
        serde_json::from_value(serde_json::to_value(&draft).unwrap()).unwrap();
    assert_eq!(restored, draft);
}

#[test]
fn latin_and_cyrillic_customer_names_have_the_same_key() {
    assert_eq!(
        normalize_order_text("Freshboll"),
        normalize_order_text("Фрешболл")
    );
    assert_eq!(normalize_order_text("O'g'il"), normalize_order_text("Ўғил"));
}

#[test]
fn order_caption_keeps_the_partial_wizard_fields_in_screenshot_order() {
    let draft = TelegramOrderDraft {
        customer_name: "freshboll".to_string(),
        product_name: "Jolly Molly 70 gr Sour Pencil mix".to_string(),
        status: "rulon".to_string(),
        layers: vec![super::TelegramOrderLayer {
            material_id: "pet".to_string(),
            material: "PET".to_string(),
            micron: "12".to_string(),
        }],
        tiraj_kg: Some(1000.0),
        frame_product_size_mm: Some(300.0),
        frame_count: Some(2.0),
        diameter_mm: Some(45.5),
        roll_count: Some(6),
        ..TelegramOrderDraft::default()
    };
    let caption = order_caption("2730", &draft, "Valiyev Abdulloh");
    assert!(caption.contains("№T2730"));
    assert!(caption.contains("1. Material: PET 12"));
    assert!(caption.contains("2. Rang: 6 xil"));
    assert!(caption.contains("3. Tiraj: 1000 kg"));
    assert!(caption.contains("4. Menedjer: Valiyev Abdulloh"));
    assert!(caption.contains("5. Diametr: 45.5 mm"));
    assert!(!caption.contains("Kadrdagi o‘lcham"));
    assert!(!caption.contains("Kadr soni"));
    assert!(!caption.contains("Chala buyurtma"));
    assert!(!caption.contains("Eslatma:"));
}

#[test]
fn empty_order_prompt_shows_all_fields_above_the_current_question() {
    let text = order_prompt("", &TelegramOrderDraft::default(), false, "👤 Mijozni tanlang:");
    for field in [
        "№T—",
        "Mijoz: —",
        "Mahsulot: —",
        "Turi: —",
        "Tiraj: — kg",
        "Bitta kadrdagi mahsulot o‘lchami: — mm",
        "Kadr soni: — ta",
        "Diametr: — mm",
        "1. Material: — · Mikron: —",
        "Bosma turi: —",
        "Val/rang soni: —",
        "Edge allowance: —",
        "Cold glue: —",
        "Order rasmi: ❌ Kutilmoqda",
        "Taraf: —",
    ] {
        assert!(text.contains(field), "missing {field}: {text}");
    }
    assert!(text.ends_with("━━━━━━━━━━━━━━\n\n👤 Mijozni tanlang:"));
    assert_eq!(text.matches("📌 Buyurtma asoslari").count(), 1);
}

#[test]
fn order_prompt_updates_partial_layers_and_keeps_entered_values_between_steps() {
    let mut draft = TelegramOrderDraft {
        customer_name: "365 Korzinka".into(),
        product_name: "Guruch 1 kg".into(),
        status: "rulon".into(),
        tiraj_kg: Some(123.0),
        pending_material_id: "bopp".into(),
        pending_material_name: "BOPP".into(),
        step: TelegramOrderStep::Micron,
        ..Default::default()
    };
    let text = order_prompt("", &draft, false, "Mikronni kiriting:");
    assert!(text.contains("1. BOPP — mikron: —"));
    assert!(text.ends_with("Mikronni kiriting:"));

    draft.layers.push(TelegramOrderLayer {
        material_id: std::mem::take(&mut draft.pending_material_id),
        material: std::mem::take(&mut draft.pending_material_name),
        micron: "20".into(),
    });
    draft.step = TelegramOrderStep::Material;
    let text = order_prompt("", &draft, false, "2-qavat materialini tanlang:");
    assert!(text.contains("Mijoz: 365 Korzinka"));
    assert!(text.contains("Mahsulot: Guruch 1 kg"));
    assert!(text.contains("Turi: Rulon"));
    assert!(text.contains("Tiraj: 123 kg"));
    assert!(text.contains("1. BOPP — 20 mikron\n2. Material: — · Mikron: —"));
    assert!(!text.contains("Mikronni kiriting:"));
    assert!(text.ends_with("2-qavat materialini tanlang:"));

    draft.step = TelegramOrderStep::Review;
    let text = order_prompt("0019", &draft, true, "🧾 Buyurtmani tekshiring");
    assert!(!text.contains("2. Material:"));
    assert!(text.contains("Order rasmi: ✅ Yuklandi"));
    assert_eq!(text.matches("📌 Buyurtma asoslari").count(), 1);
    assert!(text.ends_with("🧾 Buyurtmani tekshiring"));
}

#[test]
fn image_upload_requires_one_of_four_sides_before_review() {
    for value in ["1", "2", "3", "4"] {
        let mut draft = TelegramOrderDraft {
            step: TelegramOrderStep::Attachment,
            side: Some(4),
            ..Default::default()
        };
        assert!(!draft.select_side(value));
        draft.request_side();
        assert_eq!(draft.step, TelegramOrderStep::Side);
        assert_eq!(draft.side, None);
        for invalid in ["", "0", "5", "-1", "01", "abc"] {
            assert!(!draft.select_side(invalid));
            assert_eq!(draft.step, TelegramOrderStep::Side);
            assert_eq!(draft.side, None);
        }
        assert!(draft.select_side(value));
        assert_eq!(draft.side, Some(value.parse().unwrap()));
        assert_eq!(draft.step, TelegramOrderStep::Review);
        assert!(!draft.select_side("2"), "stale buttons must not change the selection");
        let field = format!("Taraf: {value}");
        assert!(order_prompt("0019", &draft, true, "Tekshiring").contains(&field));
        assert!(order_caption("0019", &draft, "Manager").contains(&field));
    }
}

#[test]
fn order_review_lists_every_layer_and_confirmation_fields() {
    let draft = TelegramOrderDraft {
        customer_name: "Freshboll".into(),
        product_name: "Jolly Molly".into(),
        status: "rulon".into(),
        print_method: Some(crate::core::production_map::automatic::PrintMethod::Flexo),
        cold_glue: Some(true),
        edge_allowance_mm: Some(15.0),
        layers: vec![
            TelegramOrderLayer {
                material_id: "pet".into(),
                material: "PET".into(),
                micron: "12".into(),
            },
            TelegramOrderLayer {
                material_id: "pe".into(),
                material: "PE".into(),
                micron: "50".into(),
            },
        ],
        tiraj_kg: Some(500.0),
        frame_product_size_mm: Some(300.0),
        frame_count: Some(2.0),
        diameter_mm: Some(45.5),
        roll_count: Some(6),
        step: TelegramOrderStep::Review,
        ..Default::default()
    };
    let review = order_prompt("2730", &draft, true, "🧾 Buyurtmani tekshiring");
    assert!(review.contains("Freshboll"));
    assert!(review.contains("Tiraj: 500 kg"));
    assert!(review.contains("1. PET — 12 mikron"));
    assert!(review.contains("2. PE — 50 mikron"));
    assert!(review.contains("Edge allowance: 15 mm"));
    assert!(review.contains("Cold glue: Ha"));
    assert!(review.contains("Order rasmi: ✅ Yuklandi"));
}
