use crate::core::auth::models::Principal;

use super::{QolipError, QolipProduct, QolipService};

/// Store identities stay on the server; HTTP exposes an opaque row identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QolipCatalogEntry {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QolipCatalogPosition {
    pub name: String,
    pub code: String,
}

impl QolipService {
    pub async fn catalog_page(
        &self,
        principal: &Principal,
        is_admin: bool,
        query: &str,
        limit: usize,
        after: Option<&QolipCatalogPosition>,
    ) -> Result<Vec<QolipCatalogEntry>, QolipError> {
        let allowed = if is_admin {
            None
        } else {
            Some(self.warehouses_for_principal(principal, false).await?)
        };
        // One extra product establishes has_more without a full count query.
        self.store
            .catalog_page(query, limit.clamp(1, 200) + 1, after, allowed.as_deref())
            .await
    }

    pub async fn catalog_detail(
        &self,
        principal: &Principal,
        is_admin: bool,
        item_code: &str,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        let allowed = if is_admin {
            None
        } else {
            Some(self.warehouses_for_principal(principal, false).await?)
        };
        self.store
            .catalog_detail(item_code, allowed.as_deref())
            .await
    }
}
