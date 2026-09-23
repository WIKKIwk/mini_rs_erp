use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub(crate) struct TelegramOrderAttachment {
    pub file_name: String,
    pub mime_type: String,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TelegramOrderLayer {
    pub material_id: String,
    pub material: String,
    pub micron: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TelegramOrderStep {
    #[default]
    Customer,
    CustomerConfirmation,
    CustomerName,
    Product,
    ProductConfirmation,
    ProductName,
    Status,
    Tiraj,
    FrameSize,
    FrameCount,
    Diameter,
    Material,
    Micron,
    LayerOptions,
    PrintMethod,
    ValCount,
    EdgeAllowance,
    ColdGlue,
    Attachment,
    Side,
    Review,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TelegramOrderEditSection {
    Basics,
    Dimensions,
    Layers,
    Print,
    Image,
    Side,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct TelegramOrderDraft {
    pub order_number: String,
    pub customer_ref: String,
    pub customer_name: String,
    pub product_code: String,
    pub product_name: String,
    pub status: String,
    pub print_method: Option<crate::core::production_map::automatic::PrintMethod>,
    pub cold_glue: Option<bool>,
    pub edge_allowance_mm: Option<f64>,
    pub layers: Vec<TelegramOrderLayer>,
    pub pending_material_id: String,
    pub pending_material_name: String,
    pub tiraj_kg: Option<f64>,
    pub frame_product_size_mm: Option<f64>,
    pub frame_count: Option<f64>,
    pub diameter_mm: Option<f64>,
    pub roll_count: Option<i64>,
    pub side: Option<u8>,
    #[serde(default)]
    pub edit_section: Option<TelegramOrderEditSection>,
    #[serde(default)]
    pub pending_order_saved: bool,
    #[serde(default)]
    pub customer_prompt_message_id: Option<i64>,
    #[serde(default)]
    pub product_prompt_message_id: Option<i64>,
    #[serde(default)]
    pub prompt_message_id: Option<i64>,
    #[serde(default)]
    pub prompt_inline_message_id: Option<String>,
    pub side_prompt_message_id: Option<i64>,
    pub step: TelegramOrderStep,
}

impl TelegramOrderDraft {
    pub(crate) fn request_side(&mut self) {
        self.edit_section = None;
        self.side = None;
        self.step = TelegramOrderStep::Side;
    }

    pub(crate) fn select_side(&mut self, value: &str) -> bool {
        if self.step != TelegramOrderStep::Side {
            return false;
        }
        let side = match value {
            "1" => 1,
            "2" => 2,
            "3" => 3,
            "4" => 4,
            _ => return false,
        };
        self.side = Some(side);
        self.edit_section = None;
        self.step = TelegramOrderStep::Review;
        true
    }

    pub(crate) fn production_options(
        &self,
    ) -> Option<crate::core::production_map::automatic::OrderProductionOptions> {
        Some(crate::core::production_map::automatic::OrderProductionOptions {
            print_method: self.print_method?,
            cold_glue: self.cold_glue?,
            diameter_mm: self.diameter_mm,
        })
    }
}

pub(crate) fn normalize_order_text(value: &str) -> String {
    let mut normalized = String::new();
    for character in value.to_lowercase().chars() {
        let replacement = match character {
            'а' => "a",
            'б' => "b",
            'в' => "v",
            'г' => "g",
            'д' => "d",
            'е' => "e",
            'ё' => "yo",
            'ж' => "j",
            'з' => "z",
            'и' => "i",
            'й' => "y",
            'к' => "k",
            'л' => "l",
            'м' => "m",
            'н' => "n",
            'о' => "o",
            'п' => "p",
            'р' => "r",
            'с' => "s",
            'т' => "t",
            'у' => "u",
            'ф' => "f",
            'х' => "x",
            'ц' => "ts",
            'ч' => "ch",
            'ш' => "sh",
            'щ' => "shch",
            'ъ' | 'ь' => "",
            'ы' => "i",
            'э' => "e",
            'ю' => "yu",
            'я' => "ya",
            'ў' => "o",
            'қ' => "q",
            'ғ' => "g",
            'ҳ' => "h",
            'a'..='z' | '0'..='9' => {
                normalized.push(character);
                continue;
            }
            _ => "",
        };
        normalized.push_str(replacement);
    }
    normalized
}

pub(crate) fn order_caption(
    order_number: &str,
    draft: &TelegramOrderDraft,
    manager_name: &str,
) -> String {
    let local_now = time::OffsetDateTime::now_utc()
        .to_offset(time::UtcOffset::from_hms(5, 0, 0).expect("Tashkent UTC offset is valid"));
    let date = format!(
        "{:02}/{:02}/{:02}",
        local_now.day(),
        u8::from(local_now.month()),
        local_now.year().rem_euclid(100)
    );
    let material = if draft.layers.is_empty() {
        "—".to_string()
    } else {
        draft
            .layers
            .iter()
            .map(|layer| format!("{} {}", layer.material, layer.micron))
            .collect::<Vec<_>>()
            .join(" + ")
    };
    let tiraj = draft
        .tiraj_kg
        .map(format_number)
        .unwrap_or_else(|| "—".to_string());
    let manager = if manager_name.trim().is_empty() {
        "Mini RS ERP"
    } else {
        manager_name.trim()
    };
    let diameter = draft
        .diameter_mm
        .map(format_number)
        .unwrap_or_else(|| "—".to_string());
    let color = draft
        .roll_count
        .filter(|value| *value > 0)
        .map(|value| format!("{value} xil"))
        .unwrap_or_else(|| "—".to_string());
    let method = match draft.print_method {
        Some(crate::core::production_map::automatic::PrintMethod::Flexo) => "Flexo",
        Some(crate::core::production_map::automatic::PrintMethod::Metal) => "Temir",
        None => "—",
    };
    let cold = match draft.cold_glue {
        Some(true) => "Ha",
        Some(false) => "Yo‘q",
        None => "—",
    };
    let side = draft.side.map(|value| value.to_string()).unwrap_or_else(|| "—".into());
    format!(
        "Buyurtma raqami: №T{} {}\n\
Mijoz: {}\n\
Mahsulot: {}\n\
Holat: {}\n\
Bosma: {method}\n\
Holodniy kley: {cold}\n\
Taraf: {side}\n\n\
1. Material: {}\n\
2. Rang: {}\n\
3. Tiraj: {} kg\n\
4. Menedjer: {}\n\
5. Diametr: {} mm",
        order_number.trim(),
        date,
        dash(&draft.customer_name),
        dash(&draft.product_name),
        dash(&draft.status),
        material,
        color,
        tiraj,
        manager,
        diameter,
    )
}

pub(crate) fn order_prompt(
    order_number: &str,
    draft: &TelegramOrderDraft,
    has_image: bool,
    prompt: &str,
) -> String {
    let status = match draft.status.as_str() {
        "rulon" => "Rulon",
        "paket" => "Paket",
        value if !value.trim().is_empty() => value.trim(),
        _ => "—",
    };
    let method = match draft.print_method {
        Some(crate::core::production_map::automatic::PrintMethod::Flexo) => "Flexo",
        Some(crate::core::production_map::automatic::PrintMethod::Metal) => "Temir",
        None => "—",
    };
    let cold_glue = match draft.cold_glue {
        Some(true) => "Ha",
        Some(false) => "Yo‘q",
        None => "—",
    };
    let edge_allowance = match draft.print_method {
        Some(crate::core::production_map::automatic::PrintMethod::Flexo) => draft
            .edge_allowance_mm
            .map(|value| format!("{} mm", format_number(value)))
            .unwrap_or_else(|| "—".to_string()),
        Some(crate::core::production_map::automatic::PrintMethod::Metal) => {
            "Qo‘llanmaydi".to_string()
        }
        None => "—".to_string(),
    };
    let mut layers = draft
        .layers
        .iter()
        .enumerate()
        .map(|(index, layer)| {
            format!(
                "{}. {} — {} mikron",
                index + 1,
                dash(&layer.material),
                dash(&layer.micron)
            )
        })
        .collect::<Vec<_>>();
    if !draft.pending_material_name.trim().is_empty() {
        layers.push(format!(
            "{}. {} — mikron: —",
            layers.len() + 1,
            draft.pending_material_name.trim()
        ));
    } else if layers.is_empty() || draft.step == TelegramOrderStep::Material {
        layers.push(format!("{}. Material: — · Mikron: —", layers.len() + 1));
    }
    let layers = layers.join("\n");
    let image = if has_image { "✅ Yuklandi" } else { "❌ Kutilmoqda" };
    let side = draft.side.map(|value| value.to_string()).unwrap_or_else(|| "—".into());
    let tiraj = draft
        .tiraj_kg
        .map(format_number)
        .unwrap_or_else(|| "—".to_string());
    let frame_size = draft
        .frame_product_size_mm
        .map(format_number)
        .unwrap_or_else(|| "—".to_string());
    let frame_count = draft
        .frame_count
        .map(format_number)
        .unwrap_or_else(|| "—".to_string());
    let diameter = draft
        .diameter_mm
        .map(format_number)
        .unwrap_or_else(|| "—".to_string());
    let roll_count = draft
        .roll_count
        .map(|value| value.to_string())
        .unwrap_or_else(|| "—".to_string());
    format!(
        "🧾 Buyurtma ma’lumotlari\n\n\
№T{}\n\n\
📌 Buyurtma asoslari\n\
Mijoz: {}\n\
Mahsulot: {}\n\
Turi: {status}\n\
Tiraj: {tiraj} kg\n\n\
📐 O‘lchamlar\n\
Bitta kadrdagi mahsulot o‘lchami: {frame_size} mm\n\
Kadr soni: {frame_count} ta\n\
Diametr: {diameter} mm\n\n\
🧱 Material qatlamlari\n{layers}\n\n\
🖨 Bosma parametrlari\n\
Bosma turi: {method}\n\
Val/rang soni: {roll_count}\n\
Edge allowance: {edge_allowance}\n\
Cold glue: {cold_glue}\n\n\
🖼 Order rasmi: {image}\n\
Taraf: {side}\n\n\
━━━━━━━━━━━━━━\n\n\
{prompt}",
        if order_number.trim().is_empty() {
            "—"
        } else {
            order_number.trim()
        },
        dash(&draft.customer_name),
        dash(&draft.product_name),
    )
}

fn dash(value: &str) -> &str {
    if value.trim().is_empty() {
        "—"
    } else {
        value.trim()
    }
}

fn format_number(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
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
}
