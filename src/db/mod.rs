pub mod postgres;
pub mod postgres_admin_catalog;
pub mod postgres_calculate_material;
pub mod postgres_calculate_order;
pub(crate) mod postgres_canonical_apparatus;
pub mod postgres_chat;
pub mod postgres_chat_media;
pub mod postgres_customer;
pub mod postgres_engine;
pub mod postgres_factory_location;
pub mod postgres_gscale_receipt;
pub mod postgres_inventory_movements;
pub mod postgres_mini_order;
pub mod postgres_order_reset;
pub mod postgres_production_map;
pub mod postgres_push_token;
pub mod postgres_qolip;
pub mod postgres_raw_material_events;
pub mod postgres_returned_paint;
pub mod postgres_rps_batch;
pub mod postgres_system_user;
pub mod postgres_training_workspace;
pub mod postgres_warehouse;
pub mod postgres_worker;
pub mod postgres_worker_group;

#[cfg(test)]
mod tests;
