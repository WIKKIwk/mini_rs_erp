use serde::{Deserialize, Serialize};

use super::calculate_orders::CalculateOrderTemplate;
use super::production_map::ProductionMapDefinition;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderEditSource {
    pub map: ProductionMapDefinition,
    pub template: CalculateOrderTemplate,
    pub revision: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum OrderEditError {
    #[error("Buyurtma topilmadi")]
    NotFound,
    #[error("{0}")]
    Locked(&'static str),
    #[error("Buyurtma o‘zgargan. Sahifani qayta oching")]
    Conflict,
    #[error("{0}")]
    Invalid(String),
    #[error("Buyurtmani tahrirlash ma’lumotlari yuklanmadi")]
    Store,
}

pub(crate) fn check_queue_position(
    order_id: &str,
    sequences: &std::collections::BTreeMap<String, Vec<String>>,
    states: &std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<
            String,
            super::production_map::queue_state::ApparatusQueueOrderState,
        >,
    >,
) -> Result<(), OrderEditError> {
    use super::production_map::queue_state::first_actionable_order_id;
    for (apparatus, sequence) in sequences {
        let empty = std::collections::BTreeMap::new();
        if sequence.first().is_some_and(|id| id == order_id)
            || first_actionable_order_id(sequence, states.get(apparatus).unwrap_or(&empty))
                == Some(order_id)
        {
            return Err(OrderEditError::Locked(
                "Buyurtma apparat navbatida birinchi: tahrirlash mumkin emas",
            ));
        }
    }
    Ok(())
}

impl From<super::production_map::ProductionMapError> for OrderEditError {
    fn from(error: super::production_map::ProductionMapError) -> Self {
        use super::production_map::ProductionMapError;
        match error {
            ProductionMapError::MapNotFound => Self::NotFound,
            ProductionMapError::StoreFailed => Self::Store,
            _ => Self::Invalid(error.to_string()),
        }
    }
}

/// The edit changes calculation inputs, never the route. Reject inputs that
/// require a different operation or cannot fit any existing route candidate.
pub(crate) fn validate_route(
    original: &CalculateOrderTemplate,
    template: &CalculateOrderTemplate,
    map: &ProductionMapDefinition,
    apparatus: &[super::apparatus_standard::RuntimeApparatusConfiguration],
) -> Result<(), OrderEditError> {
    use super::apparatus_standard::{
        ExecutionOperation as Operation, ProcessTechnology as Technology,
    };
    use super::production_map::{ProductionMapNodeKind, automatic, pechat};
    let invalid = || {
        OrderEditError::Invalid(
            "Yangi Calculate qiymatlari mavjud production map bilan mos emas".into(),
        )
    };
    if original.status.trim().to_lowercase() != template.status.trim().to_lowercase()
        || (original.effective_layers().len() > 1) != (template.effective_layers().len() > 1)
        || original.production_options != template.production_options
        || !template.frame_count.is_finite()
        || template.frame_count <= 0.0
        || template.frame_count.fract() != 0.0
        || template.frame_count > 1024.0
        || template.roll_count.is_none_or(|count| count <= 0)
    {
        return Err(invalid());
    }
    if original.item_code != template.item_code
        && map
            .nodes
            .iter()
            .any(|node| !node.item_code.trim().is_empty())
    {
        return Err(OrderEditError::Invalid(
            "Mapdagi mahsulot bog‘lanishlari yangi mahsulotga mos emas".into(),
        ));
    }
    let catalog = apparatus
        .iter()
        .map(|a| (a.runtime.apparatus_id.clone(), a))
        .collect::<std::collections::BTreeMap<_, _>>();
    let cut_ids = apparatus
        .iter()
        .filter(|a| a.runtime.execution_profile.operation == Operation::Cut)
        .map(|a| a.runtime.apparatus_id.clone())
        .collect();
    for node in map
        .nodes
        .iter()
        .filter(|n| n.kind == ProductionMapNodeKind::Apparatus)
    {
        let id = node.canonical_apparatus_id().ok_or_else(invalid)?;
        let canonical = catalog.get(&id).ok_or_else(invalid)?;
        if !canonical.has_coherent_source() || !canonical.is_active() {
            return Err(invalid());
        }
        let profile = &canonical.runtime.execution_profile;
        if profile.operation == Operation::Cut
            && !node.rezka_frame_groups.is_empty()
            && (node.rezka_frame_groups.iter().any(|n| *n <= 0)
                || node
                    .rezka_frame_groups
                    .iter()
                    .try_fold(0_i64, |sum, n| sum.checked_add(*n))
                    != node.rezka_kadr_count)
        {
            return Err(invalid());
        }
        if profile.operation == Operation::Print {
            if let Some(options) = &template.production_options {
                let technology = match options.print_method {
                    automatic::PrintMethod::Flexo => Technology::Flexographic,
                    automatic::PrintMethod::Metal => Technology::Rotogravure,
                };
                if profile.technology != technology {
                    return Err(invalid());
                }
            }
            let width = template
                .print_val_size_mm
                .unwrap_or(template.frame_product_size_mm * template.frame_count);
            if !pechat::apparatus_profile_can_handle_order(
                canonical,
                template.roll_count,
                Some(width),
            ) {
                return Err(invalid());
            }
            if profile.technology != Technology::Flexographic {
                let colors = pechat::pechat_color_stations(canonical).ok_or_else(invalid)?;
                if !pechat::pechat_can_handle_order(
                    colors,
                    template.roll_count,
                    template.print_val_size_mm.or(map.width_mm),
                ) {
                    return Err(invalid());
                }
            }
        } else {
            let widths = automatic::stage_input_widths(map, &node.id, &cut_ids);
            if widths.is_empty()
                || widths.into_iter().any(|width| {
                    !pechat::apparatus_profile_can_handle_order(canonical, None, Some(width))
                        || (profile.operation == Operation::Laminate
                            && !automatic::lamination_fits(canonical, width))
                })
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

pub(crate) fn calculation_map_changed(
    before: &ProductionMapDefinition,
    after: &ProductionMapDefinition,
) -> bool {
    before.product_code != after.product_code
        || before.title != after.title
        || before.customer_name != after.customer_name
        || before.roll_count != after.roll_count
        || before.width_mm != after.width_mm
        || before.print_val_size_mm != after.print_val_size_mm
        || before.order_kg != after.order_kg
        || before.base_length != after.base_length
}

#[cfg(test)]
mod tests;
