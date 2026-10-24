//! Resolve legacy customer labels without loading calculation bodies or images.
use super::*;
use crate::core::calculate_orders::{CalculateOrderTemplate, hydrate_template_dimensions};
use crate::core::formula::DEFAULT_EDGE_ALLOWANCE_MM;
use crate::core::production_map::ProductionMapDefinition;
use std::collections::BTreeSet;

/// The catalogue retains the map's own customer as the primary authority.
/// Missing labels use the same source -> order -> product/width precedence as
/// production-map customer resolution, including ambiguity and empty matches.
pub(super) async fn fill_customers(
    tx: &mut Transaction<'_, Postgres>,
    maps: &mut [ProductionMapDefinition],
) -> Result<(), PreparationError> {
    let missing = maps
        .iter()
        .filter(|map| needs_customer(map))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    let mut sources = BTreeSet::new();
    let mut orders = BTreeSet::new();
    let mut products = BTreeSet::new();
    for map in missing {
        let id = map.id.trim();
        sources.insert(id.to_string());
        sources.insert(format!("template-{id}"));
        orders.extend(order_keys(map).into_iter().map(str::to_string));
        products.extend(product_keys(map));
    }
    // Dimensions and product blocks are projected only to reproduce the
    // existing hydration of product names and widths. No layers, formula
    // bodies, photos or unrelated template fields leave PostgreSQL.
    let payloads = sqlx::query_scalar::<_, Value>(
        "SELECT jsonb_build_object(
            'source_map_id',COALESCE(t.payload_json->>'source_map_id',''),
            'order_number',COALESCE(t.payload_json->>'order_number',''),
            'code',COALESCE(t.payload_json->>'code',''),
            'customer',COALESCE(t.payload_json->>'customer',''),
            'item_code',COALESCE(t.payload_json->>'item_code',''),
            'product',COALESCE(t.payload_json->>'product',''),
            'width_mm',COALESCE(t.payload_json->'width_mm','0'::jsonb),
            'frame_product_size_mm',COALESCE(t.payload_json->'frame_product_size_mm','0'::jsonb),
            'frame_count',COALESCE(t.payload_json->'frame_count','0'::jsonb),
            'edge_allowance_mm',COALESCE(t.payload_json->'edge_allowance_mm',to_jsonb($4::double precision)),
            'products',COALESCE(t.payload_json->'products','[]'::jsonb))
         FROM mini_quick_order_templates t
         WHERE btrim(COALESCE(t.payload_json->>'source_map_id',''))=ANY($1::text[])
            OR btrim(COALESCE(t.payload_json->>'order_number',''))=ANY($2::text[])
            OR btrim(COALESCE(t.payload_json->>'code',''))=ANY($2::text[])
            OR lower(btrim(COALESCE(t.payload_json->>'item_code','')))=ANY($3::text[])
            OR lower(btrim(COALESCE(t.payload_json->>'product','')))=ANY($3::text[])
            OR lower(btrim(COALESCE(t.payload_json#>>'{products,0,item_code}','')))=ANY($3::text[])
            OR lower(btrim(COALESCE((
                SELECT string_agg(btrim(COALESCE(p->>'product','')),' + ' ORDER BY ordinal)
                FROM jsonb_array_elements(CASE
                    WHEN jsonb_typeof(t.payload_json->'products')='array'
                    THEN t.payload_json->'products' ELSE '[]'::jsonb END)
                    WITH ORDINALITY product(p,ordinal)
            ),'')))=ANY($3::text[])
         ORDER BY t.saved_at DESC",
    )
    .bind(sources.into_iter().collect::<Vec<_>>())
    .bind(orders.into_iter().collect::<Vec<_>>())
    .bind(products.into_iter().collect::<Vec<_>>())
    .bind(DEFAULT_EDGE_ALLOWANCE_MM)
    .fetch_all(&mut **tx)
    .await?;
    let templates = payloads
        .into_iter()
        .map(|value| {
            serde_json::from_value::<CalculateOrderTemplate>(value).map(hydrate_template_dimensions)
        })
        .collect::<Result<Vec<_>, _>>();
    let templates = match templates {
        Ok(templates) => templates,
        Err(error) => {
            // Match the existing fallback's failure behavior: a corrupt
            // calculation cannot make otherwise valid order rows disappear.
            tracing::warn!(%error, "preparation order customer metadata is invalid");
            return Ok(());
        }
    };
    apply_customers(maps, &templates);
    Ok(())
}

fn normalized(value: &str) -> String {
    value.trim().to_lowercase()
}

fn needs_customer(map: &ProductionMapDefinition) -> bool {
    let id = map.id.trim();
    map.customer_name.trim().is_empty()
        && !id.is_empty()
        && !id.starts_with("template-")
        && (id.starts_with("zakaz-")
            || !map.order_number.trim().is_empty()
            || !map.code.trim().is_empty())
}

fn order_keys(map: &ProductionMapDefinition) -> BTreeSet<&str> {
    [
        map.order_number.trim(),
        map.code.trim(),
        map.id.trim().strip_prefix("zakaz-").unwrap_or("").trim(),
    ]
    .into_iter()
    .filter(|key| !key.is_empty())
    .collect()
}

fn product_keys(map: &ProductionMapDefinition) -> BTreeSet<String> {
    [map.product_code.as_str(), map.title.as_str()]
        .into_iter()
        .chain(map.nodes.iter().map(|node| node.item_code.as_str()))
        .map(normalized)
        .filter(|key| !key.is_empty())
        .collect()
}

fn resolve_customer(
    map: &ProductionMapDefinition,
    templates: &[CalculateOrderTemplate],
) -> Option<String> {
    let id = map.id.trim();
    let mut source = templates
        .iter()
        .filter(|template| {
            let source = template.source_map_id.trim();
            source == id || (!id.is_empty() && source.strip_prefix("template-") == Some(id))
        })
        .peekable();
    if source.peek().is_some() {
        return unique_customer(source);
    }
    let keys = order_keys(map);
    let mut orders = templates
        .iter()
        .filter(|template| {
            !keys.is_empty()
                && (keys.contains(template.order_number.trim())
                    || keys.contains(template.code.trim()))
        })
        .peekable();
    if orders.peek().is_some() {
        return unique_customer(orders);
    }
    let products = product_keys(map);
    if products.is_empty() {
        return None;
    }
    unique_customer(templates.iter().filter(|template| {
        if !products.contains(&normalized(&template.product))
            && !products.contains(&normalized(&template.item_code))
        {
            return false;
        }
        match (map.width_mm, template.width_mm) {
            (Some(width), other) if width > 0.0 && other > 0.0 => (width - other).abs() <= 0.5,
            _ => true,
        }
    }))
}

fn unique_customer<'a>(
    templates: impl Iterator<Item = &'a CalculateOrderTemplate>,
) -> Option<String> {
    let mut key = None;
    let mut customer = None;
    for template in templates {
        let value = template.customer.trim();
        if value.is_empty() {
            continue;
        }
        let normalized = normalized(value);
        if key.as_ref().is_some_and(|existing| existing != &normalized) {
            return None;
        }
        key = Some(normalized);
        customer = Some(value.to_string());
    }
    customer
}

fn apply_customers(maps: &mut [ProductionMapDefinition], templates: &[CalculateOrderTemplate]) {
    for map in maps {
        if needs_customer(map) {
            if let Some(customer) = resolve_customer(map, templates) {
                map.customer_name = customer;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> ProductionMapDefinition {
        serde_json::from_value(
            json!({"id":"zakaz-8768","code":"Z8768","order_number":"8768",
            "product_code":"FILM","title":"Film","width_mm":100}),
        )
        .unwrap()
    }

    fn template(customer: &str) -> CalculateOrderTemplate {
        CalculateOrderTemplate {
            customer: customer.into(),
            ..Default::default()
        }
    }

    #[test]
    fn preparation_order_customers_preserve_legacy_source_precedence_and_ambiguity() {
        let mut source = template(" Source Customer ");
        source.source_map_id = "template-zakaz-8768".into();
        let mut order = template("Order Customer");
        order.order_number = "8768".into();
        assert_eq!(
            resolve_customer(&map(), &[source.clone(), order.clone()]).as_deref(),
            Some("Source Customer")
        );
        let mut conflict = template("Conflicting customer");
        conflict.source_map_id = "zakaz-8768".into();
        assert_eq!(
            resolve_customer(&map(), &[source, conflict, order.clone()]),
            None
        );
        let mut blank = template("");
        blank.source_map_id = "zakaz-8768".into();
        assert_eq!(
            resolve_customer(&map(), &[blank, order]),
            None,
            "an existing empty source must not borrow a lower-priority customer's label"
        );
    }

    #[test]
    fn preparation_order_customers_match_all_order_keys_before_product_fallback() {
        for key in ["8768", "Z8768"] {
            let mut order = template("Order Customer");
            order.code = key.into();
            let mut product = template("Product Customer");
            product.product = "film".into();
            assert_eq!(
                resolve_customer(&map(), &[order, product]).as_deref(),
                Some("Order Customer")
            );
        }
        let mut suffix_map = map();
        suffix_map.order_number.clear();
        suffix_map.code.clear();
        let mut order = template("Suffix Customer");
        order.order_number = "8768".into();
        assert_eq!(
            resolve_customer(&suffix_map, &[order]).as_deref(),
            Some("Suffix Customer")
        );
    }

    #[test]
    fn preparation_order_customers_product_fallback_checks_width_and_refuses_conflicts() {
        let mut matched = template(" Customer A ");
        matched.item_code = " FiLm ".into();
        matched.width_mm = 100.5;
        let mut wrong_width = template("Customer B");
        wrong_width.product = "Film".into();
        wrong_width.width_mm = 100.51;
        assert_eq!(
            resolve_customer(&map(), &[matched.clone(), wrong_width.clone()]).as_deref(),
            Some("Customer A")
        );
        wrong_width.width_mm = 100.;
        assert_eq!(
            resolve_customer(&map(), &[matched.clone(), wrong_width]),
            None
        );
        let mut same = template("customer a");
        same.product = "film".into();
        assert_eq!(
            resolve_customer(&map(), &[matched, same]).as_deref(),
            Some("customer a")
        );
    }

    #[test]
    fn preparation_order_customers_keep_canonical_labels_and_match_node_item_codes() {
        let mut node_map = map();
        node_map.product_code = "unrelated".into();
        node_map.title = "unrelated".into();
        node_map.nodes = serde_json::from_value(json!([
            {"id":"material","kind":"material","title":"Material","item_code":" PE "}
        ]))
        .unwrap();
        let mut product = template("Node Customer");
        product.item_code = "pe".into();
        let mut canonical = map();
        canonical.customer_name = "Canonical Customer".into();
        let mut maps = [node_map, canonical];
        apply_customers(&mut maps, &[product]);
        assert_eq!(maps[0].customer_name, "Node Customer");
        assert_eq!(maps[1].customer_name, "Canonical Customer");
    }
}
