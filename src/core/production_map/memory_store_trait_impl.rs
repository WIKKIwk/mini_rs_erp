
// Compose the domain sections before async_trait expands the complete store implementation.
include!("memory_store_trait_impl_parts/memory_map_settings.rs");
include!("memory_store_trait_impl_parts/memory_map_scheduling.rs");
include!("memory_store_trait_impl_parts/memory_map_execution.rs");
