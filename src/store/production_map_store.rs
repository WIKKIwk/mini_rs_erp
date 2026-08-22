mod capacity;
mod map_helpers;
mod maps;
mod migration;
mod queue;
mod unsupported_materials;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rusqlite::Connection;

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{
    ApparatusDowntime, ApparatusQueueActionEvent, ApparatusScheduleCancelRequest,
    ApparatusScheduleReservation, ProductionMapDefinition, ProductionMapError,
    ProductionMapStorePort, QueueActionActor, RawMaterialAssignment,
};

use self::migration::{configure_connection, migrate};

#[derive(Clone)]
pub struct ProductionMapStore {
    conn: Arc<Mutex<Connection>>,
}

impl ProductionMapStore {
    pub fn new(path: PathBuf) -> Self {
        Self::open(path).unwrap_or_else(|error| {
            panic!("production map sqlite store unavailable: {error}");
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, ProductionMapError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|_| ProductionMapError::StoreFailed)?;
        }
        let conn = Connection::open(path).map_err(|_| ProductionMapError::StoreFailed)?;
        configure_connection(&conn).map_err(|_| ProductionMapError::StoreFailed)?;
        migrate(&conn).map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

#[async_trait]
impl ProductionMapStorePort for ProductionMapStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        maps::maps(self).await
    }

    async fn put_map(&self, map: ProductionMapDefinition) -> Result<(), ProductionMapError> {
        maps::put_map(self, map).await
    }

    async fn put_maps_batch(
        &self,
        maps: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError> {
        maps::put_maps_batch(self, maps).await
    }

    async fn delete_map(&self, map_id: &str) -> Result<(), ProductionMapError> {
        maps::delete_map(self, map_id).await
    }

    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        maps::apparatus_sequences(self).await
    }

    async fn put_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        maps::put_apparatus_sequence(self, apparatus, order_ids).await
    }

    async fn apparatus_downtimes(&self) -> Result<Vec<ApparatusDowntime>, ProductionMapError> {
        capacity::apparatus_downtimes(self).await
    }

    async fn put_apparatus_downtime(
        &self,
        downtime: ApparatusDowntime,
    ) -> Result<(), ProductionMapError> {
        capacity::put_apparatus_downtime(self, downtime).await
    }

    async fn apparatus_schedule_reservations(
        &self,
    ) -> Result<Vec<ApparatusScheduleReservation>, ProductionMapError> {
        capacity::apparatus_schedule_reservations(self).await
    }

    async fn apparatus_schedule_reservation_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<ApparatusScheduleReservation>, ProductionMapError> {
        capacity::apparatus_schedule_reservation_by_idempotency_key(self, idempotency_key).await
    }

    async fn put_apparatus_schedule_reservation(
        &self,
        reservation: ApparatusScheduleReservation,
        capacity_slots: u16,
        finite_capacity: bool,
    ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
        capacity::put_apparatus_schedule_reservation(
            self,
            reservation,
            capacity_slots,
            finite_capacity,
        )
        .await
    }

    async fn cancel_apparatus_schedule_reservation(
        &self,
        input: ApparatusScheduleCancelRequest,
    ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
        capacity::cancel_apparatus_schedule_reservation(self, input).await
    }

    async fn update_apparatus_schedule_reservation_status(
        &self,
        order_id: &str,
        apparatus_id: &ApparatusId,
        status: crate::core::production_map::ApparatusScheduleStatus,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        capacity::update_apparatus_schedule_reservation_status(
            self,
            order_id,
            apparatus_id.as_str(),
            status,
            actor,
        )
        .await
    }

    async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        queue::apparatus_queue_states(self).await
    }

    async fn put_apparatus_queue_states(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
    ) -> Result<(), ProductionMapError> {
        queue::put_apparatus_queue_states(self, apparatus, states).await
    }

    async fn put_apparatus_queue_states_with_event(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        queue::put_apparatus_queue_states_with_event(self, apparatus, states, event).await
    }

    async fn append_apparatus_queue_action_event(
        &self,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        queue::append_apparatus_queue_action_event(self, event).await
    }

    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        unsupported_materials::raw_material_assignments().await
    }

    async fn put_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        unsupported_materials::put_raw_material_assignment(assignment).await
    }

    async fn delete_raw_material_assignment(
        &self,
        order_id: &str,
        barcode: &str,
    ) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
        unsupported_materials::delete_raw_material_assignment(order_id, barcode).await
    }
}

fn unix_micros() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default()
}
