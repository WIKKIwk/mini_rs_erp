use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[cfg(test)]
pub(crate) mod tests;

/// An ordered product block on one customer's common printing layout.
/// `id` is stable within the layout; the production order namespaces it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OrderProduct {
    pub id: String,
    pub item_code: String,
    pub product: String,
    pub frame_product_size_mm: f64,
    pub frame_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderLane {
    pub id: String,
    pub product_id: String,
    pub order_number: String,
    pub item_code: String,
    pub product: String,
    pub width_mm: f64,
}

pub fn validate_products(products: &[OrderProduct]) -> Result<(), String> {
    if products.len() > 64 {
        return Err("Ko‘pi bilan 64 ta mahsulot bloki kiritish mumkin".into());
    }
    let mut ids = BTreeSet::new();
    let mut frames = 0_i64;
    for (index, product) in products.iter().enumerate() {
        if product.id.is_empty()
            || product.id.len() > 100
            || !product
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
            || !ids.insert(&product.id)
            || product.item_code.trim().is_empty()
            || product.product.trim().is_empty()
        {
            return Err(format!("{}-mahsulotni tanlang", index + 1));
        }
        if !product.frame_product_size_mm.is_finite()
            || product.frame_product_size_mm <= 0.0
            || !(1..=1024).contains(&product.frame_count)
        {
            return Err(format!(
                "{}-mahsulotning kadr eni yoki soni noto‘g‘ri",
                index + 1
            ));
        }
        frames += product.frame_count;
    }
    if frames > 1024 || !useful_width(products).is_finite() {
        return Err("Jami kadr soni 1024 dan oshmasligi va en to‘g‘ri bo‘lishi kerak".into());
    }
    Ok(())
}

pub fn useful_width(products: &[OrderProduct]) -> f64 {
    products
        .iter()
        .map(|p| p.frame_product_size_mm * p.frame_count as f64)
        .sum()
}

pub fn frame_count(products: &[OrderProduct]) -> i64 {
    products
        .iter()
        .fold(0_i64, |sum, p| sum.saturating_add(p.frame_count))
}

pub fn title(products: &[OrderProduct]) -> String {
    products
        .iter()
        .map(|p| p.product.trim())
        .collect::<Vec<_>>()
        .join(" + ")
}

pub fn lanes(products: &[OrderProduct], order_id: &str, order_number: &str) -> Vec<OrderLane> {
    products
        .iter()
        .enumerate()
        .flat_map(|(index, product)| {
            (0..product.frame_count).map(move |frame| OrderLane {
                id: format!("{}:{}", product.id, frame + 1),
                product_id: format!("{order_id}:product:{}", product.id),
                order_number: format!("{order_number}/{}", index + 1),
                item_code: product.item_code.clone(),
                product: product.product.clone(),
                width_mm: product.frame_product_size_mm,
            })
        })
        .collect()
}

/// Groups are consecutive physical lanes, never an average of their widths.
pub fn group_widths(lanes: &[f64], groups: &[i64]) -> Option<Vec<f64>> {
    let groups = if groups.is_empty() {
        vec![1; lanes.len()]
    } else {
        groups.to_vec()
    };
    let mut offset = 0_usize;
    let mut widths = Vec::with_capacity(groups.len());
    for count in groups {
        let count = usize::try_from(count).ok().filter(|n| *n > 0)?;
        let end = offset.checked_add(count)?;
        widths.push(lanes.get(offset..end)?.iter().sum());
        offset = end;
    }
    (offset == lanes.len()).then_some(widths)
}
