use super::super::*;

#[derive(Clone)]
pub(crate) struct RawMaterialStockLookup {
    stock: Arc<Mutex<BTreeMap<String, RawMaterialStockEntry>>>,
}

impl Default for RawMaterialStockLookup {
    fn default() -> Self {
        Self {
            stock: Arc::new(Mutex::new(BTreeMap::from([
                (
                    "30AA".to_string(),
                    raw_material_stock_entry("30AA", "INK-BLACK", "Black ink", 12.0),
                ),
                (
                    "30CC".to_string(),
                    raw_material_stock_entry("30CC", "INK-WHITE", "White ink", 8.0),
                ),
            ]))),
        }
    }
}

impl RawMaterialStockLookup {
    pub(crate) async fn insert_stock(
        &self,
        barcode: &str,
        item_code: &str,
        item_name: &str,
        qty: f64,
    ) {
        self.stock.lock().await.insert(
            barcode.trim().to_ascii_uppercase(),
            raw_material_stock_entry(barcode, item_code, item_name, qty),
        );
    }

    pub(crate) async fn set_stock_status(&self, barcode: &str, status: &str, order_id: &str) {
        let mut stock = self.stock.lock().await;
        let item = stock
            .get_mut(&barcode.trim().to_ascii_uppercase())
            .expect("stock item");
        item.status = status.trim().to_string();
        item.reserved_order_id = order_id.trim().to_string();
    }
}

fn raw_material_stock_entry(
    barcode: &str,
    item_code: &str,
    item_name: &str,
    qty: f64,
) -> RawMaterialStockEntry {
    RawMaterialStockEntry {
        id: format!("raw:{barcode}"),
        item_code: item_code.to_string(),
        item_name: item_name.to_string(),
        warehouse: "Kalidor".to_string(),
        qty,
        uom: "Kg".to_string(),
        barcode: barcode.to_string(),
        status: "available".to_string(),
        reserved_order_id: String::new(),
        source_receipt_id: format!("GSR-{barcode}"),
    }
}

#[async_trait]
impl MaterialReceiptStorePort for RawMaterialStockLookup {
    async fn create_material_receipt_draft(
        &self,
        _input: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError> {
        Err(GscalePortError::StoreWrite("not used".to_string()))
    }

    async fn submit_stock_entry_draft(&self, _name: &str) -> Result<(), GscalePortError> {
        Ok(())
    }

    async fn delete_stock_entry_draft(&self, _name: &str) -> Result<(), GscalePortError> {
        Ok(())
    }

    async fn raw_material_stock_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<RawMaterialStockEntry>, GscalePortError> {
        Ok(self
            .stock
            .lock()
            .await
            .get(&barcode.trim().to_ascii_uppercase())
            .cloned())
    }

    async fn raw_material_stock(
        &self,
        warehouse: &str,
        _limit: usize,
    ) -> Result<Vec<RawMaterialStockEntry>, GscalePortError> {
        assert_eq!(warehouse, "Kalidor");
        Ok(self.stock.lock().await.values().cloned().collect())
    }

    async fn update_raw_material_stock(
        &self,
        input: RawMaterialStockUpdateInput,
    ) -> Result<RawMaterialStockEntry, GscalePortError> {
        let mut stock = self.stock.lock().await;
        let item = stock
            .get_mut(&input.barcode.trim().to_ascii_uppercase())
            .ok_or_else(|| {
                GscalePortError::InvalidInput("raw_material_stock_not_found".to_string())
            })?;
        if item.status != "available" || !item.reserved_order_id.trim().is_empty() {
            return Err(GscalePortError::InvalidInput(
                "raw_material_stock_locked".to_string(),
            ));
        }
        item.item_code = input.item_code.trim().to_string();
        item.item_name = input.item_name.trim().to_string();
        item.qty = input.qty;
        Ok(item.clone())
    }

    async fn mark_raw_material_stock_in_use(
        &self,
        barcodes: &[String],
        order_id: &str,
    ) -> Result<Vec<RawMaterialStockEntry>, GscalePortError> {
        let mut stock = self.stock.lock().await;
        let mut updated = Vec::new();
        for barcode in barcodes {
            let key = barcode.trim().to_ascii_uppercase();
            let Some(item) = stock.get_mut(&key) else {
                return Err(GscalePortError::StoreWrite(format!("missing stock {key}")));
            };
            if item.status != "available"
                && !(item.status == "in_use" && item.reserved_order_id == order_id.trim())
            {
                return Err(GscalePortError::InvalidInput(
                    "raw_material_stock_unavailable".to_string(),
                ));
            }
            item.status = "in_use".to_string();
            item.reserved_order_id = order_id.trim().to_string();
            updated.push(item.clone());
        }
        Ok(updated)
    }

    async fn mark_raw_material_stock_consumed(
        &self,
        barcodes: &[String],
        order_id: &str,
    ) -> Result<Vec<RawMaterialStockEntry>, GscalePortError> {
        let mut stock = self.stock.lock().await;
        let mut updated = Vec::new();
        for barcode in barcodes {
            let key = barcode.trim().to_ascii_uppercase();
            let Some(item) = stock.get_mut(&key) else {
                return Err(GscalePortError::StoreWrite(format!("missing stock {key}")));
            };
            if item.reserved_order_id != order_id.trim()
                || (item.status != "in_use" && item.status != "consumed")
            {
                return Err(GscalePortError::InvalidInput(
                    "raw_material_stock_unavailable".to_string(),
                ));
            }
            item.status = "consumed".to_string();
            updated.push(item.clone());
        }
        Ok(updated)
    }
}

pub(crate) struct FakeProgressDriver {
    pub(crate) requests: Arc<Mutex<Vec<ScaleDriverPrintRequest>>>,
    pub(crate) fail: bool,
}

#[async_trait]
impl ScaleDriverPort for FakeProgressDriver {
    async fn print_material_receipt(
        &self,
        request: ScaleDriverPrintRequest,
    ) -> Result<ScaleDriverPrintResponse, GscalePortError> {
        self.requests.lock().await.push(request.clone());
        if self.fail {
            return Err(GscalePortError::Driver("printer offline".to_string()));
        }
        Ok(ScaleDriverPrintResponse {
            ok: true,
            status: "done".to_string(),
            epc: request.epc,
            printer: request.printer,
            mode: request.print_mode,
            qty: request.gross_qty,
            gross_qty: request.gross_qty,
            unit: request.unit,
            printer_status: "OK".to_string(),
            ..ScaleDriverPrintResponse::default()
        })
    }
}
