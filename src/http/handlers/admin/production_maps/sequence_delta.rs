//! Bounded, linear sequence edits for `state_delta_v2`.
//!
//! Copy operations reference the immutable previous list, never an already
//! modified result. A move therefore needs only a few integer ranges even for
//! a large queue. Dense changes fall back to the existing full-list upsert.
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub(crate) fn fingerprint(ids: &[String]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for id in ids {
        // 0xff cannot occur in UTF-8, so boundaries remain unambiguous.
        for byte in id.bytes().chain(std::iter::once(0xff)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

/// Returns an edit only when it is smaller than the full replacement array.
/// All work and allocated operation storage are O(before.len + after.len).
pub(crate) fn sequence_edit(before: &[String], after: &[String]) -> Option<Value> {
    if before == after || before.is_empty() || after.is_empty() {
        return None;
    }
    let mut positions = HashMap::with_capacity(before.len());
    for (index, id) in before.iter().enumerate() {
        if id.is_empty() || positions.insert(id.as_str(), index).is_some() {
            return None;
        }
    }
    let mut seen = HashSet::with_capacity(after.len());
    let mut ops = Vec::new();
    let mut cursor = 0;
    while cursor < after.len() {
        let id = &after[cursor];
        if id.is_empty() || !seen.insert(id.as_str()) {
            return None;
        }
        if let Some(&start) = positions.get(id.as_str()) {
            let mut count = 1;
            while cursor + count < after.len()
                && before.get(start + count) == after.get(cursor + count)
            {
                if !seen.insert(after[cursor + count].as_str()) {
                    return None;
                }
                count += 1;
            }
            ops.push(json!([0, start, count]));
            cursor += count;
        } else {
            let start = cursor;
            cursor += 1;
            while cursor < after.len() && !positions.contains_key(after[cursor].as_str()) {
                if after[cursor].is_empty() || !seen.insert(after[cursor].as_str()) {
                    return None;
                }
                cursor += 1;
            }
            ops.push(json!([1, &after[start..cursor]]));
        }
    }
    let edit = json!({
        "base_len": before.len(), "base_hash": fingerprint(before),
        "len": after.len(), "hash": fingerprint(after), "ops": ops,
    });
    let edit_len = serde_json::to_vec(&edit)
        .expect("sequence edit serialization")
        .len();
    // Most identifiers need no JSON escaping. Prove the compact win from a
    // cheap lower bound instead of serializing/allocating the entire old wire
    // list merely to measure it on every reorder.
    let replacement_lower_bound =
        2 + after.len().saturating_sub(1) + after.iter().map(|id| id.len() + 2).sum::<usize>();
    if edit_len < replacement_lower_bound {
        return Some(edit);
    }
    struct CountBytes(usize);
    impl std::io::Write for CountBytes {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut replacement = CountBytes(0);
    serde_json::to_writer(&mut replacement, after).expect("sequence serialization");
    (edit_len < replacement.0).then_some(edit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(size: usize) -> Vec<String> {
        (0..size).map(|i| format!("order-{i:06}")).collect()
    }

    fn apply(before: &[String], edit: &Value) -> Vec<String> {
        assert_eq!(edit["base_len"], before.len());
        assert_eq!(edit["base_hash"], fingerprint(before));
        let mut after = Vec::new();
        for op in edit["ops"].as_array().unwrap() {
            match op[0].as_u64().unwrap() {
                0 => {
                    let start = op[1].as_u64().unwrap() as usize;
                    let len = op[2].as_u64().unwrap() as usize;
                    after.extend_from_slice(&before[start..start + len]);
                }
                1 => after.extend(
                    op[1]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_str().unwrap().into()),
                ),
                _ => panic!("unknown operation"),
            }
        }
        assert_eq!(edit["len"], after.len());
        assert_eq!(edit["hash"], fingerprint(&after));
        after
    }

    #[test]
    fn a_single_move_of_ten_thousand_ids_needs_four_ranges() {
        let before = ids(10_000);
        let mut after = before.clone();
        let moved = after.remove(9_998);
        after.insert(1, moved);
        let edit = sequence_edit(&before, &after).unwrap();
        assert_eq!(edit["ops"].as_array().unwrap().len(), 4);
        assert!(serde_json::to_vec(&edit).unwrap().len() < 200);
        assert_eq!(apply(&before, &edit), after);
    }

    #[test]
    fn mixed_insert_remove_move_and_unicode_round_trip() {
        let before = ids(1_000);
        let mut after = before.clone();
        after.remove(41);
        let moved = after.remove(800);
        after.insert(2, moved);
        after.splice(60..60, ["yangi-ўзбек-😀".into(), "new-id".into()]);
        let edit = sequence_edit(&before, &after).unwrap();
        assert_eq!(apply(&before, &edit), after);
    }

    #[test]
    fn small_dense_empty_duplicate_and_unchanged_lists_use_legacy_replacement() {
        assert!(sequence_edit(&ids(3), &ids(3)).is_none());
        assert!(sequence_edit(&[], &ids(3)).is_none());
        assert!(sequence_edit(&ids(3), &[]).is_none());
        assert!(sequence_edit(&ids(3), &["different".into()]).is_none());
        assert!(sequence_edit(&ids(100), &vec!["same".into(); 100]).is_none());
        let duplicate = vec!["same".into(); 100];
        assert!(sequence_edit(&duplicate, &ids(100)).is_none());
    }

    #[test]
    fn fingerprints_encode_boundaries_order_and_unicode() {
        assert_eq!(fingerprint(&[]), "cbf29ce484222325");
        assert_ne!(
            fingerprint(&["ab".into(), "c".into()]),
            fingerprint(&["a".into(), "bc".into()])
        );
        assert_ne!(
            fingerprint(&ids(2)),
            fingerprint(&ids(2).into_iter().rev().collect::<Vec<_>>())
        );
        // Shared with the Dart reducer regression fixtures.
        assert_eq!(
            fingerprint(&["order-000000".into(), "ўзбек-😀".into()]),
            "054688fb8e618984"
        );
    }
}
