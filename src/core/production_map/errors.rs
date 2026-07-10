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
    #[error("store failed")]
    StoreFailed,
    #[error("queue action is not allowed")]
    QueueActionNotAllowed,
    #[error("previous production stage is not completed")]
    PreviousStageNotCompleted,
    #[error("apparatus is not assigned to this operator")]
    ApparatusNotAssigned,
    #[error("laminatsiya is not allowed when rubber size is above 1050")]
    LaminatsiyaRubberTooLarge,
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
    #[error("raw material scan is required")]
    RawMaterialScanRequired,
    #[error("raw material scan does not match assigned material")]
    RawMaterialMismatch,
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
    #[error("rezka progress metrics are required")]
    RezkaProgressMetricsRequired,
    #[error("progress batch not found")]
    ProgressBatchNotFound,
    #[error("progress batch does not match previous stage")]
    ProgressBatchNotAccepted,
    #[error("progress batch cannot resume")]
    ProgressBatchNotResumable,
}
