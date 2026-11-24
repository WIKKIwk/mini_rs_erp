pub(in crate::http::handlers::admin) fn production_map_error(error: ProductionMapError) -> AdminError {
    match error {
        ProductionMapError::ReactivationDenied(reason) => {
            conflict(format!("reactivation_{reason}"))
        }
        ProductionMapError::PrintSideChanged => conflict("print_side_changed"),
        ProductionMapError::TemplateIdentityConflict => conflict("template_identity_conflict"),
        ProductionMapError::MissingId => bad_request("map_id_required"),
        ProductionMapError::MissingProductCode => bad_request("map_product_code_required"),
        ProductionMapError::MissingTitle => bad_request("map_title_required"),
        ProductionMapError::MissingStart => bad_request("map_start_required"),
        ProductionMapError::MissingEnd => bad_request("map_end_required"),
        ProductionMapError::DuplicateNode(_) => bad_request("duplicate_node_id"),
        ProductionMapError::DuplicateOrderNumber => bad_request("duplicate_order_number"),
        ProductionMapError::OrderNumberImmutable => bad_request("order_number_immutable"),
        ProductionMapError::OrderNumberExhausted => bad_request("order_number_exhausted"),
        ProductionMapError::MissingEdgeNode(_) => bad_request("missing_edge_node"),
        ProductionMapError::Cycle => bad_request("production_map_cycle"),
        ProductionMapError::MissingFormulaTarget => bad_request("formula_target_required"),
        ProductionMapError::MissingFormulaExpression => bad_request("formula_expression_required"),
        ProductionMapError::InvalidFormulaTarget(_) => bad_request("invalid_formula_target"),
        ProductionMapError::InvalidFormulaExpression(_) => {
            bad_request("invalid_formula_expression")
        }
        ProductionMapError::MapNotFound => not_found("map_not_found"),
        ProductionMapError::InvalidOrderQty => bad_request("invalid_order_qty"),
        ProductionMapError::InvalidPrintValSize => bad_request("invalid_print_val_size"),
        ProductionMapError::InvalidNodeQty(_) => bad_request("invalid_node_qty"),
        ProductionMapError::InvalidLocation(_) => bad_request("invalid_location"),
        ProductionMapError::UnknownFormulaVariable(_) => bad_request("unknown_formula_variable"),
        ProductionMapError::FormulaDivisionByZero => bad_request("formula_division_by_zero"),
        ProductionMapError::MissingConditionBranch => bad_request("condition_branch_required"),
        ProductionMapError::MoveNotAllowed => bad_request("move_not_allowed"),
        ProductionMapError::StartedOrderMoveRequiresTransfer => {
            conflict("started_order_move_requires_transfer")
        }
        ProductionMapError::StartedProductionMapStageLocked => {
            conflict("production_map_started_stage_locked")
        }
        ProductionMapError::ApparatusTransferReasonRequired => {
            bad_request("apparatus_transfer_reason_required")
        }
        ProductionMapError::ApparatusTransferIdempotencyRequired => {
            bad_request("apparatus_transfer_idempotency_required")
        }
        ProductionMapError::ApparatusTransferIdempotencyConflict => {
            conflict("apparatus_transfer_idempotency_conflict")
        }
        ProductionMapError::ApparatusTransferOrderNotPaused => {
            conflict("apparatus_transfer_order_not_paused")
        }
        ProductionMapError::ApparatusTransferSessionNotFound => {
            conflict("apparatus_transfer_session_not_found")
        }
        ProductionMapError::ApparatusTransferProgressNotFound => {
            conflict("apparatus_transfer_progress_not_found")
        }
        ProductionMapError::ApparatusTransferSessionMismatch => {
            conflict("apparatus_transfer_session_mismatch")
        }
        ProductionMapError::ApparatusTransferProgressMismatch => {
            conflict("apparatus_transfer_progress_mismatch")
        }
        ProductionMapError::ApparatusTransferTargetConflict => {
            conflict("apparatus_transfer_target_conflict")
        }
        ProductionMapError::OpenedOrderCalculationLocked => conflict(error.to_string()),
        ProductionMapError::StoreFailed => server_error("store_failed"),
        ProductionMapError::QueueActionNotAllowed => bad_request("queue_action_not_allowed"),
        ProductionMapError::QueueReorderInvalid => bad_request("queue_reorder_invalid"),
        ProductionMapError::QueueReorderConflict => conflict("queue_reorder_conflict"),
        ProductionMapError::QueueReorderFrozen => conflict("queue_reorder_frozen"),
        ProductionMapError::QueueReorderBlocked => conflict("queue_reorder_blocked"),
        ProductionMapError::QueueReorderIdempotencyConflict => {
            conflict("queue_reorder_idempotency_conflict")
        }
        ProductionMapError::PrintPreflightActive => conflict("print_preflight_active"),
        ProductionMapError::PrintPreflightNotFound => not_found("print_preflight_not_found"),
        ProductionMapError::PrintPreflightNotReady => conflict("print_preflight_not_ready"),
        ProductionMapError::PrintPreflightActionNotAllowed => {
            bad_request("print_preflight_action_not_allowed")
        }
        ProductionMapError::QueueSequenceOrderNotFound(_) => {
            bad_request("queue_sequence_order_not_found")
        }
        ProductionMapError::QueueSequenceApparatusMismatch(_) => {
            bad_request("queue_sequence_apparatus_mismatch")
        }
        ProductionMapError::OrderNotStarted => conflict("order_not_started"),
        ProductionMapError::OrderAlreadyCompleted => conflict("order_already_completed"),
        ProductionMapError::OrderFreezeRequested => conflict("order_freeze_requested"),
        ProductionMapError::OrderFrozen => conflict("order_frozen"),
        ProductionMapError::OrderControlActionNotAllowed => {
            conflict("order_control_action_not_allowed")
        }
        ProductionMapError::OrderFreezeTargetNotFound => conflict("order_freeze_target_not_found"),
        ProductionMapError::OrderFreezeTargetAmbiguous => conflict("order_freeze_target_ambiguous"),
        ProductionMapError::OrderFreezeRequestMismatch => conflict("order_freeze_request_mismatch"),
        ProductionMapError::OrderDeleteBlocked(blockers) => (
            StatusCode::CONFLICT,
            Json(AdminErrorResponse::with_blockers(
                "order_delete_blocked",
                blockers,
            )),
        ),
        ProductionMapError::PreviousStageNotCompleted => {
            bad_request("previous_stage_not_completed")
        }
        ProductionMapError::ApparatusNotAssigned => bad_request("apparatus_not_assigned"),
        ProductionMapError::ApparatusWidthExceedsCapability => {
            bad_request("apparatus_width_exceeds_capability")
        }
        ProductionMapError::ApparatusWidthBelowCapability => {
            bad_request("apparatus_width_below_capability")
        }
        ProductionMapError::ApparatusRollCountExceedsCapability => {
            bad_request("apparatus_roll_count_exceeds_capability")
        }
        ProductionMapError::ApparatusQueuePolicyLocked => bad_request("queue_policy_locked"),
        ProductionMapError::RawMaterialInvalidInput => bad_request("raw_material_invalid_input"),
        ProductionMapError::RawMaterialGroupNotAllowed => {
            bad_request("raw_material_group_not_allowed")
        }
        ProductionMapError::RawMaterialGroupAmbiguous(apparatuses) => {
            ambiguous_raw_material_apparatuses(apparatuses)
        }
        ProductionMapError::RawMaterialAlreadyAssigned => {
            bad_request("raw_material_already_assigned")
        }
        ProductionMapError::RawMaterialAlreadyAssignedToOrder => {
            bad_request("raw_material_already_assigned_to_order")
        }
        ProductionMapError::RawMaterialAssignmentNotFound => {
            bad_request("raw_material_assignment_not_found")
        }
        ProductionMapError::RawMaterialAssignmentLocked => {
            bad_request("raw_material_assignment_locked")
        }
        ProductionMapError::RawMaterialStockUnavailable => {
            bad_request("raw_material_stock_unavailable")
        }
        ProductionMapError::RawMaterialOrderNotActive => conflict("raw_material_order_not_active"),
        ProductionMapError::QolipLocationNotFound => bad_request("qolip_location_not_found"),
        ProductionMapError::QolipCodeMismatch => bad_request("qolip_code_mismatch"),
        ProductionMapError::QolipAlreadyInUse => bad_request("qolip_already_in_use"),
        ProductionMapError::QolipInRework => conflict("qolip_in_rework"),
        ProductionMapError::QolipAutomaticConfirmationDisabled => {
            conflict("qolip_automatic_confirmation_disabled")
        }
        ProductionMapError::QolipPolicyChanged => conflict("qolip_policy_changed"),
        ProductionMapError::QolipSetChanged => conflict("qolip_set_changed"),
        ProductionMapError::QolipInsufficientStock => bad_request("insufficient_stock"),
        ProductionMapError::QolipLocationIdentityMismatch => {
            bad_request("location_identity_mismatch")
        }
        ProductionMapError::RawMaterialScanRequired => bad_request("raw_material_scan_required"),
        ProductionMapError::RawMaterialMismatch => bad_request("raw_material_mismatch"),
        ProductionMapError::RawMaterialStateNotReady => bad_request("raw_material_state_not_ready"),
        ProductionMapError::RawMaterialScanIncomplete => {
            bad_request("raw_material_scan_incomplete")
        }
        ProductionMapError::RawMaterialRequirementNotMet => {
            bad_request("raw_material_requirement_not_met")
        }
        ProductionMapError::RawMaterialRollSizeMissing => {
            bad_request("raw_material_roll_size_missing")
        }
        ProductionMapError::RawMaterialRollSizeMismatch => {
            bad_request("raw_material_roll_size_mismatch")
        }
        ProductionMapError::ProgressInputInvalid => bad_request("progress_input_invalid"),
        ProductionMapError::BobinaExceedsGross => bad_request("bobina_exceeds_gross"),
        ProductionMapError::ProgressQrRequired => bad_request("progress_qr_required"),
        ProductionMapError::MergeInputRequired => bad_request("merge_input_required"),
        ProductionMapError::MergeInputSame => conflict("merge_input_same"),
        ProductionMapError::MergeInputAlreadyUsed => conflict("merge_input_already_used"),
        ProductionMapError::MergeInputNotAccepted => bad_request("merge_input_not_accepted"),
        ProductionMapError::MergeInputFrameCountMismatch {
            active_kadr_count,
            scanned_kadr_count,
        } => (
            StatusCode::BAD_REQUEST,
            Json(AdminErrorResponse::merge_input_frame_count_mismatch(
                active_kadr_count,
                scanned_kadr_count,
            )),
        ),
        ProductionMapError::BosmaCompletionMetricsRequired => {
            bad_request("bosma_completion_metrics_required")
        }
        ProductionMapError::LaminatsiyaCompletionMetricsRequired => {
            bad_request("laminatsiya_completion_metrics_required")
        }
        ProductionMapError::LaminatsiyaAstatkaMetricsRequired => {
            bad_request("laminatsiya_astatka_metrics_required")
        }
        ProductionMapError::LaminationRemainderSourceMissing => {
            conflict("lamination_remainder_source_missing")
        }
        ProductionMapError::LaminationRemainderSourceAmbiguous => {
            conflict("lamination_remainder_source_ambiguous")
        }
        ProductionMapError::RezkaAstatkaMetricsRequired => {
            bad_request("rezka_astatka_metrics_required")
        }
        ProductionMapError::RezkaProgressMetricsRequired => {
            bad_request("rezka_progress_metrics_required")
        }
        ProductionMapError::RezkaKadrCountRequired => bad_request("rezka_kadr_count_required"),
        ProductionMapError::InvalidRezkaFrameGroups => bad_request("rezka_frame_groups_invalid"),
        ProductionMapError::RezkaFrameCountMismatch => bad_request("rezka_frame_count_mismatch"),
        ProductionMapError::RezkaOutputCycleConflict => conflict("rezka_output_cycle_conflict"),
        ProductionMapError::RezkaBobinaModeChanged(enabled) => {
            let (status, Json(mut response)) = conflict("rezka_bobina_mode_changed");
            response.include_bobina_in_net = Some(enabled);
            (status, Json(response))
        }
        ProductionMapError::RezkaFinalRollRequired => bad_request("rezka_final_roll_required"),
        ProductionMapError::WarehouseQrAmbiguous => conflict("qr_ambiguous"),
        ProductionMapError::WarehouseWipConflict => conflict("wip_receipt_conflict"),
        ProductionMapError::ProgressBatchNotFound => not_found("progress_batch_not_found"),
        ProductionMapError::ProgressBatchNotAccepted => bad_request("progress_batch_not_accepted"),
        ProductionMapError::WipRouteSourceUnresolved => bad_request("wip_route_source_unresolved"),
        ProductionMapError::WipRouteDestinationUnresolved => {
            bad_request("wip_route_destination_unresolved")
        }
        ProductionMapError::WipRouteAmbiguous => bad_request("wip_route_ambiguous"),
        ProductionMapError::WipRouteChanged => conflict("wip_route_changed"),
        ProductionMapError::ProgressBatchAlreadyUsed { apparatus_name } => {
            let mut body = AdminErrorResponse::new("progress_batch_already_used");
            body.apparatus_name = Some(apparatus_name);
            (StatusCode::BAD_REQUEST, Json(body))
        }
        ProductionMapError::ProgressBatchInUse { apparatus_name } => {
            let mut body = AdminErrorResponse::new("progress_batch_in_use");
            body.apparatus_name = Some(apparatus_name);
            (StatusCode::BAD_REQUEST, Json(body))
        }
        ProductionMapError::ProgressBatchNotResumable => {
            bad_request("progress_batch_not_resumable")
        }
        ProductionMapError::ProgressBatchCorrectionReasonRequired => {
            bad_request("progress_batch_correction_reason_required")
        }
        ProductionMapError::ProgressBatchCorrectionForbidden => forbidden(),
        ProductionMapError::ProgressBatchCorrectionLocked => {
            conflict("progress_batch_correction_locked")
        }
        ProductionMapError::ProgressBatchCorrectionConflict => {
            conflict("progress_batch_correction_conflict")
        }
        ProductionMapError::ProgressBatchCorrectionUnchanged => {
            bad_request("progress_batch_correction_unchanged")
        }
        ProductionMapError::OpeningWipInvalidInput => bad_request("opening_wip_invalid_input"),
        ProductionMapError::OpeningWipEntryMismatch => bad_request("opening_wip_entry_mismatch"),
        ProductionMapError::OpeningWipLocationMismatch => {
            bad_request("opening_wip_location_mismatch")
        }
        ProductionMapError::OpeningWipSourceMismatch => bad_request("opening_wip_source_mismatch"),
        ProductionMapError::OpeningWipSourceFinalStage => {
            bad_request("opening_wip_source_final_stage")
        }
        ProductionMapError::OpeningWipTargetStageAlreadyCompleted => {
            conflict("opening_wip_target_stage_already_completed")
        }
        ProductionMapError::OpeningWipOrderAlreadyStarted => {
            conflict("opening_wip_order_already_started")
        }
        ProductionMapError::OpeningWipIdempotencyConflict => {
            conflict("opening_wip_idempotency_conflict")
        }
        ProductionMapError::OpeningWipDeleteLocked => conflict("opening_wip_delete_locked"),
        ProductionMapError::OpeningWipDeleteForbidden => forbidden(),
        ProductionMapError::PaddonInvalidInput => bad_request("paddon_invalid_input"),
        ProductionMapError::PaddonRollKindConflict => conflict("paddon_roll_kind_conflict"),
        ProductionMapError::PaddonLocked => conflict("paddon_locked"),
        ProductionMapError::PaddonUnlockForbidden => (
            StatusCode::FORBIDDEN,
            Json(AdminErrorResponse::new("paddon_unlock_forbidden")),
        ),
        ProductionMapError::PaddonTransferForbidden => (
            StatusCode::FORBIDDEN,
            Json(AdminErrorResponse::new("paddon_transfer_forbidden")),
        ),
        ProductionMapError::ActivePaddonRequired => conflict("active_paddon_required"),
        ProductionMapError::PaddonReceiptConflict => conflict("paddon_receipt_conflict"),
        ProductionMapError::PaddonAlreadyReceived => conflict("paddon_already_received"),
        ProductionMapError::PaddonCodeExhausted => conflict("paddon_code_exhausted"),
        ProductionMapError::PaddonNotFound => not_found("paddon_not_found"),
        ProductionMapError::PaddonDeleteLocked => conflict("paddon_delete_locked"),
        ProductionMapError::PaddonItemAlreadyAssigned => conflict("paddon_item_already_assigned"),
        ProductionMapError::PaddonItemNotAssigned => bad_request("paddon_item_not_assigned"),
        ProductionMapError::CapacityProfileInvalid => bad_request("capacity_profile_invalid"),
        ProductionMapError::CapacityProfileNotFound => not_found("capacity_profile_not_found"),
        ProductionMapError::CapabilityNotSupported => bad_request("capability_not_supported"),
        ProductionMapError::CapabilityLevelInsufficient => {
            conflict("capability_level_insufficient")
        }
        ProductionMapError::CapacityConflict => conflict("capacity_conflict"),
        ProductionMapError::CapacityNoWorkingWindow => conflict("capacity_no_working_window"),
        ProductionMapError::CapacityUnavailable => conflict("capacity_unavailable"),
        ProductionMapError::ScheduleInputInvalid => bad_request("schedule_input_invalid"),
        ProductionMapError::ScheduleIdempotencyConflict => {
            conflict("schedule_idempotency_conflict")
        }
        ProductionMapError::ScheduleReservationNotFound => {
            not_found("schedule_reservation_not_found")
        }
        ProductionMapError::ScheduleReservationLocked => conflict("schedule_reservation_locked"),
    }
}
