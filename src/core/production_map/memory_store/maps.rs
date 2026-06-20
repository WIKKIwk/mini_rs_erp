use super::*;

use std::collections::BTreeMap;

use super::super::compiler::reject_order_number_immutable;

pub(super) async fn maps(
    store: &MemoryProductionMapStore,
) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
    Ok(store.maps.read().await.values().cloned().collect())
}

pub(super) async fn put_map(
    store: &MemoryProductionMapStore,
    map: ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let mut maps = store.maps.write().await;
    reject_order_number_immutable(&maps, &map)?;
    let order_number = map.order_number.trim();
    if !order_number.is_empty() {
        let duplicate = maps.values().any(|existing| {
            existing.order_number.trim() == order_number && existing.id.trim() != map.id.trim()
        });
        if duplicate {
            return Err(ProductionMapError::DuplicateOrderNumber);
        }
    }
    maps.insert(map.id.clone(), map);
    Ok(())
}

pub(super) async fn put_maps_batch(
    store: &MemoryProductionMapStore,
    maps: &[ProductionMapDefinition],
) -> Result<(), ProductionMapError> {
    let mut existing_maps = store.maps.write().await;
    for map in maps {
        reject_order_number_immutable(&existing_maps, map)?;
        let order_number = map.order_number.trim();
        if !order_number.is_empty() {
            let duplicate = existing_maps.values().any(|existing| {
                existing.order_number.trim() == order_number && existing.id.trim() != map.id.trim()
            });
            if duplicate {
                return Err(ProductionMapError::DuplicateOrderNumber);
            }
        }
    }
    for map in maps {
        existing_maps.insert(map.id.clone(), map.clone());
    }
    Ok(())
}

pub(super) async fn delete_map(
    store: &MemoryProductionMapStore,
    map_id: &str,
) -> Result<(), ProductionMapError> {
    store.maps.write().await.remove(map_id.trim());
    Ok(())
}

pub(super) async fn apparatus_sequences(
    store: &MemoryProductionMapStore,
) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
    Ok(store.sequences.read().await.clone())
}

pub(super) async fn put_apparatus_sequence(
    store: &MemoryProductionMapStore,
    apparatus: &str,
    order_ids: Vec<String>,
) -> Result<(), ProductionMapError> {
    store
        .sequences
        .write()
        .await
        .insert(apparatus.trim().to_string(), order_ids);
    Ok(())
}
