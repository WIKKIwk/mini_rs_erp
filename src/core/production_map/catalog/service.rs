use super::*;

use std::collections::{BTreeMap, BTreeSet};

use super::compiler::{compile_map, normalize_map, run_map_with_variables};
use super::progress::{
    latest_required_complete_event, required_apparatus_for_closed_order,
};
use crate::core::apparatus_standard::ApparatusId;

#[path = "alternatives.rs"]
mod alternatives;

pub(super) fn compile_saved_maps(
    maps: impl IntoIterator<Item = ProductionMapDefinition>,
) -> Vec<ProductionMapSaved> {
    let mut saved = Vec::new();
    for mut map in maps {
        // Legacy maps saved before `code` existed: expose the order
        // number as the code so clients never need a fallback.
        if map.code.trim().is_empty() && !map.order_number.trim().is_empty() {
            map.code = map.order_number.trim().to_string();
        }
        match compile_map(&map) {
            Ok(program) => saved.push(ProductionMapSaved { map, program }),
            Err(error) => {
                tracing::warn!(
                    map_id = %map.id,
                    error = ?error,
                    "skipping invalid production map in list response"
                );
            }
        }
    }
    saved
}

/// Reuse only a program compiled from an identical complete map definition.
/// Queue permissions/state are still rebuilt from current database data.
pub(super) fn compile_saved_maps_reusing_programs(
    maps: impl IntoIterator<Item = ProductionMapDefinition>,
    previous: Option<&[ProductionMapSaved]>,
) -> Vec<ProductionMapSaved> {
    let previous = previous
        .unwrap_or_default()
        .iter()
        .map(|saved| (saved.map.id.as_str(), saved))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut result = Vec::new();
    for mut map in maps {
        if map.code.trim().is_empty() && !map.order_number.trim().is_empty() {
            map.code = map.order_number.trim().to_string();
        }
        if let Some(saved) = previous
            .get(map.id.as_str())
            .filter(|saved| saved.map == map)
        {
            result.push(ProductionMapSaved {
                map,
                program: saved.program.clone(),
            });
        } else {
            result.extend(compile_saved_maps([map]));
        }
    }
    result
}

include!("service_parts/next_order_number.rs");

include!("service_parts/validate_and_normalize_map_apparatus.rs");
