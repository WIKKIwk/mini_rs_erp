
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
