use super::*;
use crate::core::auth::models::Principal;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

pub(super) async fn affected_codes(
    state: &AppState,
    principal: &Principal,
    molds: &[String],
) -> Result<Vec<String>, QolipError> {
    let is_admin = state
        .admin
        .principal_has_admin_workspace_access(principal)
        .await;
    let mut visible = Vec::new();
    for code in state.qolip.catalog_codes_for_molds(molds).await? {
        if !state
            .qolip
            .catalog_detail(principal, is_admin, &code)
            .await?
            .is_empty()
        {
            visible.push(code);
        }
    }
    Ok(visible)
}

/// A successful write carries its scoped catalog changes. A failed optional
/// read invalidates the snapshot without reporting an already committed write
/// as failed. No background synchronization endpoint or subscription is used.
pub(super) async fn after_write(
    state: &AppState,
    principal: &Principal,
    codes: Vec<String>,
) -> serde_json::Value {
    match read(state, principal, codes).await {
        Ok(delta) => delta,
        Err(_) => serde_json::json!({"reset": true}),
    }
}

async fn read(
    state: &AppState,
    principal: &Principal,
    codes: Vec<String>,
) -> Result<serde_json::Value, QolipError> {
    let codes = codes
        .into_iter()
        .map(|code| code.trim().to_lowercase())
        .filter(|code| !code.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    let is_admin = state
        .admin
        .principal_has_admin_workspace_access(principal)
        .await;
    let images = state
        .qolip
        .order_image_order_ids(&codes.iter().cloned().collect::<Vec<_>>())
        .await?
        .into_iter()
        .map(|(code, id)| (code.trim().to_lowercase(), id))
        .collect::<std::collections::HashMap<_, _>>();
    let mut upserts = Vec::new();
    let mut removed = Vec::new();
    for code in codes {
        let products = state
            .qolip
            .catalog_detail(principal, is_admin, &code)
            .await?;
        let Some(first) = products.first() else {
            removed.push(format!("qp1.{}", URL_SAFE_NO_PAD.encode(code.as_bytes())));
            continue;
        };
        let image = images.get(&code).cloned().unwrap_or_default();
        upserts.push(serde_json::json!({
            "id": format!("qp1.{}", URL_SAFE_NO_PAD.encode(code.as_bytes())),
            "name": first.name,
            "order_image_order_id": image,
            "products": products.into_iter().map(|product| QolipProductResponse {
                product, order_image_order_id: image.clone(),
            }).collect::<Vec<_>>(),
        }));
    }
    Ok(serde_json::json!({"upserts": upserts, "removed_ids": removed}))
}
