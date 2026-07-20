mod hub;
mod models;
mod ports;
mod service;

use crate::core::auth::models::PrincipalRole;

pub use hub::ChatHub;
pub use models::*;
pub use ports::{ChatError, ChatStorePort};
pub use service::ChatService;

pub fn can_participate_in_chat(role: &PrincipalRole) -> bool {
    !matches!(role, PrincipalRole::Customer)
}
