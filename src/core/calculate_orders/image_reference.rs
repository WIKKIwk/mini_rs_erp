use std::collections::BTreeSet;

use super::CalculateOrderTemplate;
use crate::core::production_map::ProductionMapDefinition;

/// The same legacy-image rules apply to every viewer. Database adapters only
/// narrow the candidates; identity precedence and ambiguity live here.
pub struct OrderImageLookup {
    pub source_ids: Vec<String>,
    pub order_keys: Vec<String>,
    pub product_keys: Vec<String>,
    width_mm: Option<f64>,
}

impl OrderImageLookup {
    pub fn for_map(map: &ProductionMapDefinition) -> Self {
        let map_id = map.id.trim();
        let mut source_ids = Vec::new();
        if !map_id.is_empty() {
            source_ids.push(map_id.to_string());
            if !map_id.starts_with("template-") {
                source_ids.push(format!("template-{map_id}"));
            }
        }
        let order_keys = [map.order_number.trim(), map.code.trim(),
            map_id.strip_prefix("zakaz-").unwrap_or("").trim()]
            .into_iter().filter(|key| !key.is_empty()).map(str::to_owned)
            .collect::<BTreeSet<_>>().into_iter().collect();
        let product_keys = [map.product_code.as_str(), map.title.as_str()]
            .into_iter().chain(map.nodes.iter().map(|node| node.item_code.as_str()))
            .map(normalize).filter(|key| !key.is_empty())
            .collect::<BTreeSet<_>>().into_iter().collect();
        Self { source_ids, order_keys, product_keys, width_mm: map.width_mm }
    }

    fn rank(&self, template: &CalculateOrderTemplate) -> Option<usize> {
        if let Some(rank) = self.source_ids.iter()
            .position(|id| id == template.source_map_id.trim()) {
            return Some(rank);
        }
        if self.order_keys.iter().any(|key|
            key == template.order_number.trim() || key == template.code.trim()) {
            return Some(2);
        }
        if !self.product_keys.contains(&normalize(&template.item_code))
            && !self.product_keys.contains(&normalize(&template.product)) {
            return None;
        }
        if let Some(width) = self.width_mm.filter(|width| *width > 0.0) {
            if template.width_mm > 0.0 && (template.width_mm - width).abs() > 0.5 {
                return None;
            }
        }
        Some(3)
    }

    pub fn resolve(&self, templates: &[CalculateOrderTemplate]) -> Option<String> {
        let best = templates.iter().filter_map(|template| self.rank(template)).min()?;
        let images = templates.iter()
            .filter(|template| self.rank(template) == Some(best))
            .map(|template| template.image_id.trim()).filter(|id| !id.is_empty())
            .collect::<BTreeSet<_>>();
        // A stronger matching template with no image must not borrow another
        // product's photo. Conflicting photos likewise require an explicit link.
        (images.len() == 1).then(|| images.into_iter().next().unwrap().to_string())
    }
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup() -> OrderImageLookup {
        OrderImageLookup::for_map(&serde_json::from_value(serde_json::json!({
            "id":"zakaz-0012", "order_number":"0012", "code":"0012",
            "product_code":"mono 007", "title":"Mono 007", "width_mm":41
        })).unwrap())
    }

    fn candidate(image: &str, width: f64) -> CalculateOrderTemplate {
        CalculateOrderTemplate { item_code: " MONO 007 ".into(),
            image_id: image.into(), width_mm: width, ..Default::default() }
    }

    #[test]
    fn order_image_product_fallback_checks_width_and_refuses_conflicts() {
        let right = candidate("right", 41.0);
        assert_eq!(lookup().resolve(&[candidate("wrong-width", 813.0), right.clone()]).as_deref(), Some("right"));
        assert_eq!(lookup().resolve(&[right.clone(), candidate("different", 41.0)]), None);
        assert_eq!(lookup().resolve(&[right.clone(), right]).as_deref(), Some("right"));
    }

    #[test]
    fn order_image_exact_link_wins_and_empty_exact_does_not_borrow_product_photo() {
        let mut exact = candidate("exact", 813.0);
        exact.source_map_id = "template-zakaz-0012".into();
        assert_eq!(lookup().resolve(&[candidate("product", 41.0), exact.clone()]).as_deref(), Some("exact"));
        exact.image_id.clear();
        assert_eq!(lookup().resolve(&[candidate("product", 41.0), exact]), None);
    }
}
