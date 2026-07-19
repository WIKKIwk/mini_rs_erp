use std::collections::BTreeSet;

use super::models::AdminItemGroup;

pub(crate) const FINISHED_GOODS_GROUP: &str = "tayyor mahsulot";
pub(crate) const FINISHED_GOODS_CUSTOMER_REQUIRED: &str =
    "tayyor mahsulot uchun kamida bitta customer kerak";

pub(crate) fn item_group_requires_customer(item_group: &str, groups: &[AdminItemGroup]) -> bool {
    let mut current = item_group.trim();
    let mut seen = BTreeSet::new();
    while !current.is_empty() && seen.insert(current.to_ascii_lowercase()) {
        if current.eq_ignore_ascii_case(FINISHED_GOODS_GROUP) {
            return true;
        }
        let Some(group) = find_group(groups, current) else {
            break;
        };
        current = group.parent_item_group.trim();
    }
    false
}

pub(crate) fn item_group_is_descendant_of(
    item_group: &str,
    ancestor: &str,
    groups: &[AdminItemGroup],
) -> bool {
    let ancestor = ancestor.trim();
    if ancestor.is_empty() {
        return false;
    }
    let mut current = item_group.trim();
    let mut seen = BTreeSet::new();
    while !current.is_empty() && seen.insert(current.to_ascii_lowercase()) {
        if current.eq_ignore_ascii_case(ancestor) {
            return true;
        }
        let Some(group) = find_group(groups, current) else {
            break;
        };
        current = group.parent_item_group.trim();
    }
    false
}

fn find_group<'a>(groups: &'a [AdminItemGroup], name: &str) -> Option<&'a AdminItemGroup> {
    groups.iter().find(|group| {
        group.item_group_name.trim().eq_ignore_ascii_case(name)
            || group.name.trim().eq_ignore_ascii_case(name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(name: &str, parent: &str) -> AdminItemGroup {
        AdminItemGroup {
            name: name.to_string(),
            item_group_name: name.to_string(),
            parent_item_group: parent.to_string(),
            is_group: true,
        }
    }

    #[test]
    fn finished_goods_rule_follows_exact_ancestor() {
        let groups = vec![
            group("All Item Groups", ""),
            group("Tayyor mahsulot", "All Item Groups"),
            group("Paketlar", "Tayyor mahsulot"),
        ];

        assert!(item_group_requires_customer("tayyor mahsulot", &groups));
        assert!(item_group_requires_customer("Paketlar", &groups));
        assert!(item_group_is_descendant_of(
            "Paketlar",
            "Tayyor mahsulot",
            &groups
        ));
    }

    #[test]
    fn similar_words_do_not_make_a_group_finished_goods() {
        let groups = vec![
            group("All Item Groups", ""),
            group("Yarim tayyor mahsulot", "All Item Groups"),
        ];

        assert!(!item_group_requires_customer(
            "Yarim tayyor mahsulot",
            &groups
        ));
    }

    #[test]
    fn cycles_stop_without_classifying_the_group() {
        let groups = vec![group("A", "B"), group("B", "A")];

        assert!(!item_group_requires_customer("A", &groups));
        assert!(!item_group_is_descendant_of("A", "C", &groups));
    }
}
