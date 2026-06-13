use async_trait::async_trait;
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::formula::{CalculateRequest, LayerInput, calculate};
use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapNode, ProductionMapNodeKind,
};
use crate::erpdb::reader::DirectDbReader;
use crate::erpnext::client::ErpnextClient;

#[async_trait]
pub trait ProductionOrderErpSink: Send + Sync {
    async fn save_order(
        &self,
        map: &ProductionMapDefinition,
        template: &CalculateOrderTemplate,
    ) -> Result<(), ProductionOrderErpError>;
}

#[async_trait]
pub trait ProductionOrderErpSource: Send + Sync {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionOrderErpError>;
}

#[derive(Debug, Clone, Copy)]
pub struct NoopProductionOrderErpSink;

#[async_trait]
impl ProductionOrderErpSink for NoopProductionOrderErpSink {
    async fn save_order(
        &self,
        _map: &ProductionMapDefinition,
        _template: &CalculateOrderTemplate,
    ) -> Result<(), ProductionOrderErpError> {
        Ok(())
    }
}

#[async_trait]
impl ProductionOrderErpSource for NoopProductionOrderErpSink {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionOrderErpError> {
        Ok(Vec::new())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProductionOrderErpError {
    #[error("erp production order write failed: {0}")]
    WriteFailed(String),
}

#[async_trait]
impl ProductionOrderErpSink for ErpnextClient {
    async fn save_order(
        &self,
        map: &ProductionMapDefinition,
        template: &CalculateOrderTemplate,
    ) -> Result<(), ProductionOrderErpError> {
        if template.kg <= 0.0 {
            return Ok(());
        }
        let item_code = first_non_empty([&template.item_code, &map.product_code]);
        if item_code.is_empty() {
            return Err(ProductionOrderErpError::WriteFailed(
                "item code is empty".to_string(),
            ));
        }
        let fg_warehouse = self.resolve_fg_warehouse().await?;
        let company = self.erp_warehouse_company(&fg_warehouse).await?;
        let product_name = first_non_empty([&template.product, &template.name, &map.title]);
        let item = self
            .ensure_item_exists(&item_code, &product_name, "Tayyor mahsulot", "Kg")
            .await?;
        let bom_no = if item.default_bom.trim().is_empty() {
            self.create_submitted_bom(map, template, &item_code, &item.stock_uom, &company)
                .await?
        } else {
            item.default_bom
        };
        let mut payload = build_work_order_payload(map, template);
        payload["company"] = Value::String(company);
        payload["fg_warehouse"] = Value::String(fg_warehouse);
        payload["bom_no"] = Value::String(bom_no);
        payload["stock_uom"] = Value::String(item.stock_uom);
        payload["planned_start_date"] = Value::String(now_erp_datetime());
        let _: ResourceResponse<NameRow> = self
            .production_order_json_request(Method::POST, "/api/resource/Work Order", Some(payload))
            .await?;
        Ok(())
    }
}

#[async_trait]
impl ProductionOrderErpSource for ErpnextClient {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionOrderErpError> {
        let mut start = 0usize;
        let mut documents = Vec::new();
        loop {
            let response: ListResponse<NameRow> = self
                .production_order_get(
                    "/api/resource/Work Order",
                    &[
                        ("fields", r#"["name"]"#.to_string()),
                        (
                            "filters",
                            r#"[["Work Order","description","like","%RS map id:%"]]"#.to_string(),
                        ),
                        ("limit_start", start.to_string()),
                        ("limit_page_length", "500".to_string()),
                    ],
                )
                .await?;
            if response.data.is_empty() {
                break;
            }
            for row in response.data {
                let document: ResourceResponse<WorkOrderDocument> = self
                    .production_order_json_request(
                        Method::GET,
                        &format!(
                            "/api/resource/Work Order/{}",
                            urlencoding::encode(row.name.trim())
                        ),
                        None,
                    )
                    .await?;
                documents.push(document.data);
            }
            start += 500;
        }
        Ok(work_order_documents_to_maps(documents))
    }
}

#[async_trait]
impl ProductionOrderErpSource for DirectDbReader {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionOrderErpError> {
        let headers = sqlx::query_as::<_, WorkOrderHeaderDbRow>(
            r#"
            SELECT
                COALESCE(name, '') AS name,
                COALESCE(production_item, '') AS production_item,
                COALESCE(description, '') AS description
            FROM `tabWork Order`
            WHERE COALESCE(description, '') LIKE '%RS map id:%'
            ORDER BY modified DESC, name DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| ProductionOrderErpError::WriteFailed(error.to_string()))?;
        let mut documents = Vec::with_capacity(headers.len());
        for header in headers {
            let operations = sqlx::query_as::<_, WorkOrderOperationDbRow>(
                r#"
                SELECT
                    COALESCE(workstation, '') AS workstation,
                    COALESCE(description, '') AS description,
                    CAST(COALESCE(sequence_id, idx, 0) AS SIGNED) AS sequence_id,
                    CAST(COALESCE(batch_size, 0) AS DOUBLE) AS batch_size
                FROM `tabWork Order Operation`
                WHERE parent = ?
                ORDER BY sequence_id ASC, idx ASC, name ASC
                "#,
            )
            .bind(&header.name)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| ProductionOrderErpError::WriteFailed(error.to_string()))?;
            documents.push(WorkOrderDocument {
                name: header.name,
                production_item: header.production_item,
                description: header.description,
                operations: operations
                    .into_iter()
                    .map(|operation| WorkOrderOperationRow {
                        workstation: operation.workstation,
                        description: operation.description,
                        sequence_id: operation.sequence_id,
                        batch_size: operation.batch_size,
                    })
                    .collect(),
            });
        }
        Ok(work_order_documents_to_maps(documents))
    }
}

impl ErpnextClient {
    async fn create_submitted_bom(
        &self,
        map: &ProductionMapDefinition,
        template: &CalculateOrderTemplate,
        item_code: &str,
        item_uom: &str,
        company: &str,
    ) -> Result<String, ProductionOrderErpError> {
        self.ensure_bom_master_data(map, template).await?;
        let payload = build_bom_payload(map, template, company, item_uom);
        let created: ResourceResponse<NameRow> = self
            .production_order_json_request(Method::POST, "/api/resource/BOM", Some(payload))
            .await?;
        self.submit_production_doc("BOM", &created.data.name)
            .await?;
        let item = self.erp_item(item_code).await?;
        if !item.default_bom.trim().is_empty() {
            return Ok(item.default_bom);
        }
        Ok(created.data.name)
    }

    async fn ensure_bom_master_data(
        &self,
        map: &ProductionMapDefinition,
        template: &CalculateOrderTemplate,
    ) -> Result<(), ProductionOrderErpError> {
        for layer in bom_material_layers(template) {
            self.ensure_item_exists(&layer.item_code, &layer.item_code, "Xomashyo", "Kg")
                .await?;
        }
        for node in apparatus_nodes_in_flow_order(map) {
            let workstation = workstation_name(node);
            self.ensure_workstation(&workstation).await?;
            self.ensure_operation(operation_name(&node.title)).await?;
        }
        Ok(())
    }

    async fn ensure_item_exists(
        &self,
        item_code: &str,
        item_name: &str,
        item_group: &str,
        stock_uom: &str,
    ) -> Result<ItemRow, ProductionOrderErpError> {
        match self.erp_item(item_code).await {
            Ok(item) => Ok(item),
            Err(error) if is_not_found_error(&error) => {
                let payload = serde_json::json!({
                    "item_code": item_code.trim(),
                    "item_name": first_non_empty([item_name, item_code]),
                    "item_group": item_group.trim(),
                    "stock_uom": stock_uom.trim(),
                    "is_stock_item": 1,
                    "include_item_in_manufacturing": 1,
                });
                let _: ResourceResponse<NameRow> = self
                    .production_order_json_request(
                        Method::POST,
                        "/api/resource/Item",
                        Some(payload),
                    )
                    .await?;
                self.erp_item(item_code).await
            }
            Err(error) => Err(error),
        }
    }

    async fn ensure_workstation(&self, name: &str) -> Result<(), ProductionOrderErpError> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(());
        }
        self.ensure_resource_exists(
            "Workstation",
            name,
            serde_json::json!({
                "workstation_name": name,
                "production_capacity": 1,
                "description": "RS production map workstation",
            }),
        )
        .await
    }

    async fn ensure_operation(&self, name: &str) -> Result<(), ProductionOrderErpError> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(());
        }
        self.ensure_resource_exists(
            "Operation",
            name,
            serde_json::json!({
                "__newname": name,
                "name": name,
                "description": "RS production map operation",
                "batch_size": 1,
            }),
        )
        .await
    }

    async fn ensure_resource_exists(
        &self,
        doctype: &str,
        name: &str,
        payload: Value,
    ) -> Result<(), ProductionOrderErpError> {
        if self.resource_exists(doctype, name).await? {
            return Ok(());
        }
        let _: ResourceResponse<NameRow> = self
            .production_order_json_request(
                Method::POST,
                &format!("/api/resource/{}", urlencoding::encode(doctype.trim())),
                Some(payload),
            )
            .await?;
        Ok(())
    }

    async fn resource_exists(
        &self,
        doctype: &str,
        name: &str,
    ) -> Result<bool, ProductionOrderErpError> {
        let response = self
            .http
            .request(
                Method::GET,
                format!(
                    "{}/api/resource/{}/{}",
                    self.base_url(),
                    urlencoding::encode(doctype.trim()),
                    urlencoding::encode(name.trim())
                ),
            )
            .header(reqwest::header::AUTHORIZATION, self.auth_header().await)
            .send()
            .await
            .map_err(|error| ProductionOrderErpError::WriteFailed(error.to_string()))?;
        if response.status().as_u16() == 404 {
            return Ok(false);
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .map_err(|error| ProductionOrderErpError::WriteFailed(error.to_string()))?;
            return Err(ProductionOrderErpError::WriteFailed(format!(
                "status {status}: {body}"
            )));
        }
        Ok(true)
    }

    async fn submit_production_doc(
        &self,
        doctype: &str,
        name: &str,
    ) -> Result<(), ProductionOrderErpError> {
        for attempt in 0..2 {
            let latest: ResourceResponse<Value> = self
                .production_order_json_request(
                    Method::GET,
                    &format!(
                        "/api/resource/{}/{}",
                        urlencoding::encode(doctype.trim()),
                        urlencoding::encode(name.trim())
                    ),
                    None,
                )
                .await?;
            if erp_doc_is_submitted(&latest.data) {
                return Ok(());
            }
            let result: Result<Value, ProductionOrderErpError> = self
                .production_order_json_request(
                    Method::POST,
                    "/api/method/frappe.client.submit",
                    Some(serde_json::json!({ "doc": latest.data })),
                )
                .await;
            match result {
                Ok(_) => return Ok(()),
                Err(ProductionOrderErpError::WriteFailed(message))
                    if attempt == 0 && message.contains("TimestampMismatchError") =>
                {
                    continue;
                }
                Err(error) => {
                    if self
                        .production_doc_is_submitted(doctype, name)
                        .await
                        .unwrap_or(false)
                    {
                        return Ok(());
                    }
                    return Err(error);
                }
            }
        }
        Err(ProductionOrderErpError::WriteFailed(format!(
            "{doctype} submit failed: {name}"
        )))
    }

    async fn production_doc_is_submitted(
        &self,
        doctype: &str,
        name: &str,
    ) -> Result<bool, ProductionOrderErpError> {
        let latest: ResourceResponse<Value> = self
            .production_order_json_request(
                Method::GET,
                &format!(
                    "/api/resource/{}/{}",
                    urlencoding::encode(doctype.trim()),
                    urlencoding::encode(name.trim())
                ),
                None,
            )
            .await?;
        Ok(erp_doc_is_submitted(&latest.data))
    }

    async fn erp_item(&self, item_code: &str) -> Result<ItemRow, ProductionOrderErpError> {
        let response: ResourceResponse<ItemRow> = self
            .production_order_json_request(
                Method::GET,
                &format!(
                    "/api/resource/Item/{}",
                    urlencoding::encode(item_code.trim())
                ),
                None,
            )
            .await?;
        Ok(response.data)
    }

    async fn resolve_fg_warehouse(&self) -> Result<String, ProductionOrderErpError> {
        let configured = self.default_warehouse();
        if !configured.trim().is_empty() {
            return Ok(configured.trim().to_string());
        }
        let response: ListResponse<NameRow> = self
            .production_order_get(
                "/api/resource/Warehouse",
                &[
                    ("fields", r#"["name"]"#.to_string()),
                    ("limit_page_length", "1".to_string()),
                ],
            )
            .await?;
        response
            .data
            .into_iter()
            .map(|row| row.name.trim().to_string())
            .find(|name| !name.is_empty())
            .ok_or_else(|| ProductionOrderErpError::WriteFailed("warehouse not found".to_string()))
    }

    async fn erp_warehouse_company(
        &self,
        warehouse: &str,
    ) -> Result<String, ProductionOrderErpError> {
        let response: ResourceResponse<WarehouseRow> = self
            .production_order_json_request(
                Method::GET,
                &format!(
                    "/api/resource/Warehouse/{}",
                    urlencoding::encode(warehouse.trim())
                ),
                None,
            )
            .await?;
        let company = response.data.company.trim().to_string();
        if company.is_empty() {
            Err(ProductionOrderErpError::WriteFailed(format!(
                "warehouse company is empty: {}",
                warehouse.trim()
            )))
        } else {
            Ok(company)
        }
    }

    async fn production_order_get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, ProductionOrderErpError> {
        let response = self
            .http
            .get(format!("{}{}", self.base_url(), encoded_path(path)))
            .header(reqwest::header::AUTHORIZATION, self.auth_header().await)
            .query(query)
            .send()
            .await
            .map_err(|error| ProductionOrderErpError::WriteFailed(error.to_string()))?;
        decode_response(response).await
    }

    async fn production_order_json_request<T: for<'de> Deserialize<'de>>(
        &self,
        method: Method,
        path: &str,
        payload: Option<Value>,
    ) -> Result<T, ProductionOrderErpError> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url(), encoded_path(path)))
            .header(reqwest::header::AUTHORIZATION, self.auth_header().await);
        if let Some(payload) = payload {
            request = request.json(&payload);
        }
        let response = request
            .send()
            .await
            .map_err(|error| ProductionOrderErpError::WriteFailed(error.to_string()))?;
        decode_response(response).await
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn build_work_order_payload(
    map: &ProductionMapDefinition,
    template: &CalculateOrderTemplate,
) -> Value {
    let item_code = first_non_empty([&template.item_code, &map.product_code]);
    let order_number = first_non_empty([
        &map.order_number,
        &map.code,
        &template.order_number,
        &template.code,
    ]);
    let product_name = first_non_empty([&template.product, &template.name, &map.title]);
    let roll_count = template.roll_count.or(map.roll_count).unwrap_or_default();
    let rubber_size = rubber_size_mm(template).unwrap_or_default();
    let operations: Vec<Value> = apparatus_nodes_in_flow_order(map)
        .into_iter()
        .enumerate()
        .map(|(index, node)| {
            serde_json::json!({
                "operation": operation_name(&node.title),
                "workstation": workstation_name(node),
                "description": operation_description(map, template, &order_number, rubber_size),
                "sequence_id": index + 1,
                "batch_size": roll_count,
                "time_in_mins": 1,
                "status": "Pending",
            })
        })
        .collect();
    serde_json::json!({
        "production_item": item_code,
        "qty": template.kg,
        "description": format!("Zakaz: /{order_number}\nMahsulot: {product_name}\nRS map id: {}", map.id.trim()),
        "skip_transfer": 1,
        "transfer_material_against": "Job Card",
        "operations": operations,
    })
}

fn build_bom_payload(
    map: &ProductionMapDefinition,
    template: &CalculateOrderTemplate,
    company: &str,
    item_uom: &str,
) -> Value {
    let item_code = first_non_empty([&template.item_code, &map.product_code]);
    let order_number = first_non_empty([
        &map.order_number,
        &map.code,
        &template.order_number,
        &template.code,
    ]);
    let roll_count = template.roll_count.or(map.roll_count).unwrap_or_default();
    let rubber_size = rubber_size_mm(template).unwrap_or_default();
    let items = bom_material_items(template);
    let operations: Vec<Value> = apparatus_nodes_in_flow_order(map)
        .into_iter()
        .enumerate()
        .map(|(index, node)| {
            serde_json::json!({
                "operation": operation_name(&node.title),
                "workstation": workstation_name(node),
                "description": operation_description(map, template, &order_number, rubber_size),
                "sequence_id": index + 1,
                "batch_size": roll_count,
                "time_in_mins": 1,
            })
        })
        .collect();
    serde_json::json!({
        "item": item_code,
        "company": company.trim(),
        "quantity": 1.0,
        "uom": first_non_empty([item_uom, "Kg"]),
        "is_active": 1,
        "is_default": 1,
        "with_operations": if operations.is_empty() { 0 } else { 1 },
        "transfer_material_against": "Job Card",
        "rm_cost_as_per": "Valuation Rate",
        "items": items,
        "operations": operations,
    })
}

fn bom_material_items(template: &CalculateOrderTemplate) -> Vec<Value> {
    bom_material_layers(template)
        .into_iter()
        .map(|layer| {
            serde_json::json!({
                "item_code": layer.item_code,
                "qty": layer.qty,
                "uom": "Kg",
                "stock_uom": "Kg",
                "rate": 0.0,
                "include_item_in_manufacturing": 1,
                "description": format!("{}: {} {}", layer.layer, layer.item_code, number_text(layer.micron)),
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
struct BomMaterialLayer {
    layer: &'static str,
    item_code: String,
    micron: f64,
    qty: f64,
}

fn bom_material_layers(template: &CalculateOrderTemplate) -> Vec<BomMaterialLayer> {
    let mut layers = Vec::new();
    layers.extend(expand_bom_material_layer(
        "1 qavat",
        &template.first_layer_material,
        &template.first_layer_micron,
    ));
    layers.extend(expand_bom_material_layer(
        "2 qavat",
        &template.second_layer_material,
        &template.second_layer_micron,
    ));
    layers.extend(expand_bom_material_layer(
        "3 qavat",
        &template.third_layer_material,
        &template.third_layer_micron,
    ));
    let positive_total = layers
        .iter()
        .map(|layer| layer.micron)
        .filter(|micron| *micron > 0.0)
        .sum::<f64>();
    if positive_total > 0.0 {
        for layer in &mut layers {
            layer.qty = layer.micron / positive_total;
        }
    } else if !layers.is_empty() {
        let equal_qty = 1.0 / layers.len() as f64;
        for layer in &mut layers {
            layer.qty = equal_qty;
        }
    }
    layers
}

fn expand_bom_material_layer(
    layer: &'static str,
    material: &str,
    micron_text: &str,
) -> Vec<BomMaterialLayer> {
    let material = material.trim();
    if material.is_empty() {
        return Vec::new();
    }
    let materials = split_layer_parts(material);
    let microns = parse_micron_numbers(micron_text);
    if materials.len() > 1 && microns.len() == materials.len() {
        return materials
            .into_iter()
            .zip(microns)
            .map(|(item_code, micron)| BomMaterialLayer {
                layer,
                item_code,
                micron,
                qty: 0.0,
            })
            .collect();
    }
    vec![BomMaterialLayer {
        layer,
        item_code: material.to_string(),
        micron: microns.into_iter().reduce(f64::max).unwrap_or_default(),
        qty: 0.0,
    }]
}

fn split_layer_parts(value: &str) -> Vec<String> {
    value
        .split('/')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_micron_numbers(value: &str) -> Vec<f64> {
    value
        .trim()
        .split('/')
        .filter_map(|part| part.trim().parse::<f64>().ok())
        .collect()
}

fn apparatus_nodes_in_flow_order(map: &ProductionMapDefinition) -> Vec<&ProductionMapNode> {
    let mut nodes: Vec<&ProductionMapNode> = map
        .nodes
        .iter()
        .filter(|node| node.kind == ProductionMapNodeKind::Apparatus)
        .collect();
    nodes.sort_by(|left, right| {
        node_depth(map, &left.id)
            .cmp(&node_depth(map, &right.id))
            .then_with(|| left.y.total_cmp(&right.y))
            .then_with(|| left.x.total_cmp(&right.x))
            .then_with(|| left.id.cmp(&right.id))
    });
    nodes
}

fn node_depth(map: &ProductionMapDefinition, node_id: &str) -> usize {
    let start_ids: Vec<&str> = map
        .nodes
        .iter()
        .filter(|node| node.kind == ProductionMapNodeKind::Start)
        .map(|node| node.id.as_str())
        .collect();
    let mut frontier = start_ids;
    let mut seen = std::collections::BTreeSet::new();
    let mut depth = 0;
    while !frontier.is_empty() {
        if frontier.contains(&node_id) {
            return depth;
        }
        let mut next = Vec::new();
        for current in frontier {
            if !seen.insert(current.to_string()) {
                continue;
            }
            for edge in map.edges.iter().filter(|edge| edge.from == current) {
                next.push(edge.to.as_str());
            }
        }
        frontier = next;
        depth += 1;
    }
    usize::MAX
}

fn workstation_name(node: &ProductionMapNode) -> String {
    if node.alternative_assigned_title.trim().is_empty() {
        node.title.trim().to_string()
    } else {
        node.alternative_assigned_title.trim().to_string()
    }
}

fn operation_name(title: &str) -> &'static str {
    let lower = title.to_lowercase();
    if lower.contains("laminat") {
        "Laminatsiya"
    } else if lower.contains("flex") || lower.contains("fleks") || lower.contains("flekso") {
        "Flexo"
    } else if lower.contains("pechat") {
        "Pechat"
    } else if lower.contains("paket") {
        "Paket"
    } else {
        "Ishlov"
    }
}

fn operation_description(
    map: &ProductionMapDefinition,
    template: &CalculateOrderTemplate,
    order_number: &str,
    rubber_size: i64,
) -> String {
    let width_mm = if template.width_mm > 0.0 {
        template.width_mm
    } else {
        map.width_mm.unwrap_or_default()
    };
    let roll_count = template.roll_count.or(map.roll_count).unwrap_or_default();
    let mut lines = vec![
        format!("Zakaz: /{}", order_number.trim()),
        format!("Material razmer: {} mm", number_text(width_mm)),
        format!("Rezina razmer: {rubber_size} mm"),
        format!("Qolib soni: {}", number_text(roll_count)),
        format!(
            "1 qavat: {} {}",
            template.first_layer_material.trim(),
            template.first_layer_micron.trim()
        ),
        format!(
            "2 qavat: {} {}",
            template.second_layer_material.trim(),
            template.second_layer_micron.trim()
        ),
    ];
    if !template.third_layer_material.trim().is_empty()
        || !template.third_layer_micron.trim().is_empty()
    {
        lines.push(format!(
            "3 qavat: {} {}",
            template.third_layer_material.trim(),
            template.third_layer_micron.trim()
        ));
    }
    lines.push(format!("RS map id: {}", map.id.trim()));
    lines.join("\n")
}

fn rubber_size_mm(template: &CalculateOrderTemplate) -> Option<i64> {
    let calculation = calculate(CalculateRequest {
        order_number: Some(first_non_empty([&template.order_number, &template.code])),
        product: Some(first_non_empty([&template.product, &template.name])),
        kg: Some(template.kg),
        width_mm: Some(template.width_mm),
        waste_percent: Some(template.waste_percent),
        roll_count: template.roll_count,
        first_layer: LayerInput::new(
            template.first_layer_material.trim(),
            template.first_layer_micron.trim(),
        ),
        second_layer: LayerInput::new(
            template.second_layer_material.trim(),
            template.second_layer_micron.trim(),
        ),
        third_layer: LayerInput::new(
            template.third_layer_material.trim(),
            template.third_layer_micron.trim(),
        ),
        ..CalculateRequest::default()
    })
    .ok()?;
    Some(i64::from(calculation.rubber_size_mm))
}

fn first_non_empty<const N: usize>(values: [&str; N]) -> String {
    values
        .into_iter()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or("")
        .to_string()
}

fn number_text(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn now_erp_datetime() -> String {
    let now = time::OffsetDateTime::now_utc()
        .to_offset(time::UtcOffset::from_hms(5, 0, 0).expect("valid tashkent offset"));
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn is_not_found_error(error: &ProductionOrderErpError) -> bool {
    error.to_string().contains("status 404")
}

fn erp_doc_is_submitted(doc: &Value) -> bool {
    doc.get("docstatus").and_then(Value::as_i64) == Some(1)
        || doc.get("docstatus").and_then(Value::as_f64) == Some(1.0)
}

fn encoded_path(path: &str) -> String {
    path.trim_start_matches(' ').replace(' ', "%20")
}

async fn decode_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, ProductionOrderErpError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| ProductionOrderErpError::WriteFailed(error.to_string()))?;
    if !status.is_success() {
        return Err(ProductionOrderErpError::WriteFailed(format!(
            "status {status}: {body}"
        )));
    }
    serde_json::from_str(&body)
        .map_err(|error| ProductionOrderErpError::WriteFailed(format!("{error}: {body}")))
}

#[derive(Debug, Deserialize)]
struct ResourceResponse<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct ListResponse<T> {
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct NameRow {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ItemRow {
    #[serde(default)]
    default_bom: String,
    #[serde(default)]
    stock_uom: String,
}

#[derive(Debug, Deserialize)]
struct WarehouseRow {
    #[serde(default)]
    company: String,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkOrderDocument {
    #[serde(default)]
    name: String,
    #[serde(default)]
    production_item: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    operations: Vec<WorkOrderOperationRow>,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkOrderOperationRow {
    #[serde(default)]
    workstation: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    sequence_id: i64,
    #[serde(default)]
    batch_size: f64,
}

#[derive(Debug, sqlx::FromRow)]
struct WorkOrderHeaderDbRow {
    name: String,
    production_item: String,
    description: String,
}

#[derive(Debug, sqlx::FromRow)]
struct WorkOrderOperationDbRow {
    workstation: String,
    description: String,
    sequence_id: i64,
    batch_size: f64,
}

fn work_order_document_to_map(document: WorkOrderDocument) -> Option<ProductionMapDefinition> {
    let map_id = description_value(&document.description, "RS map id:")?;
    if map_id.trim().is_empty() {
        return None;
    }
    let order_number = description_value(&document.description, "Zakaz:")
        .unwrap_or_else(|| document.name.trim().to_string())
        .trim_start_matches('/')
        .trim()
        .to_string();
    let title = description_value(&document.description, "Mahsulot:")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| document.production_item.trim().to_string());
    let mut operations = document
        .operations
        .into_iter()
        .filter(|operation| !operation.workstation.trim().is_empty())
        .collect::<Vec<_>>();
    operations.sort_by(|left, right| {
        left.sequence_id
            .cmp(&right.sequence_id)
            .then_with(|| left.workstation.cmp(&right.workstation))
    });
    let roll_count = operations
        .iter()
        .find_map(|operation| (operation.batch_size > 0.0).then_some(operation.batch_size));
    let width_mm = operations
        .iter()
        .find_map(|operation| description_number(&operation.description, "Material razmer:"));
    let mut nodes = vec![
        work_order_node("start", ProductionMapNodeKind::Start, "Start", 0.0, 0.0),
        work_order_node("task", ProductionMapNodeKind::Task, &title, 0.0, 120.0),
    ];
    for (index, operation) in operations.iter().enumerate() {
        nodes.push(work_order_node(
            &format!("apparatus-{}", index + 1),
            ProductionMapNodeKind::Apparatus,
            operation.workstation.trim(),
            0.0,
            240.0 + (index as f64 * 120.0),
        ));
    }
    nodes.push(work_order_node(
        "end",
        ProductionMapNodeKind::End,
        &title,
        0.0,
        240.0 + (operations.len() as f64 * 120.0),
    ));
    let mut edges = vec![work_order_edge("start", "task")];
    let mut previous = "task".to_string();
    for index in 0..operations.len() {
        let next = format!("apparatus-{}", index + 1);
        edges.push(work_order_edge(&previous, &next));
        previous = next;
    }
    edges.push(work_order_edge(&previous, "end"));
    Some(ProductionMapDefinition {
        id: map_id.trim().to_string(),
        product_code: document.production_item.trim().to_string(),
        title,
        code: order_number.clone(),
        order_number,
        roll_count,
        width_mm,
        nodes,
        edges,
    })
}

fn work_order_documents_to_maps(documents: Vec<WorkOrderDocument>) -> Vec<ProductionMapDefinition> {
    documents
        .into_iter()
        .filter_map(work_order_document_to_map)
        .collect()
}

fn description_value(description: &str, key: &str) -> Option<String> {
    description.lines().find_map(|line| {
        line.trim()
            .strip_prefix(key)
            .map(|value| value.trim().to_string())
    })
}

fn description_number(description: &str, key: &str) -> Option<f64> {
    let value = description_value(description, key)?;
    value
        .split_whitespace()
        .next()
        .and_then(|number| number.parse::<f64>().ok())
}

fn work_order_node(
    id: &str,
    kind: ProductionMapNodeKind,
    title: &str,
    x: f64,
    y: f64,
) -> ProductionMapNode {
    ProductionMapNode {
        id: id.to_string(),
        kind,
        title: title.to_string(),
        formula: None,
        role_code: String::new(),
        item_code: String::new(),
        qty_formula: String::new(),
        from_location: String::new(),
        to_location: String::new(),
        alternative_group_id: String::new(),
        alternative_group_label: String::new(),
        alternative_assigned_title: String::new(),
        x,
        y,
    }
}

fn work_order_edge(from: &str, to: &str) -> crate::core::production_map::ProductionMapEdge {
    crate::core::production_map::ProductionMapEdge {
        from: from.to_string(),
        to: to.to_string(),
        branch: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::production_map::{
        ProductionMapEdge, ProductionMapNode, ProductionMapNodeKind,
    };

    #[test]
    fn work_order_payload_maps_order_to_standard_erpnext_manufacturing_fields() {
        let map = ProductionMapDefinition {
            id: "zakaz-8756".to_string(),
            product_code: "ITEM-8756".to_string(),
            title: "vitagum vitamin zip paket".to_string(),
            code: "8756".to_string(),
            order_number: "8756".to_string(),
            roll_count: Some(7.0),
            width_mm: Some(630.0),
            nodes: vec![
                node("start", ProductionMapNodeKind::Start, "Start"),
                node(
                    "pechat",
                    ProductionMapNodeKind::Apparatus,
                    "7 ta rangli pechat - A",
                ),
                node("lam", ProductionMapNodeKind::Apparatus, "Laminatsiya 1 - A"),
                node("end", ProductionMapNodeKind::End, "End"),
            ],
            edges: vec![
                edge("start", "pechat"),
                edge("pechat", "lam"),
                edge("lam", "end"),
            ],
        };
        let template = CalculateOrderTemplate {
            name: "vitagum vitamin zip paket".to_string(),
            product: "vitagum vitamin zip paket".to_string(),
            item_code: "ITEM-8756".to_string(),
            width_mm: 630.0,
            waste_percent: 3.0,
            roll_count: Some(7.0),
            first_layer_material: "pet".to_string(),
            first_layer_micron: "12".to_string(),
            second_layer_material: "pe oq".to_string(),
            second_layer_micron: "50".to_string(),
            third_layer_material: "cpp".to_string(),
            third_layer_micron: "20".to_string(),
            kg: 1000.0,
            ..CalculateOrderTemplate::default()
        };

        let payload = build_work_order_payload(&map, &template);

        assert_eq!(payload["production_item"], "ITEM-8756");
        assert_eq!(payload["qty"], 1000.0);
        assert_eq!(
            payload["description"],
            "Zakaz: /8756\nMahsulot: vitagum vitamin zip paket\nRS map id: zakaz-8756"
        );
        assert_eq!(
            payload["operations"].as_array().expect("operations").len(),
            2
        );
        assert_eq!(payload["operations"][0]["operation"], "Pechat");
        assert_eq!(
            payload["operations"][0]["workstation"],
            "7 ta rangli pechat - A"
        );
        assert_eq!(payload["operations"][0]["sequence_id"], 1);
        assert_eq!(payload["operations"][0]["batch_size"], 7.0);
        assert_eq!(payload["operations"][0]["time_in_mins"], 1);
        assert_eq!(payload["operations"][1]["operation"], "Laminatsiya");
        assert_eq!(payload["operations"][1]["workstation"], "Laminatsiya 1 - A");
        assert_eq!(payload["operations"][1]["sequence_id"], 2);
        let description = payload["operations"][0]["description"]
            .as_str()
            .expect("operation description");
        assert!(description.contains("Material razmer: 630 mm"));
        assert!(description.contains("Rezina razmer: 650 mm"));
        assert!(description.contains("Qolib soni: 7"));
        assert!(description.contains("1 qavat: pet 12"));
        assert!(description.contains("2 qavat: pe oq 50"));
        assert!(description.contains("3 qavat: cpp 20"));
    }

    #[test]
    fn bom_payload_maps_order_to_standard_erpnext_manufacturing_fields() {
        let map = ProductionMapDefinition {
            id: "zakaz-8970".to_string(),
            product_code: "dolce cake".to_string(),
            title: "dolce cake".to_string(),
            code: "8970".to_string(),
            order_number: "8970".to_string(),
            roll_count: Some(7.0),
            width_mm: Some(730.0),
            nodes: vec![
                node("start", ProductionMapNodeKind::Start, "Start"),
                node(
                    "pechat",
                    ProductionMapNodeKind::Apparatus,
                    "7 ta rangli pechat - A",
                ),
                node("lam", ProductionMapNodeKind::Apparatus, "Laminatsiya 1 - A"),
                node("end", ProductionMapNodeKind::End, "End"),
            ],
            edges: vec![
                edge("start", "pechat"),
                edge("pechat", "lam"),
                edge("lam", "end"),
            ],
        };
        let template = CalculateOrderTemplate {
            name: "dolce cake".to_string(),
            product: "dolce cake".to_string(),
            item_code: "dolce cake".to_string(),
            width_mm: 730.0,
            waste_percent: 3.0,
            roll_count: Some(7.0),
            first_layer_material: "pet".to_string(),
            first_layer_micron: "12".to_string(),
            second_layer_material: "pe oq".to_string(),
            second_layer_micron: "50".to_string(),
            kg: 53453.0,
            ..CalculateOrderTemplate::default()
        };

        let payload = build_bom_payload(&map, &template, "Accord", "Kg");

        assert_eq!(payload["item"], "dolce cake");
        assert_eq!(payload["company"], "Accord");
        assert_eq!(payload["quantity"], 1.0);
        assert_eq!(payload["uom"], "Kg");
        assert_eq!(payload["is_active"], 1);
        assert_eq!(payload["is_default"], 1);
        assert_eq!(payload["with_operations"], 1);
        assert_eq!(payload["transfer_material_against"], "Job Card");
        assert_eq!(payload["rm_cost_as_per"], "Valuation Rate");
        assert_eq!(payload["items"].as_array().expect("items").len(), 2);
        assert_eq!(payload["items"][0]["item_code"], "pet");
        assert_eq!(payload["items"][0]["uom"], "Kg");
        assert_eq!(payload["items"][0]["include_item_in_manufacturing"], 1);
        assert_eq!(payload["items"][0]["qty"], 12.0 / 62.0);
        assert_eq!(payload["items"][1]["item_code"], "pe oq");
        assert_eq!(payload["items"][1]["qty"], 50.0 / 62.0);
        assert_eq!(
            payload["operations"].as_array().expect("operations").len(),
            2
        );
        assert_eq!(payload["operations"][0]["operation"], "Pechat");
        assert_eq!(
            payload["operations"][0]["workstation"],
            "7 ta rangli pechat - A"
        );
        assert_eq!(payload["operations"][0]["sequence_id"], 1);
        assert_eq!(payload["operations"][0]["time_in_mins"], 1);
        assert_eq!(payload["operations"][0]["batch_size"], 7.0);
    }

    #[test]
    fn submitted_erpnext_docstatus_counts_as_success_after_submit_transport_error() {
        assert!(erp_doc_is_submitted(&serde_json::json!({ "docstatus": 1 })));
        assert!(erp_doc_is_submitted(
            &serde_json::json!({ "docstatus": 1.0 })
        ));
        assert!(!erp_doc_is_submitted(
            &serde_json::json!({ "docstatus": 0 })
        ));
        assert!(!erp_doc_is_submitted(
            &serde_json::json!({ "docstatus": 2 })
        ));
        assert!(!erp_doc_is_submitted(&serde_json::json!({})));
    }

    #[test]
    fn work_order_document_rebuilds_production_map_for_hybrid_cache() {
        let document = WorkOrderDocument {
            name: "MFG-WO-2026-00007".to_string(),
            production_item: "ITEM-8756".to_string(),
            description: "Zakaz: /8756\nMahsulot: vitagum vitamin zip paket\nRS map id: zakaz-8756"
                .to_string(),
            operations: vec![
                WorkOrderOperationRow {
                    workstation: "8 ta rangli pechat - A".to_string(),
                    description: "Zakaz: /8756\nMaterial razmer: 630 mm\nQolib soni: 7".to_string(),
                    sequence_id: 1,
                    batch_size: 7.0,
                },
                WorkOrderOperationRow {
                    workstation: "Laminatsiya 1 - A".to_string(),
                    description: String::new(),
                    sequence_id: 2,
                    batch_size: 7.0,
                },
            ],
        };

        let map = work_order_document_to_map(document).expect("map");

        assert_eq!(map.id, "zakaz-8756");
        assert_eq!(map.product_code, "ITEM-8756");
        assert_eq!(map.title, "vitagum vitamin zip paket");
        assert_eq!(map.code, "8756");
        assert_eq!(map.order_number, "8756");
        assert_eq!(map.roll_count, Some(7.0));
        assert_eq!(map.width_mm, Some(630.0));
        let apparatus = map
            .nodes
            .iter()
            .filter(|node| node.kind == ProductionMapNodeKind::Apparatus)
            .map(|node| node.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            apparatus,
            vec!["8 ta rangli pechat - A", "Laminatsiya 1 - A"]
        );
        assert_eq!(map.edges.len(), 4);
        assert_eq!(map.edges[1].from, "task");
        assert_eq!(map.edges[1].to, "apparatus-1");
    }

    #[test]
    fn work_order_documents_to_maps_skips_non_rs_orders() {
        let maps = work_order_documents_to_maps(vec![
            WorkOrderDocument {
                name: "MFG-WO-1".to_string(),
                production_item: "ITEM-1".to_string(),
                description: "Manual ERP order".to_string(),
                operations: Vec::new(),
            },
            WorkOrderDocument {
                name: "MFG-WO-2".to_string(),
                production_item: "ITEM-2".to_string(),
                description: "Zakaz: /222\nMahsulot: test\nRS map id: zakaz-222".to_string(),
                operations: vec![WorkOrderOperationRow {
                    workstation: "7 ta rangli pechat - A".to_string(),
                    description: "Material razmer: 630 mm\nQolib soni: 7".to_string(),
                    sequence_id: 1,
                    batch_size: 7.0,
                }],
            },
        ]);

        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].id, "zakaz-222");
        assert_eq!(maps[0].product_code, "ITEM-2");
    }

    fn node(id: &str, kind: ProductionMapNodeKind, title: &str) -> ProductionMapNode {
        ProductionMapNode {
            id: id.to_string(),
            kind,
            title: title.to_string(),
            formula: None,
            role_code: String::new(),
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: String::new(),
            alternative_group_label: String::new(),
            alternative_assigned_title: String::new(),
            x: 0.0,
            y: 0.0,
        }
    }

    fn edge(from: &str, to: &str) -> ProductionMapEdge {
        ProductionMapEdge {
            from: from.to_string(),
            to: to.to_string(),
            branch: String::new(),
        }
    }
}
