mod graph;
mod normalize;
mod operation;
mod runner;
mod validation;

use std::collections::BTreeMap;

use super::types::*;
use graph::topological_order;
pub(super) use normalize::normalize_map;
#[cfg(test)]
pub(super) use normalize::reject_order_number_immutable;
use operation::compile_node;
#[cfg(test)]
pub use runner::run_map;
pub use runner::run_map_with_variables;
use validation::validate_map;

pub fn compile_map(
    map: &ProductionMapDefinition,
) -> Result<ProductionMapProgram, ProductionMapError> {
    validate_map(map)?;
    let order = topological_order(map)?;
    let node_by_id: BTreeMap<&str, &ProductionMapNode> = map
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut operations = Vec::with_capacity(order.len());
    for (index, node_id) in order.into_iter().enumerate() {
        let node = node_by_id
            .get(node_id.as_str())
            .expect("topological order only contains known node ids");
        operations.push(compile_node(index + 1, node)?);
    }
    Ok(ProductionMapProgram {
        map_id: map.id.clone(),
        product_code: map.product_code.clone(),
        operations,
    })
}
