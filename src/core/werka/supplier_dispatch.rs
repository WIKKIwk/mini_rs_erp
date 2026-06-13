use crate::core::werka::models::DispatchRecord;
use crate::core::werka::ports::{CreatePurchaseReceiptInput, WerkaPortError};
use crate::core::werka::service::{WerkaService, current_timestamp_label};

impl WerkaService {
    pub async fn create_supplier_dispatch(
        &self,
        supplier_ref: &str,
        supplier_display_name: &str,
        supplier_phone: &str,
        item_code: &str,
        qty: f64,
    ) -> Result<Option<DispatchRecord>, WerkaPortError> {
        let Some(writer) = &self.unannounced_writer else {
            return Ok(None);
        };

        self.validate_supplier_dispatch_item(supplier_ref, item_code)
            .await?;
        let warehouse = writer.resolve_warehouse().await?;
        let draft = writer
            .create_draft_purchase_receipt(CreatePurchaseReceiptInput {
                supplier: supplier_ref.trim().to_string(),
                supplier_phone: supplier_phone.trim().to_string(),
                item_code: item_code.trim().to_string(),
                qty,
                warehouse,
                ..CreatePurchaseReceiptInput::default()
            })
            .await?;

        Ok(Some(DispatchRecord {
            id: draft.name,
            supplier_name: supplier_display_name.trim().to_string(),
            item_code: draft.item_code,
            item_name: draft.item_name,
            uom: draft.uom,
            sent_qty: draft.qty,
            accepted_qty: 0.0,
            status: "pending".to_string(),
            created_label: current_timestamp_label(),
            ..DispatchRecord::default()
        }))
    }

    async fn validate_supplier_dispatch_item(
        &self,
        supplier_ref: &str,
        item_code: &str,
    ) -> Result<(), WerkaPortError> {
        let items = self
            .supplier_mobile_items(supplier_ref, "", 200)
            .await?
            .ok_or_else(|| WerkaPortError::WriteFailed("supplier items failed".to_string()))?;
        if items
            .iter()
            .any(|item| item.code.trim().eq_ignore_ascii_case(item_code.trim()))
        {
            Ok(())
        } else {
            Err(WerkaPortError::WriteFailed(
                "item supplierga biriktirilmagan".to_string(),
            ))
        }
    }
}
