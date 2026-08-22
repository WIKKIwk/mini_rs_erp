use sqlx::{PgPool, Postgres, Transaction};

use crate::core::auth::models::Principal;
use crate::core::qolip::normalize::qolip_location_id;
use crate::core::qolip::{QolipBlock, QolipError, QolipProduct, QolipProductSpec, role_code};

use super::rows::{QolipBlockRow, QolipProductRow, QolipProductSpecRow, row_to_product_spec};

include!("catalog_parts/part_01.rs");
include!("catalog_parts/part_02.rs");
include!("catalog_parts/part_03.rs");
