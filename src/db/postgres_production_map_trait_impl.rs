
// Compose the domain sections before async_trait expands the complete store implementation.
include!("postgres_production_map_trait_impl_parts/postgres_map_catalog.rs");
include!("postgres_production_map_trait_impl_parts/postgres_map_execution.rs");
include!("postgres_production_map_trait_impl_parts/postgres_map_stock.rs");
