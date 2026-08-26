use super::*;

use std::collections::BTreeMap;

use super::super::compiler::reject_order_number_immutable;
use crate::core::apparatus_standard::ApparatusId;

pub(super) async fn maps(
    store: &MemoryProductionMapStore,
) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
    Ok(store.maps.read().await.values().cloned().collect())
}

pub(super) async fn put_map(
    store: &MemoryProductionMapStore,
    map: ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let order_id = map.id.trim().to_string();
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
    drop(maps);
    store
        .production_order_lifecycles
        .write()
        .await
        .entry(order_id.clone())
        .or_insert_with(|| ProductionOrderLifecycleRecord::released(&order_id));
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
    drop(existing_maps);
    let mut lifecycles = store.production_order_lifecycles.write().await;
    for map in maps {
        let order_id = map.id.trim().to_string();
        lifecycles
            .entry(order_id.clone())
            .or_insert_with(|| ProductionOrderLifecycleRecord::released(&order_id));
    }
    Ok(())
}

pub(super) async fn delete_map(
    store: &MemoryProductionMapStore,
    map_id: &str,
) -> Result<(), ProductionMapError> {
    let map_id = map_id.trim();
    store.maps.write().await.remove(map_id);
    store
        .production_order_lifecycles
        .write()
        .await
        .remove(map_id);
    for order_ids in store.sequences.write().await.values_mut() {
        order_ids.retain(|order_id| order_id.trim() != map_id);
    }
    for states in store.queue_states.write().await.values_mut() {
        states.remove(map_id);
    }
    store.order_controls.write().await.remove(map_id);
    Ok(())
}

pub(super) async fn production_order_lifecycles(
    store: &MemoryProductionMapStore,
    order_ids: &[String],
) -> Result<BTreeMap<String, ProductionOrderLifecycleRecord>, ProductionMapError> {
    let requested = order_ids
        .iter()
        .map(|order_id| order_id.trim())
        .filter(|order_id| !order_id.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    Ok(store
        .production_order_lifecycles
        .read()
        .await
        .iter()
        .filter(|(order_id, _)| requested.is_empty() || requested.contains(order_id.as_str()))
        .map(|(order_id, record)| (order_id.clone(), record.clone()))
        .collect())
}

pub(super) async fn apparatus_sequences(
    store: &MemoryProductionMapStore,
) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
    let sequences = store.sequences.read().await;
    let mut result = BTreeMap::new();
    for (apparatus, order_ids) in sequences.iter() {
        let apparatus = ApparatusId::new(apparatus.trim().to_string())
            .map_err(|_| ProductionMapError::StoreFailed)?;
        if result
            .insert(apparatus.to_string(), order_ids.clone())
            .is_some()
        {
            return Err(ProductionMapError::StoreFailed);
        }
    }
    Ok(result)
}

pub(super) async fn put_apparatus_sequence(
    store: &MemoryProductionMapStore,
    apparatus: &str,
    order_ids: Vec<String>,
) -> Result<(), ProductionMapError> {
    let apparatus = ApparatusId::new(apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::StoreFailed)?;
    store
        .sequences
        .write()
        .await
        .insert(apparatus.to_string(), order_ids);
    Ok(())
}
