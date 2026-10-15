async fn validate_quick_template_identity(
    state: &AppState,
    owner: &str,
    input: &ProductionMapSaveWithOrderRequest,
) -> Result<(), AdminError> {
    let Some(template) = &input.template else {
        return Ok(());
    };
    let read_only_clone = input.template_action == "use"
        || (input.template_action.is_empty()
            && !template.source_map_id.is_empty()
            && template.source_map_id != input.map.id
            && (input.map.id.starts_with("zakaz-draft-") || is_sheet_order_map(&input.map)));
    if read_only_clone {
        state
            .calculate_orders
            .validate_read_identity(owner, template)
            .await
            .map_err(calculate_order_error)?;
    } else {
        state
            .calculate_orders
            .validate_identity(owner, template)
            .await
            .map_err(calculate_order_error)?;
    }
    let templates = state
        .calculate_orders
        .list(owner)
        .await
        .map_err(calculate_order_error)?;
    if input.template_action != "fork"
        && !template.item_code.is_empty()
        && templates.iter().any(|existing| {
            existing.source_map_id.eq_ignore_ascii_case(&input.map.id)
                && !existing.item_code.is_empty()
                && existing.item_code != template.item_code
        })
    {
        return Err(conflict("template_identity_conflict"));
    }
    // Early validation prevents even a transient map write for a stale identity.
    for existing in &templates {
        if input.template_action == "fork"
            && ((!template.id.trim().is_empty() && existing.id == template.id)
                || (!template.code.trim().is_empty()
                    && existing.code.eq_ignore_ascii_case(template.code.trim())))
            && (existing.id != template.id
                || !existing.code.eq_ignore_ascii_case(&template.code)
                || existing.source_map_id
                    != format!("template-{}", template.id.trim()).to_ascii_lowercase())
        {
            return Err(conflict("template_identity_conflict"));
        }
    }
    if input.template_action == "fork"
        && (template.id.trim().is_empty() || template.code.trim().is_empty())
    {
        return Err(bad_request("template_fork_identity_required"));
    }
    if input.template_action == "fork" {
        let fork_id = format!("template-{}", template.id.trim()).to_ascii_lowercase();
        if state
            .production_maps
            .raw_map(&fork_id)
            .await
            .map_err(production_map_error)?
            .is_some()
            && !templates.iter().any(|saved| {
                saved.id == template.id
                    && saved.source_map_id == fork_id
                    && saved.variant_id == template.id
            })
        {
            return Err(conflict("template_identity_conflict"));
        }
    }
    if !template.source_map_id.is_empty()
        && input.template_action != "fork"
        && is_quick_template_order_clone(&input.map, template)
    {
        let source = state
            .production_maps
            .raw_map(&template.source_map_id)
            .await
            .map_err(production_map_error)?;
        let Some(source) = source else {
            // Legacy callers submit a full order snapshot and may refer to an
            // archived design. New explicit `use` requests require that design.
            if input.template_action.is_empty() {
                return Ok(());
            }
            return Err(not_found("map_not_found"));
        };
        let same_sku = templates.iter().any(|saved| {
            saved
                .source_map_id
                .eq_ignore_ascii_case(&template.source_map_id)
                && saved.item_code == template.item_code
                && !saved.item_code.is_empty()
        });
        if !same_sku && !template.item_code.is_empty() && source.product_code != template.item_code
        {
            return Err(conflict("template_identity_conflict"));
        }
    }
    Ok(())
}

fn order_creation_identity(
    input: &ProductionMapSaveWithOrderRequest,
) -> Option<crate::core::mini_orders::OrderCreationIdentity> {
    use sha2::{Digest, Sha256};
    if !input.map.id.starts_with("zakaz-draft-") {
        return None;
    }
    let mut value = serde_json::to_value(input).ok()?;
    if let Some(template) = value["template"].as_object_mut() {
        template.remove("saved_at");
    }
    Some(crate::core::mini_orders::OrderCreationIdentity {
        key: input.map.id.clone(),
        fingerprint: format!("{:x}", Sha256::digest(serde_json::to_vec(&value).ok()?)),
    })
}

async fn replay_order_creation(
    state: &AppState,
    owner: &str,
    creation: Option<&crate::core::mini_orders::OrderCreationIdentity>,
) -> Result<Option<serde_json::Value>, AdminError> {
    let Some(creation) = creation else {
        return Ok(None);
    };
    let Some((fingerprint, result)) = state
        .production_orders
        .order_creation_result(owner, &creation.key)
        .await
        .map_err(production_map_error)?
    else {
        return Ok(None);
    };
    if fingerprint != creation.fingerprint {
        return Err(conflict("order_creation_idempotency_conflict"));
    }
    Ok(Some(result))
}

async fn prepare_template_fork(
    state: &AppState,
    input: &mut ProductionMapSaveWithOrderRequest,
) -> Result<Option<ProductionMapDefinition>, AdminError> {
    if input.template_action != "fork" {
        return Ok(None);
    }
    let template = input
        .template
        .as_mut()
        .ok_or_else(|| bad_request("template_required"))?;
    let source_id = template.source_map_id.trim();
    let source = state
        .production_maps
        .raw_map(source_id)
        .await
        .map_err(production_map_error)?
        .ok_or_else(|| not_found("map_not_found"))?;
    // Deterministic fork identity: the same variant retry cannot create another map.
    let fork_id = format!("template-{}", template.id.trim()).to_ascii_lowercase();
    if source.id == fork_id {
        return Err(conflict("template_identity_conflict"));
    }
    let mut copy = input.map.clone();
    copy.id = fork_id.clone();
    copy.product_code = template.item_code.trim().to_string();
    copy.code.clear();
    copy.order_number.clear();
    copy.order_kg = None;
    copy.base_length = None;
    for node in &mut copy.nodes {
        node.alternative_assigned_apparatus_id.clear();
        node.alternative_assigned_title.clear();
        if node.item_code == source.product_code {
            node.item_code = copy.product_code.clone();
        }
    }
    template.source_map_id = fork_id;
    template.variant_id = template.id.trim().to_string();
    if is_sheet_order_map(&input.map) || input.map.id.starts_with("zakaz-draft-") {
        Ok(Some(copy))
    } else {
        input.map = copy;
        Ok(None)
    }
}
