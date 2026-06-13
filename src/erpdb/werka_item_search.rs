use crate::core::werka::models::{CustomerItemOption, SupplierItem};

pub(crate) struct SupplierItemSearchEntry {
    pub(crate) item: SupplierItem,
    pub(crate) search_terms: Vec<String>,
}

pub(crate) fn rank_supplier_items_by_query(
    items: Vec<SupplierItem>,
    query: &str,
) -> Vec<SupplierItem> {
    if query.trim().is_empty() {
        return items;
    }

    let mut scored = items
        .into_iter()
        .filter_map(|item| {
            let score = search_query_score(query, &[&item.code, &item.name]);
            (score > 0).then_some((item, score))
        })
        .collect::<Vec<_>>();

    scored.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| normalized_cmp_key(&left.name).cmp(&normalized_cmp_key(&right.name)))
            .then_with(|| normalized_cmp_key(&left.code).cmp(&normalized_cmp_key(&right.code)))
    });

    scored.into_iter().map(|(item, _)| item).collect()
}

pub(crate) fn rank_customer_item_entries_by_query(
    items: Vec<SupplierItemSearchEntry>,
    query: &str,
) -> Vec<SupplierItem> {
    if query.trim().is_empty() {
        return items.into_iter().map(|entry| entry.item).collect();
    }

    let mut scored = items
        .into_iter()
        .filter_map(|entry| {
            let terms = entry
                .search_terms
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            let score = search_query_score(query, &terms);
            (score > 0).then_some((entry.item, score))
        })
        .collect::<Vec<_>>();

    scored.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| normalized_cmp_key(&left.name).cmp(&normalized_cmp_key(&right.name)))
            .then_with(|| normalized_cmp_key(&left.code).cmp(&normalized_cmp_key(&right.code)))
    });

    scored.into_iter().map(|(item, _)| item).collect()
}

pub(crate) fn rank_customer_item_options_by_query(
    items: Vec<CustomerItemOption>,
    query: &str,
) -> Vec<CustomerItemOption> {
    if query.trim().is_empty() {
        return items;
    }

    let mut scored = items
        .into_iter()
        .filter_map(|item| {
            let item_score = search_query_score(query, &[&item.item_code, &item.item_name]);
            let customer_score = search_query_score(
                query,
                &[
                    &item.customer_name,
                    &item.customer_ref,
                    &item.customer_phone,
                ],
            );
            (item_score > 0 || customer_score > 0).then_some((item, item_score, customer_score))
        })
        .collect::<Vec<_>>();

    scored.sort_by(
        |(left, left_item, left_customer), (right, right_item, right_customer)| {
            right_item
                .cmp(left_item)
                .then_with(|| right_customer.cmp(left_customer))
                .then_with(|| {
                    normalized_cmp_key(&left.item_name).cmp(&normalized_cmp_key(&right.item_name))
                })
                .then_with(|| {
                    normalized_cmp_key(&left.customer_name)
                        .cmp(&normalized_cmp_key(&right.customer_name))
                })
                .then_with(|| {
                    normalized_cmp_key(&left.item_code).cmp(&normalized_cmp_key(&right.item_code))
                })
        },
    );

    scored.into_iter().map(|(item, _, _)| item).collect()
}

pub(crate) fn append_search_terms(terms: &mut Vec<String>, joined: &str) {
    terms.extend(
        joined
            .split('\n')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string),
    );
}

pub(crate) fn slice_page<T: Clone>(items: &[T], offset: usize, limit: usize) -> Vec<T> {
    if offset >= items.len() {
        return Vec::new();
    }

    let end = if limit > 0 {
        items.len().min(offset + limit)
    } else {
        items.len()
    };
    items[offset..end].to_vec()
}

fn search_query_score(query: &str, values: &[&str]) -> i32 {
    let needle = normalize_for_search(query);
    if needle.is_empty() {
        return 1;
    }

    let needle_compact = needle.replace(' ', "");
    let needle_skeleton = search_skeleton(&needle_compact);
    let mut best = 0;
    for (index, value) in values.iter().enumerate() {
        let mut score = search_value_score(&needle, &needle_compact, &needle_skeleton, value);
        if score == 0 {
            continue;
        }
        score -= (index as i32) * 10;
        best = best.max(score);
    }
    best
}

fn search_value_score(
    needle: &str,
    needle_compact: &str,
    needle_skeleton: &str,
    value: &str,
) -> i32 {
    let haystack = normalize_for_search(value);
    if haystack.is_empty() {
        return 0;
    }
    if haystack == needle {
        return 1000;
    }
    if haystack.starts_with(needle) {
        return 850;
    }
    if haystack
        .split_whitespace()
        .any(|word| word.starts_with(needle))
    {
        return 700;
    }
    if haystack.contains(needle) {
        return 550;
    }

    if needle_compact.is_empty() {
        return 0;
    }
    let haystack_compact = haystack.replace(' ', "");
    if haystack_compact == needle_compact {
        return 500;
    }
    if haystack_compact.starts_with(needle_compact) {
        return 425;
    }
    if haystack_compact.contains(needle_compact) {
        return 350;
    }

    if needle_skeleton.is_empty() || needle_skeleton.len() < 3 {
        return 0;
    }
    let haystack_skeleton = search_skeleton(&haystack_compact);
    if haystack_skeleton == needle_skeleton {
        return 250;
    }
    if haystack_skeleton.starts_with(needle_skeleton) {
        return 175;
    }
    if haystack_skeleton.contains(needle_skeleton) {
        return 125;
    }
    0
}

fn normalize_for_search(input: &str) -> String {
    if input.trim().is_empty() {
        return String::new();
    }

    let lower = input.trim().to_lowercase();
    let transliterated = transliterate_cyrillic_to_latin(&lower)
        .replace(['\'', '`', 'ʻ', 'ʼ', '’'], "")
        .replace('x', "h");
    let mut result = String::with_capacity(transliterated.len());
    let mut last_was_space = false;
    for ch in transliterated.chars() {
        if ch.is_alphanumeric() {
            result.push(ch);
            last_was_space = false;
        } else if !last_was_space {
            result.push(' ');
            last_was_space = true;
        }
    }
    result.trim().to_string()
}

fn transliterate_cyrillic_to_latin(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    for ch in input.chars() {
        result.push_str(match ch {
            'а' | 'ә' => "a",
            'б' => "b",
            'в' => "v",
            'г' => "g",
            'ғ' => "g'",
            'д' => "d",
            'е' | 'э' => "e",
            'ё' => "yo",
            'ж' => "j",
            'з' => "z",
            'и' => "i",
            'й' => "y",
            'к' => "k",
            'қ' => "q",
            'л' => "l",
            'м' => "m",
            'н' => "n",
            'ң' => "ng",
            'о' | 'ө' => "o",
            'п' => "p",
            'р' => "r",
            'с' => "s",
            'т' => "t",
            'у' | 'ү' => "u",
            'ў' => "o'",
            'ф' => "f",
            'х' => "x",
            'ҳ' => "h",
            'ц' => "ts",
            'ч' => "ch",
            'ш' | 'щ' => "sh",
            'ъ' => "'",
            'ь' => "",
            'ю' => "yu",
            'я' => "ya",
            _ => {
                result.push(ch);
                continue;
            }
        });
    }
    result
}

fn search_skeleton(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !matches!(ch, 'a' | 'e' | 'i' | 'o' | 'u'))
        .collect()
}

fn normalized_cmp_key(value: &str) -> String {
    value.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_score_matches_go_priority_order() {
        assert_eq!(search_query_score("abc", &["abc"]), 1000);
        assert_eq!(search_query_score("abc", &["abcdef"]), 850);
        assert_eq!(search_query_score("bar", &["foo bar"]), 700);
        assert_eq!(search_query_score("bc", &["abc"]), 550);
        assert_eq!(search_query_score("a b", &["ab"]), 500);
        assert_eq!(search_query_score("bcd", &["bacod"]), 250);
    }

    #[test]
    fn search_score_prefers_earlier_fields_like_go() {
        assert_eq!(search_query_score("abc", &["", "abc"]), 990);
    }

    #[test]
    fn normalize_for_search_matches_go_transliteration_rules() {
        assert_eq!(normalize_for_search(" Ғўза Xom "), "goza hom");
        assert_eq!(normalize_for_search("A_B%'C"), "a b c");
    }

    #[test]
    fn rank_supplier_items_filters_scores_and_tie_breaks_like_go() {
        let items = vec![
            supplier_item("B", "Gamma"),
            supplier_item("A", "Alpha"),
            supplier_item("C", "Beta Alpha"),
        ];

        let ranked = rank_supplier_items_by_query(items, "alpha");

        assert_eq!(
            ranked
                .iter()
                .map(|item| item.code.as_str())
                .collect::<Vec<_>>(),
            ["A", "C"]
        );
    }

    #[test]
    fn rank_customer_options_prioritizes_item_score_then_customer_score() {
        let items = vec![
            customer_option("CUST-B", "Alpha Customer", "ITEM-2", "Milk"),
            customer_option("CUST-A", "Beta Customer", "ALPHA-ITEM", "Bread"),
            customer_option("CUST-C", "Alpha Customer", "ITEM-1", "Apple"),
        ];

        let ranked = rank_customer_item_options_by_query(items, "alpha");

        assert_eq!(ranked[0].item_code, "ALPHA-ITEM");
        assert_eq!(ranked[1].item_name, "Apple");
        assert_eq!(ranked[2].item_name, "Milk");
    }

    #[test]
    fn slice_page_matches_go_bounds() {
        assert_eq!(slice_page(&[1, 2, 3, 4], 1, 2), vec![2, 3]);
        assert_eq!(slice_page(&[1, 2, 3], 3, 2), Vec::<i32>::new());
        assert_eq!(slice_page(&[1, 2, 3], 1, 0), vec![2, 3]);
    }

    fn supplier_item(code: &str, name: &str) -> SupplierItem {
        SupplierItem {
            code: code.to_string(),
            name: name.to_string(),
            uom: "Kg".to_string(),
            warehouse: "Stores".to_string(),
            item_group: String::new(),
        }
    }

    fn customer_option(
        customer_ref: &str,
        customer_name: &str,
        item_code: &str,
        item_name: &str,
    ) -> CustomerItemOption {
        CustomerItemOption {
            customer_ref: customer_ref.to_string(),
            customer_name: customer_name.to_string(),
            customer_phone: String::new(),
            item_code: item_code.to_string(),
            item_name: item_name.to_string(),
            uom: "Kg".to_string(),
            warehouse: "Stores".to_string(),
        }
    }
}
