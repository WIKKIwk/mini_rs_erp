use thiserror::Error;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ProductionMapError {
    #[error("map id is required")]
    MissingId,
    #[error("product code is required")]
    MissingProductCode,
    #[error("map title is required")]
    MissingTitle,
    #[error("map needs one start node")]
    MissingStart,
    #[error("map needs one end node")]
    MissingEnd,
    #[error("duplicate node id: {0}")]
    DuplicateNode(String),
    #[error("order number already belongs to another zakaz")]
    DuplicateOrderNumber,
    #[error("order number cannot be changed")]
    OrderNumberImmutable,
    #[error("order number sequence is exhausted")]
    OrderNumberExhausted,
    #[error("edge references missing node: {0}")]
    MissingEdgeNode(String),
    #[error("map has a cycle")]
    Cycle,
    #[error("formula target is required")]
    MissingFormulaTarget,
    #[error("formula expression is required")]
    MissingFormulaExpression,
    #[error("invalid formula target: {0}")]
    InvalidFormulaTarget(String),
    #[error("invalid formula expression: {0}")]
    InvalidFormulaExpression(String),
    #[error("map not found")]
    MapNotFound,
    #[error("order quantity must be positive")]
    InvalidOrderQty,
    #[error("node quantity must be positive: {0}")]
    InvalidNodeQty(String),
    #[error("invalid location: {0}")]
    InvalidLocation(String),
    #[error("unknown formula variable: {0}")]
    UnknownFormulaVariable(String),
    #[error("formula division by zero")]
    FormulaDivisionByZero,
    #[error("condition needs true and false branches")]
    MissingConditionBranch,
    #[error("order is not allowed on the target apparatus")]
    MoveNotAllowed,
    #[error("started order requires an apparatus transfer")]
    StartedOrderMoveRequiresTransfer,
    #[error("started production map stages cannot be changed")]
    StartedProductionMapStageLocked,
    #[error("apparatus transfer reason is required")]
    ApparatusTransferReasonRequired,
    #[error("apparatus transfer requires a stable idempotency key")]
    ApparatusTransferIdempotencyRequired,
    #[error("apparatus transfer idempotency key belongs to another request")]
    ApparatusTransferIdempotencyConflict,
    #[error("apparatus transfer is allowed only for a paused order")]
    ApparatusTransferOrderNotPaused,
    #[error("apparatus transfer session was not found")]
    ApparatusTransferSessionNotFound,
    #[error("apparatus transfer progress batch was not found")]
    ApparatusTransferProgressNotFound,
    #[error("apparatus transfer session does not match the paused order")]
    ApparatusTransferSessionMismatch,
    #[error("apparatus transfer progress batch does not match the paused session")]
    ApparatusTransferProgressMismatch,
    #[error("apparatus transfer target already contains the order")]
    ApparatusTransferTargetConflict,
    #[error("store failed")]
    StoreFailed,
    #[error("queue action is not allowed")]
    QueueActionNotAllowed,
    #[error("queue sequence order was not found: {0}")]
    QueueSequenceOrderNotFound(String),
    #[error("queue sequence order is not assigned to the apparatus: {0}")]
    QueueSequenceApparatusMismatch(String),
    #[error("order has not started")]
    OrderNotStarted,
    #[error("order is already completed")]
    OrderAlreadyCompleted,
    #[error("order freeze acknowledgement is pending")]
    OrderFreezeRequested,
    #[error("order is frozen")]
    OrderFrozen,
    #[error("order control action is not allowed")]
    OrderControlActionNotAllowed,
    #[error("active worker session for freeze request was not found")]
    OrderFreezeTargetNotFound,
    #[error("multiple active worker sessions exist for freeze request")]
    OrderFreezeTargetAmbiguous,
    #[error("freeze request does not belong to this worker session")]
    OrderFreezeRequestMismatch,
    #[error("order cannot be deleted")]
    OrderDeleteBlocked(Vec<super::types::OrderDeleteBlocker>),
    #[error("previous production stage is not completed")]
    PreviousStageNotCompleted,
    #[error("apparatus is not assigned to this operator")]
    ApparatusNotAssigned,
    #[error("order width exceeds the canonical apparatus capability")]
    ApparatusWidthExceedsCapability,
    #[error("apparatus queue policy is locked")]
    ApparatusQueuePolicyLocked,
    #[error("raw material input is invalid")]
    RawMaterialInvalidInput,
    #[error("raw material group is not allowed for this order")]
    RawMaterialGroupNotAllowed,
    #[error("raw material group matches multiple apparatus")]
    RawMaterialGroupAmbiguous(Vec<String>),
    #[error("raw material is already assigned")]
    RawMaterialAlreadyAssigned,
    #[error("raw material is already assigned to this order")]
    RawMaterialAlreadyAssignedToOrder,
    #[error("raw material assignment is required")]
    RawMaterialAssignmentNotFound,
    #[error("raw material assignment cannot be unlinked after stock is used")]
    RawMaterialAssignmentLocked,
    #[error("raw material stock is unavailable")]
    RawMaterialStockUnavailable,
    #[error("raw material can only be received while the order is active")]
    RawMaterialOrderNotActive,
    #[error("qolip location not found")]
    QolipLocationNotFound,
    #[error("qolip does not match order product")]
    QolipCodeMismatch,
    #[error("qolip is already in use by another apparatus")]
    QolipAlreadyInUse,
    #[error("qolip stock is insufficient")]
    QolipInsufficientStock,
    #[error("qolip location identity does not match")]
    QolipLocationIdentityMismatch,
    #[error("raw material scan is required")]
    RawMaterialScanRequired,
    #[error("raw material scan does not match assigned material")]
    RawMaterialMismatch,
    #[error("raw material has not been staged at the apparatus state")]
    RawMaterialStateNotReady,
    #[error("all raw materials staged at the apparatus state must be scanned")]
    RawMaterialScanIncomplete,
    #[error("raw material group requirements are not met")]
    RawMaterialRequirementNotMet,
    #[error("raw material roll size is missing")]
    RawMaterialRollSizeMissing,
    #[error("raw material roll size does not match order width")]
    RawMaterialRollSizeMismatch,
    #[error("progress input is invalid")]
    ProgressInputInvalid,
    #[error("previous stage progress qr is required")]
    ProgressQrRequired,
    #[error("bosma completion metrics are required")]
    BosmaCompletionMetricsRequired,
    #[error("laminatsiya completion metrics are required")]
    LaminatsiyaCompletionMetricsRequired,
    #[error("laminatsiya astatka metrics are required")]
    LaminatsiyaAstatkaMetricsRequired,
    #[error("rezka astatka metrics are required")]
    RezkaAstatkaMetricsRequired,
    #[error("rezka progress metrics are required")]
    RezkaProgressMetricsRequired,
    #[error("rezka kadr count is required")]
    RezkaKadrCountRequired,
    #[error("rezka frame input count does not match kadr count")]
    RezkaFrameCountMismatch,
    #[error("rezka final roll is required")]
    RezkaFinalRollRequired,
    #[error("progress batch not found")]
    ProgressBatchNotFound,
    #[error("progress batch does not match previous stage")]
    ProgressBatchNotAccepted,
    #[error("progress batch cannot resume")]
    ProgressBatchNotResumable,
    #[error("progress batch correction reason is required")]
    ProgressBatchCorrectionReasonRequired,
    #[error("progress batch correction is forbidden")]
    ProgressBatchCorrectionForbidden,
    #[error("progress batch can no longer be corrected")]
    ProgressBatchCorrectionLocked,
    #[error("progress batch correction revision conflicts")]
    ProgressBatchCorrectionConflict,
    #[error("progress batch correction has no changes")]
    ProgressBatchCorrectionUnchanged,
    #[error("opening WIP input is invalid")]
    OpeningWipInvalidInput,
    #[error("opening WIP entry apparatus is not the first production-map apparatus")]
    OpeningWipEntryMismatch,
    #[error("opening WIP current location is not a production-map apparatus")]
    OpeningWipLocationMismatch,
    #[error("opening WIP source apparatus is not the selected production-map stage")]
    OpeningWipSourceMismatch,
    #[error("opening WIP source apparatus has no next production-map stage")]
    OpeningWipSourceFinalStage,
    #[error("opening WIP cannot be added after the order has started")]
    OpeningWipOrderAlreadyStarted,
    #[error("opening WIP idempotency key belongs to another request")]
    OpeningWipIdempotencyConflict,
    #[error("paddon input is invalid")]
    PaddonInvalidInput,
    #[error("paddon code sequence is exhausted")]
    PaddonCodeExhausted,
    #[error("paddon was not found")]
    PaddonNotFound,
    #[error("progress batch is already assigned to another paddon")]
    PaddonItemAlreadyAssigned,
    #[error("progress batch is not assigned to this paddon")]
    PaddonItemNotAssigned,
    #[error("capacity profile is invalid")]
    CapacityProfileInvalid,
    #[error("capacity profile was not found")]
    CapacityProfileNotFound,
    #[error("apparatus does not support the requested capability")]
    CapabilityNotSupported,
    #[error("apparatus capability level is insufficient")]
    CapabilityLevelInsufficient,
    #[error("apparatus capacity is fully reserved")]
    CapacityConflict,
    #[error("apparatus has no working window for the requested duration")]
    CapacityNoWorkingWindow,
    #[error("apparatus is unavailable during the requested time")]
    CapacityUnavailable,
    #[error("schedule reservation input is invalid")]
    ScheduleInputInvalid,
    #[error("schedule reservation idempotency key conflicts with another order")]
    ScheduleIdempotencyConflict,
    #[error("schedule reservation was not found")]
    ScheduleReservationNotFound,
    #[error("schedule reservation cannot be cancelled")]
    ScheduleReservationLocked,
}
