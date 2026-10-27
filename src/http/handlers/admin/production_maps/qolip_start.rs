use super::*;
use crate::core::production_map::{AutomaticQolipConfirmation, automatic_qolip_set_fingerprint};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsInput {
    automatic_confirmation_enabled: bool,
}

pub async fn production_map_qolip_start_settings(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    if principal.role != PrincipalRole::Admin {
        return Err(forbidden());
    }
    let settings = match method {
        Method::GET => state.production_maps.qolip_start_settings().await,
        Method::PUT => {
            let input: SettingsInput = parse_json(&body)?;
            state
                .production_maps
                .update_qolip_start_settings(
                    input.automatic_confirmation_enabled,
                    &queue_action_actor(&principal),
                )
                .await
        }
        _ => return Err(method_not_allowed()),
    }
    .map_err(production_map_error)?;
    Ok(json_response(
        serde_json::json!({"ok":true,"settings":settings}),
    ))
}

#[derive(Clone, serde::Serialize)]
pub(super) struct ReadyMold {
    pub qolip_code: String,
    pub color: String,
    pub qolip_set_id: String,
    pub is_extension: bool,
    pub ready: bool,
    pub blocking_reason_code: String,
}

#[derive(Clone, serde::Serialize)]
pub(super) struct ReadySet {
    pub set_id: String,
    pub fingerprint: String,
    pub ready: bool,
    pub blocking_reason_code: String,
}

pub(super) struct AutomaticReadiness {
    pub policy_revision: i64,
    pub blocking_reason_code: String,
    pub sets: Vec<ReadySet>,
    pub molds: Vec<ReadyMold>,
    pub specs: Vec<crate::core::qolip::QolipProductSpec>,
    pub start_allowed: bool,
}

pub(super) async fn automatic_readiness(
    state: &AppState,
    principal: &Principal,
    apparatus: &queue_actions::QueueApparatusMetadata,
    order_id: &str,
) -> Result<AutomaticReadiness, AdminError> {
    let settings = state
        .production_maps
        .qolip_start_settings()
        .await
        .map_err(production_map_error)?;
    if !settings.automatic_confirmation_enabled {
        return Err(conflict("qolip_automatic_confirmation_disabled"));
    }
    if order_id.starts_with("training-") || !apparatus.requires_qolip_scan() {
        return Err(bad_request("qolip_confirmation_invalid"));
    }
    // Opt-in readiness is scoped to an actionable fresh start; it cannot grant
    // tooling to paused, completed or another worker's queue row.
    let controls = state
        .production_maps
        .queue_action_controls_for_apparatus(apparatus.id.as_str())
        .await
        .map_err(production_map_error)?;
    let control = controls
        .get(apparatus.id.as_str())
        .and_then(|orders| orders.get(order_id))
        .ok_or_else(|| bad_request("order_not_available"))?;
    let start_allowed = control
        .allowed_actions
        .contains(&queue_state::ApparatusQueueAction::Start);
    if control.interaction.qolip_mode
        != crate::core::production_map::ApparatusQueueQolipMode::ScanRequired
    {
        return Err(bad_request("queue_action_not_allowed"));
    }
    let map = state
        .production_maps
        .raw_map(order_id)
        .await
        .map_err(production_map_error)?
        .ok_or_else(|| production_map_error(ProductionMapError::MapNotFound))?;
    let mut blocker = String::new();
    let specs = match state
        .qolip
        .required_qolips_for_layout(&map.products, &map.product_code, &map.title)
        .await
    {
        Ok(specs) => specs,
        Err(error) => {
            blocker = qolip_error_code(&error).to_string();
            // A missing/incompatible common set has no selectable candidate.
            Vec::new()
        }
    };
    if specs.is_empty() && blocker.is_empty() {
        blocker = "qolip_scan_required".into();
    }
    // Preserve the strict existing all-order rework rule, including intact
    // alternative sets, while still returning individual member statuses.
    if let Err(error) = state
        .qolip
        .require_qolips_available_for_layout(&map.products, &map.product_code, &map.title)
        .await
    {
        if matches!(error, crate::core::qolip::QolipError::QolipInRework) {
            blocker = "qolip_in_rework".into();
        } else {
            return Err(queue_actions::qolip_queue_error(error));
        }
    }
    let rework_codes = state
        .qolip
        .active_rework_codes(
            &specs
                .iter()
                .map(|spec| spec.qolip_code.clone())
                .collect::<Vec<_>>(),
        )
        .await
        .map_err(queue_actions::qolip_queue_error)?
        .into_iter()
        .map(|code| code.trim().to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    let mut molds = Vec::new();
    for spec in &specs {
        let mut reason = String::new();
        if rework_codes.contains(&spec.qolip_code.trim().to_lowercase()) {
            reason = "qolip_in_rework".into();
        } else if let Err(error) =
            queue_actions::reject_qolip_in_use(state, apparatus, order_id, &spec.qolip_code).await
        {
            reason = error.1.0.error;
        } else {
            // This prepares a read-only checkout draft; no stock is claimed by
            // opening a sheet. Per-mold validation omits the global rework gate.
            if let Err(error) = state
                .qolip
                .prepare_available_qolip_for_order_start(
                    &spec.qolip_code,
                    &map.product_code,
                    &map.title,
                    &principal.ref_,
                    &principal.display_name,
                    principal,
                )
                .await
            {
                reason = qolip_error_code(&error).into();
            }
        }
        molds.push(ReadyMold {
            qolip_code: spec.qolip_code.clone(),
            color: spec.color.clone(),
            qolip_set_id: spec.set_id(),
            is_extension: spec.is_extension,
            ready: reason.is_empty(),
            blocking_reason_code: reason,
        });
    }
    let ids = specs
        .iter()
        .filter(|spec| !spec.is_extension)
        .map(|spec| spec.set_id())
        .collect::<std::collections::BTreeSet<_>>();
    let ids = if ids.is_empty() && !specs.is_empty() {
        std::collections::BTreeSet::from([specs[0].set_id()])
    } else {
        ids
    };
    let sets = ids
        .into_iter()
        .map(|id| {
            let members = specs
                .iter()
                .filter(|spec| spec.is_extension || spec.set_id() == id)
                .cloned()
                .collect::<Vec<_>>();
            let reason = if !blocker.is_empty() {
                blocker.clone()
            } else {
                molds
                    .iter()
                    .find(|mold| (mold.is_extension || mold.qolip_set_id == id) && !mold.ready)
                    .map(|mold| mold.blocking_reason_code.clone())
                    .unwrap_or_default()
            };
            ReadySet {
                set_id: id.clone(),
                fingerprint: automatic_qolip_set_fingerprint(&id, &members),
                ready: reason.is_empty(),
                blocking_reason_code: reason,
            }
        })
        .collect::<Vec<_>>();
    if blocker.is_empty() && !sets.iter().any(|set| set.ready) {
        blocker = sets
            .first()
            .map(|set| set.blocking_reason_code.clone())
            .unwrap_or_else(|| "qolip_scan_required".into());
    }
    if state
        .production_maps
        .qolip_start_settings()
        .await
        .map_err(production_map_error)?
        != settings
    {
        return Err(conflict("qolip_policy_changed"));
    }
    Ok(AutomaticReadiness {
        policy_revision: settings.revision,
        blocking_reason_code: blocker,
        sets,
        molds,
        specs,
        start_allowed,
    })
}

fn qolip_error_code(error: &crate::core::qolip::QolipError) -> &'static str {
    use crate::core::qolip::QolipError::*;
    match error {
        QolipInRework => "qolip_in_rework",
        QolipCodeNotFound => "qolip_code_not_found",
        QolipCodeMismatch => "qolip_code_mismatch",
        MissingQolipCode => "qolip_scan_required",
        ProductNotFound => "product_not_found",
        CheckoutRequired => "qolip_checkout_required",
        CheckoutAssignedToAnotherWorker => "qolip_checkout_assigned_to_another_worker",
        QolipInUse => "qolip_already_in_use",
        InsufficientStock => "insufficient_stock",
        LocationNotFound => "qolip_location_not_found",
        LocationIdentityMismatch => "location_identity_mismatch",
        _ => "qolip_store_failed",
    }
}

pub(super) async fn resolve_automatic_codes(
    state: &AppState,
    principal: &Principal,
    apparatus: &queue_actions::QueueApparatusMetadata,
    order_id: &str,
    confirmation: &AutomaticQolipConfirmation,
) -> Result<Vec<String>, AdminError> {
    if !confirmation.is_valid() {
        return Err(bad_request("qolip_confirmation_invalid"));
    }
    confirmation
        .require_policy(
            &state
                .production_maps
                .qolip_start_settings()
                .await
                .map_err(production_map_error)?,
        )
        .map_err(production_map_error)?;
    let ready = automatic_readiness(state, principal, apparatus, order_id).await?;
    if !ready.start_allowed {
        return Err(bad_request("queue_action_not_allowed"));
    }
    if ready.policy_revision != confirmation.policy_revision {
        return Err(conflict("qolip_policy_changed"));
    }
    let set = ready
        .sets
        .iter()
        .find(|set| set.set_id == confirmation.set_id)
        .ok_or_else(|| conflict("qolip_set_changed"))?;
    if set.fingerprint != confirmation.fingerprint {
        return Err(conflict("qolip_set_changed"));
    }
    if !set.ready {
        return Err(conflict(&set.blocking_reason_code));
    }
    Ok(ready
        .specs
        .iter()
        .filter(|spec| spec.is_extension || spec.set_id() == set.set_id)
        .map(|spec| spec.qolip_code.clone())
        .collect())
}
