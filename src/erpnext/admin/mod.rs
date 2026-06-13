mod client;
mod helpers;
mod read;
mod write;

use serde::Deserialize;

use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup};
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminWritePort};
use crate::core::werka::models::SupplierItem;
use crate::erpnext::client::ErpnextClient;
