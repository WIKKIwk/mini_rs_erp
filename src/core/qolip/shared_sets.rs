use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::QolipProductSpec;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QolipSetProduct {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QolipSetSnapshot {
    pub set_id: String,
    /// Stable public set code, independent of internal IDs and member codes.
    pub display_code: String,
    pub fingerprint: String,
    pub members: Vec<QolipProductSpec>,
    pub products: Vec<QolipSetProduct>,
}

impl QolipSetSnapshot {
    pub fn new(
        set_id: String,
        display_code: String,
        mut members: Vec<QolipProductSpec>,
        products: Vec<QolipSetProduct>,
    ) -> Self {
        members.sort_by_key(|spec| spec.qolip_code.trim().to_lowercase());
        let content =
            serde_json::to_vec(&(&set_id, &display_code, &members)).expect("serializable mold set");
        let fingerprint = format!("{:x}", Sha256::digest(content));
        Self {
            set_id,
            display_code,
            fingerprint,
            members,
            products,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct QolipSetReuse {
    pub set_id: String,
    pub item_code: String,
    pub fingerprint: String,
}

pub(crate) fn candidate_codes(identifier: &str) -> Vec<String> {
    let Some((prefix, suffix)) = identifier.trim().rsplit_once('-') else {
        return Vec::new();
    };
    let Ok(count) = suffix.parse::<usize>() else {
        return Vec::new();
    };
    if prefix.is_empty() || !(1..=100).contains(&count) {
        return Vec::new();
    }
    (1..=count)
        .map(|index| format!("{prefix}-{index:0width$}", width = suffix.len()))
        .collect()
}
