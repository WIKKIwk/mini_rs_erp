use axum::http::header;
use axum::response::IntoResponse;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

use super::*;
use crate::core::qolip::QolipCatalogPosition;

#[derive(Default, Deserialize)]
pub struct CatalogQuery {
    #[serde(default)]
    q: String,
    limit: Option<usize>,
    cursor: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    q: String,
    name: String,
    code: String,
}

#[derive(Deserialize)]
pub struct DetailQuery {
    id: String,
}

pub async fn catalog_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CatalogQuery>,
) -> Result<axum::response::Response, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let is_admin = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await;
    let q = query.q.trim().to_lowercase();
    let limit = query.limit.unwrap_or(200).clamp(1, 200);
    let after = query
        .cursor
        .as_deref()
        .map(|raw| {
            let cursor: Cursor = decode(raw).ok_or_else(|| bad_request("invalid_cursor"))?;
            if cursor.q != q || cursor.code.is_empty() {
                return Err(bad_request("invalid_cursor"));
            }
            Ok(QolipCatalogPosition {
                name: cursor.name,
                code: cursor.code,
            })
        })
        .transpose()?;
    let mut entries = state
        .qolip
        .catalog_page(&principal, is_admin, &q, limit, after.as_ref())
        .await
        .map_err(qolip_error)?;
    let has_more = entries.len() > limit;
    entries.truncate(limit);
    let next_cursor = if has_more {
        entries
            .last()
            .map(|last| {
                encode(&Cursor {
                    q,
                    name: last.name.to_lowercase(),
                    code: last.code.trim().to_lowercase(),
                })
            })
            .transpose()
            .map_err(|_| qolip_error(QolipError::StoreFailed))?
    } else {
        None
    };
    let item_codes = entries
        .iter()
        .map(|entry| entry.code.clone())
        .collect::<Vec<_>>();
    let images = state
        .qolip
        .order_image_order_ids(&item_codes)
        .await
        .map_err(qolip_error)?
        .into_iter()
        .map(|(code, order_id)| (code.trim().to_lowercase(), order_id))
        .collect::<std::collections::HashMap<_, _>>();
    let items = entries
        .into_iter()
        .map(|entry| {
            serde_json::json!({
                "id": format!("qp1.{}", URL_SAFE_NO_PAD.encode(entry.code.trim().to_lowercase().as_bytes())),
                "name": entry.name,
                "order_image_order_id": images.get(&entry.code.trim().to_lowercase())
                    .map(String::as_str).unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();
    Ok((
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(serde_json::json!({"ok": true, "items": items,
        "next_cursor": next_cursor, "has_more": has_more})),
    )
        .into_response())
}

pub async fn catalog_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DetailQuery>,
) -> Result<axum::response::Response, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let code = query
        .id
        .strip_prefix("qp1.")
        .filter(|raw| raw.len() <= 8192)
        .and_then(|raw| URL_SAFE_NO_PAD.decode(raw).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|code| !code.trim().is_empty())
        .ok_or_else(|| bad_request("invalid_product_id"))?;
    let is_admin = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await;
    let products = state
        .qolip
        .catalog_detail(&principal, is_admin, &code)
        .await
        .map_err(qolip_error)?;
    if products.is_empty() {
        return Err(qolip_error(QolipError::ProductNotFound));
    }
    let images = state
        .qolip
        .order_image_order_ids(&[code.clone()])
        .await
        .map_err(qolip_error)?;
    let order_id = images
        .into_iter()
        .find(|(item, _)| item.trim().eq_ignore_ascii_case(code.trim()))
        .map(|(_, id)| id)
        .unwrap_or_default();
    let products = products
        .into_iter()
        .map(|product| QolipProductResponse {
            product,
            order_image_order_id: order_id.clone(),
        })
        .collect::<Vec<_>>();
    Ok((
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(serde_json::json!({"ok": true, "products": products})),
    )
        .into_response())
}

fn encode(value: &impl Serialize) -> Result<String, serde_json::Error> {
    serde_json::to_vec(value).map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
}

fn decode<T: serde::de::DeserializeOwned>(raw: &str) -> Option<T> {
    if raw.len() > 16384 {
        return None;
    }
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(raw).ok()?).ok()
}
