use super::*;

#[derive(Default, serde::Deserialize)]
pub struct SharedSetQuery {
    #[serde(default)]
    pub identifier: String,
    #[serde(default)]
    pub item_code: String,
}

fn product(spec: crate::core::qolip::QolipProductSpec) -> QolipProduct {
    QolipProduct {
        is_extension: spec.is_extension,
        qolip_set_id: spec.set_id(),
        warehouse: spec.warehouse,
        code: spec.item_code,
        name: spec.item_name,
        item_group: spec.item_group,
        qolip_code: spec.qolip_code,
        size: spec.size,
        color: spec.color,
        has_qolip_spec: true,
        ..Default::default()
    }
}

pub async fn shared_sets(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<SharedSetQuery>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    match method {
        Method::GET if !query.identifier.trim().is_empty() => {
            let snapshot = state
                .qolip
                .resolve_shared_set(&query.identifier)
                .await
                .map_err(qolip_error)?;
            if let Some(set) = &snapshot {
                for spec in &set.members {
                    ensure_qolip_owner_access(&state, &principal, spec).await?;
                }
            }
            Ok(Json(serde_json::json!({"ok": true, "set": snapshot})))
        }
        Method::GET if !query.item_code.trim().is_empty() => {
            let specs = state
                .qolip
                .required_qolips_for_order(&query.item_code, "")
                .await
                .map_err(qolip_error)?;
            let mut products = Vec::new();
            for spec in specs {
                match ensure_qolip_owner_access(&state, &principal, &spec).await {
                    Ok(()) => products.push(product(spec)),
                    Err((StatusCode::FORBIDDEN, _)) => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(Json(serde_json::json!({"ok": true, "products": products})))
        }
        Method::POST => {
            let input: crate::core::qolip::QolipSetReuse =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            let is_admin = state
                .admin
                .principal_has_capability(&principal, Capability::AdminAccess)
                .await;
            let snapshot = state
                .qolip
                .reuse_shared_set(&input, &principal, is_admin)
                .await
                .map_err(qolip_error)?;
            let products = snapshot
                .members
                .iter()
                .cloned()
                .map(product)
                .collect::<Vec<_>>();
            Ok(Json(
                serde_json::json!({"ok": true, "set": snapshot, "products": products}),
            ))
        }
        Method::GET => Err(bad_request("qolip_code_required")),
        _ => Err(method_not_allowed()),
    }
}
