//! Pechat (printing apparatus) compatibility rules.
//!
//! Mirror of the mobile client rules in
//! `accord_mobile/lib/src/features/admin/logic/production_map_pechat_rules.dart`.
//! The server is the source of truth: moves are validated here before any
//! apparatus change is persisted.

/// Rubber plate size derived from order width, in 50mm steps (50..=1300).
pub fn rubber_size_from_width(width_mm: f64) -> i64 {
    let steps = (width_mm / 50.0).ceil() as i64;
    steps.clamp(1, 26) * 50
}

/// Parses the pechat color count (7/8/9) out of an apparatus title such as
/// "7 ta rangli pechat - A", "8 rangli val" or "9ta rangli".
pub fn pechat_color_count(title: &str) -> Option<u8> {
    let lower = title.trim().to_lowercase();
    let bytes = lower.as_bytes();
    let mut search_from = 0;
    while let Some(found) = lower[search_from..].find("rangli") {
        let index = search_from + found;
        if let Some(count) = color_count_before(bytes, index) {
            return Some(count);
        }
        search_from = index + "rangli".len();
    }
    None
}

fn color_count_before(bytes: &[u8], rangli_start: usize) -> Option<u8> {
    let mut i = rangli_start;
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    if i >= 2 && &bytes[i - 2..i] == b"ta" {
        i -= 2;
        while i > 0 && bytes[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
    }
    if i == 0 {
        return None;
    }
    let digit = bytes[i - 1];
    if !matches!(digit, b'7' | b'8' | b'9') {
        return None;
    }
    if i >= 2 && bytes[i - 2].is_ascii_alphanumeric() {
        return None;
    }
    Some(digit - b'0')
}

/// Minimal pechat color count required by the order, or `None` when the
/// order data does not constrain the pechat (or exceeds all pechats).
pub fn recommended_pechat_color_count(
    roll_count: Option<f64>,
    width_mm: Option<f64>,
) -> Option<u8> {
    let roll = roll_count.filter(|value| *value > 0.0);
    let width = width_mm.filter(|value| *value > 0.0);
    if roll.is_none() && width.is_none() {
        return None;
    }

    let mut required: u8 = 0;
    if let Some(roll) = roll {
        if roll > 9.0 {
            return None;
        }
        required = if roll > 8.0 {
            9
        } else if roll > 7.0 {
            8
        } else {
            7
        };
    }
    if let Some(width) = width {
        let rubber = rubber_size_from_width(width);
        if rubber > 1300 {
            return None;
        }
        let rubber_required = if rubber > 1000 {
            9
        } else if rubber > 800 {
            8
        } else {
            7
        };
        required = required.max(rubber_required);
    }
    if required == 0 { None } else { Some(required) }
}

/// Whether a pechat with the given color count can physically handle the order.
pub fn pechat_can_handle_order(
    apparatus_color_count: u8,
    roll_count: Option<f64>,
    width_mm: Option<f64>,
) -> bool {
    if let Some(roll) = roll_count {
        if roll > f64::from(apparatus_color_count) {
            return false;
        }
    }
    let Some(width) = width_mm.filter(|value| *value > 0.0) else {
        return true;
    };
    let rubber = rubber_size_from_width(width);
    match apparatus_color_count {
        7 => rubber <= 800,
        8 => (150..=1000).contains(&rubber),
        9 => (800..=1300).contains(&rubber),
        _ => false,
    }
}

/// Highest pechat color count among the order's apparatus titles.
pub fn order_pechat_color_count<'a>(titles: impl IntoIterator<Item = &'a str>) -> Option<u8> {
    titles.into_iter().filter_map(pechat_color_count).max()
}

/// Whether an apparatus node belongs to the source warehouse/pechat being moved
/// from. Pechat nodes match by color count so minor title suffixes still work.
pub fn apparatus_node_matches_from(node_title: &str, from_apparatus: &str) -> bool {
    let from = from_apparatus.trim();
    let title = node_title.trim();
    if title == from {
        return true;
    }
    let Some(from_color) = pechat_color_count(from) else {
        return false;
    };
    pechat_color_count(title) == Some(from_color)
}

/// Whether the order may be moved onto a pechat with the given color count.
pub fn pechat_can_move_order(
    apparatus_color_count: u8,
    roll_count: Option<f64>,
    width_mm: Option<f64>,
    source_apparatus_color_count: Option<u8>,
) -> bool {
    if let Some(recommended) = recommended_pechat_color_count(roll_count, width_mm) {
        if apparatus_color_count < recommended {
            return false;
        }
    }
    let moving_down = source_apparatus_color_count
        .map(|source| apparatus_color_count < source)
        .unwrap_or(false);
    if moving_down {
        if width_mm.filter(|value| *value > 0.0).is_none() {
            return false;
        }
        return pechat_can_handle_order(apparatus_color_count, roll_count, width_mm);
    }
    let has_roll = roll_count.filter(|value| *value > 0.0).is_some();
    let has_width = width_mm.filter(|value| *value > 0.0).is_some();
    if !has_roll || !has_width {
        return apparatus_color_count != 9;
    }
    pechat_can_handle_order(apparatus_color_count, roll_count, width_mm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pechat_color_count_parses_apparatus_titles() {
        assert_eq!(pechat_color_count("7 ta rangli pechat - A"), Some(7));
        assert_eq!(pechat_color_count("8 ta rangli pechat"), Some(8));
        assert_eq!(pechat_color_count("9 rangli val"), Some(9));
        assert_eq!(pechat_color_count("7ta rangli"), Some(7));
        assert_eq!(pechat_color_count("Paket aparat"), None);
        assert_eq!(pechat_color_count("17 rangli"), None);
        assert_eq!(pechat_color_count("rangli pechat"), None);
    }

    #[test]
    fn recommended_color_count_uses_roll_and_rubber() {
        assert_eq!(recommended_pechat_color_count(Some(7.0), None), Some(7));
        assert_eq!(recommended_pechat_color_count(Some(8.0), None), Some(8));
        assert_eq!(recommended_pechat_color_count(Some(9.0), None), Some(9));
        assert_eq!(recommended_pechat_color_count(Some(10.0), None), None);
        assert_eq!(recommended_pechat_color_count(None, Some(650.0)), Some(7));
        assert_eq!(recommended_pechat_color_count(None, Some(900.0)), Some(8));
        assert_eq!(recommended_pechat_color_count(None, Some(1250.0)), Some(9));
        // Width is clamped to 26 rubber steps (1300mm), matching the client.
        assert_eq!(recommended_pechat_color_count(None, Some(1500.0)), Some(9));
        assert_eq!(
            recommended_pechat_color_count(Some(7.0), Some(1250.0)),
            Some(9)
        );
        assert_eq!(recommended_pechat_color_count(None, None), None);
    }

    #[test]
    fn move_allows_compatible_order_from_seven_to_eight_color_pechat() {
        assert!(pechat_can_move_order(8, Some(7.0), Some(650.0), Some(7)));
    }

    #[test]
    fn apparatus_node_matches_from_uses_pechat_color_count() {
        assert!(apparatus_node_matches_from(
            "7 ta rangli pechat - A",
            "7 ta rangli pechat",
        ));
        assert!(!apparatus_node_matches_from(
            "8 ta rangli pechat",
            "7 ta rangli pechat",
        ));
    }

    #[test]
    fn move_blocks_nine_color_rubber_on_seven_color_pechat() {
        assert!(!pechat_can_move_order(7, Some(7.0), Some(1250.0), Some(8)));
        assert!(pechat_can_move_order(9, Some(7.0), Some(1250.0), Some(8)));
    }

    #[test]
    fn move_down_requires_width_and_compatibility() {
        assert!(!pechat_can_move_order(7, Some(7.0), None, Some(8)));
        assert!(pechat_can_move_order(7, Some(7.0), Some(650.0), Some(8)));
        assert!(!pechat_can_move_order(7, Some(7.0), Some(900.0), Some(8)));
    }

    #[test]
    fn move_without_order_data_avoids_nine_color() {
        assert!(pechat_can_move_order(8, None, None, None));
        assert!(!pechat_can_move_order(9, None, None, None));
    }
}
