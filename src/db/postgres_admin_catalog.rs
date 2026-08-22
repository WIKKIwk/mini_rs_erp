use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::admin::item_customer_policy::FINISHED_GOODS_CUSTOMER_REQUIRED;
use crate::core::admin::models::{
    AdminDirectoryEntry, AdminItemDetail, AdminItemGroup, AdminWarehouse,
};
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminWritePort};
use crate::core::werka::models::SupplierItem;

pub(crate) mod customer_policy;
mod helpers;
mod item_customer_writes;
mod item_delete_safety;
mod rows;

use self::customer_policy::{customerless_items_in_subtree, lock_item_customer_policy};
use self::rows::{ItemGroupRow, ItemRow};

include!("postgres_admin_catalog_parts/part_01.rs");
include!("postgres_admin_catalog_parts/part_02.rs");
