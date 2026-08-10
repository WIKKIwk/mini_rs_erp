use serde::{Deserialize, Serialize};

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
    CustomerName,
    Product,
    ProductName,
    Status,
    Material,
    Micron,
    LayerOptions,
    Tiraj,
    Attachment,
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
    pub layers: Vec<TelegramOrderLayer>,
    pub pending_material_id: String,
    pub pending_material_name: String,
    pub tiraj_kg: Option<f64>,
    pub step: TelegramOrderStep,
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
    format!(
        "Buyurtma raqami: №T{} {}\n\
Mijoz: {}\n\
Mahsulot: {}\n\
Holat: {}\n\n\
1. Material: {}\n\
2. Rang: —\n\
3. Tiraj: {} kg\n\
4. Menedjer: {}\n\
5. Tarafi: —\n\
6. Diametr: —\n\n\
Eslatma:",
        order_number.trim(),
        date,
        dash(&draft.customer_name),
        dash(&draft.product_name),
        dash(&draft.status),
        material,
        tiraj,
        manager,
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
    use super::{TelegramOrderDraft, normalize_order_text, order_caption};

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
            ..TelegramOrderDraft::default()
        };
        let caption = order_caption("2730", &draft, "Valiyev Abdulloh");
        assert!(caption.contains("№T2730"));
        assert!(caption.contains("1. Material: PET 12"));
        assert!(caption.contains("3. Tiraj: 1000 kg"));
        assert!(caption.contains("4. Menedjer: Valiyev Abdulloh"));
    }
}
