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
    #[error("Buyurtma bazada topilmadi. Buyurtmalar ro‘yxatini yangilang")]
    NotFound,
    #[error("{0}")]
    Locked(&'static str),
    #[error(
        "Tahrirlash oynasi ochilgandan keyin buyurtma ma’lumotlari o‘zgargan. Eski ma’lumotlar bilan saqlash bloklandi. Buyurtmani qayta ochib, o‘zgarishlarni yangidan kiriting"
    )]
    Conflict,
    #[error("{0}")]
    Invalid(String),
    #[error("{message}")]
    Storage {
        code: &'static str,
        message: String,
    },
    #[error(
        "Server buyurtmaning tahrirlash ma’lumotlarini o‘qiy yoki saqlay olmadi. Bu kiritgan ma’lumotlaringizdagi xato emas. Buyurtmani qayta ochib holatini tekshiring; muammo takrorlansa mas’ul administratorga murojaat qiling"
    )]
    Store,
}

pub(crate) fn check_queue_position(
    map: &ProductionMapDefinition,
    sequences: &std::collections::BTreeMap<String, Vec<String>>,
    states: &std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<
            String,
            super::production_map::queue_state::ApparatusQueueOrderState,
        >,
    >,
) -> Result<(), OrderEditError> {
    use super::production_map::{chain, queue_state::first_actionable_order_id};
    // Only entry stages can start this untouched order. A downstream queue
    // head still waits for its physical predecessors and must not block edits.
    let initial_apparatuses = chain::linear_work_stages(map)
        .into_iter()
        .filter(|stage| chain::previous_work_stages_for_node(map, &stage.node_id).is_empty())
        .filter_map(|stage| stage.apparatus_id)
        .collect::<std::collections::BTreeSet<_>>();
    let order_id = map.id.as_str();
    for (apparatus, sequence) in sequences {
        if !initial_apparatuses.contains(apparatus) {
            continue;
        }
        let empty = std::collections::BTreeMap::new();
        if sequence.first().is_some_and(|id| id == order_id)
            || first_actionable_order_id(sequence, states.get(apparatus).unwrap_or(&empty))
                == Some(order_id)
        {
            return Err(OrderEditError::Locked(
                "Buyurtma production mapdagi boshlang‘ich apparat navbatida birinchi yoki ishga tushirish uchun birinchi hisoblanadi. Boshlang‘ich bosqichdagi navbatini tekshiring. Faqat birinchi bo‘lmagan va hech qanday harakat boshlanmagan buyurtmani tahrirlash mumkin",
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
    let invalid = |reason: &str| OrderEditError::Invalid(reason.into());
    if original.status.trim().to_lowercase() != template.status.trim().to_lowercase() {
        return Err(invalid(
            "Buyurtma turi o‘zgartirilgan. Mavjud production mapni saqlash uchun Paket/Rulon/Flexo turini avvalgi qiymatiga qaytaring",
        ));
    }
    if (original.effective_layers().len() > 1) != (template.effective_layers().len() > 1) {
        return Err(invalid(
            "Material qatlamlari o‘zgarishi laminatsiya yo‘nalishini o‘zgartirishni talab qiladi. Mavjud production map bilan tahrirlash uchun bir qatlamli/ko‘p qatlamli tuzilishni avvalgidek qoldiring",
        ));
    }
    if original.production_options != template.production_options {
        return Err(invalid(
            "Bosma usuli, sovuq yelim yoki diametr sozlamalari o‘zgartirilgan. Ular mavjud production mapga bog‘langan. Ushbu sozlamalarni avvalgi qiymatlariga qaytaring",
        ));
    }
    if !template.frame_count.is_finite()
        || template.frame_count <= 0.0
        || template.frame_count.fract() != 0.0
        || template.frame_count > 1024.0
    {
        return Err(invalid(
            "Kadr soni noto‘g‘ri. 1 dan 1024 gacha butun son kiriting",
        ));
    }
    if template.roll_count.is_none_or(|count| count <= 0) {
        return Err(invalid(
            "Rang soni kiritilmagan yoki 0 dan katta emas. 0 dan katta rang sonini kiriting",
        ));
    }
    if original.item_code != template.item_code
        && map
            .nodes
            .iter()
            .any(|node| !node.item_code.trim().is_empty())
    {
        return Err(OrderEditError::Invalid(
            "Production map bosqichlari avvalgi mahsulotga bog‘langan. Boshqa mahsulot tanlab saqlab bo‘lmaydi; avvalgi mahsulotni qayta tanlang".into(),
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
        let stage_error = |reason: &str| invalid(&format!("«{}» bosqichi: {reason}", node.title));
        let id = node.canonical_apparatus_id().ok_or_else(|| stage_error("apparat identifikatori yo‘q. Mas’ul administrator production mapdagi apparat bog‘lanishini tekshirishi kerak"))?;
        let canonical = catalog.get(&id).ok_or_else(|| stage_error("apparat katalogdan topilmadi. Mas’ul administrator apparat sozlamalarini tekshirishi kerak"))?;
        if !canonical.has_coherent_source() || !canonical.is_active() {
            return Err(stage_error(
                "apparat faol emas yoki sozlamalari mos emas. Mas’ul administrator apparat sozlamalarini tekshirishi kerak",
            ));
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
            return Err(stage_error(
                "Rezka kadr guruhlari jami bosqichdagi kadr soniga teng emas. Mas’ul administrator production mapdagi Rezka bo‘linishini tekshirishi kerak",
            ));
        }
        if profile.operation == Operation::Print {
            if let Some(options) = &template.production_options {
                let technology = match options.print_method {
                    automatic::PrintMethod::Flexo => Technology::Flexographic,
                    automatic::PrintMethod::Metal => Technology::Rotogravure,
                };
                if profile.technology != technology {
                    return Err(stage_error(
                        "tanlangan bosma usuli bu apparatga mos emas. Bosma usulini avvalgi qiymatiga qaytaring",
                    ));
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
                return Err(stage_error(
                    "kiritilgan rang soni yoki bosma eni apparat imkoniyatiga mos emas. Rang soni, val o‘lchami va kadr enini tekshiring",
                ));
            }
            if profile.technology != Technology::Flexographic {
                let colors = pechat::pechat_color_stations(canonical).ok_or_else(|| stage_error("apparatning rang stansiyalari sozlanmagan. Mas’ul administrator apparat sozlamalarini tekshirishi kerak"))?;
                if !pechat::pechat_can_handle_order(
                    colors,
                    template.roll_count,
                    template.print_val_size_mm.or(map.width_mm),
                ) {
                    return Err(stage_error(
                        "kiritilgan rang soni yoki val o‘lchami bosma apparatga mos emas. Rang soni va val o‘lchamini tekshiring",
                    ));
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
                return Err(stage_error(
                    "hisoblangan material eni apparat yoki laminatsiya chegaralariga mos emas, yoxud bosqichga kiruvchi en aniqlanmadi. Kadr eni, kadr soni va chet qo‘shimchasini tekshiring",
                ));
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
