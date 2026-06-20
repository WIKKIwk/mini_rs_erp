use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapDefinition {
    pub id: String,
    pub product_code: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub order_number: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roll_count: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_length: Option<f64>,
    #[serde(default)]
    pub nodes: Vec<ProductionMapNode>,
    #[serde(default)]
    pub edges: Vec<ProductionMapEdge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapNode {
    pub id: String,
    pub kind: ProductionMapNodeKind,
    pub title: String,
    #[serde(default)]
    pub formula: Option<ProductionFormula>,
    #[serde(default)]
    pub role_code: String,
    #[serde(default)]
    pub item_code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub qty_formula: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from_location: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to_location: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alternative_group_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alternative_group_label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alternative_assigned_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_kadr_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_label_length: Option<f64>,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductionMapNodeKind {
    Start,
    Location,
    Material,
    Apparatus,
    KkProduct,
    Formula,
    Condition,
    Task,
    Wait,
    Output,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionFormula {
    pub target: String,
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionMapEdge {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionMapProgram {
    pub map_id: String,
    pub product_code: String,
    pub operations: Vec<ProductionMapOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionMapOperation {
    pub order: usize,
    pub node_id: String,
    pub op_code: String,
    pub args: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapSaved {
    pub map: ProductionMapDefinition,
    pub program: ProductionMapProgram,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapMoveRequest {
    #[serde(default)]
    pub map_id: String,
    #[serde(default)]
    pub from_apparatus: String,
    #[serde(default)]
    pub to_apparatus: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapBatchMoveRequest {
    #[serde(default)]
    pub from_apparatus: String,
    #[serde(default)]
    pub to_apparatus: String,
    #[serde(default)]
    pub map_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapRunRequest {
    #[serde(default)]
    pub map_id: String,
    #[serde(default)]
    pub product_code: String,
    pub order_qty: f64,
    #[serde(default)]
    pub variables: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionTaskDraft {
    pub order: usize,
    pub node_id: String,
    pub task_kind: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role_code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub item_code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from_location: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to_location: String,
    pub qty: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapRunResult {
    pub map_id: String,
    pub product_code: String,
    pub order_qty: f64,
    pub variables: BTreeMap<String, f64>,
    pub tasks: Vec<ProductionTaskDraft>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visited_node_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub awaiting_node_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub awaiting_variable: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub awaiting_expression: String,
}
