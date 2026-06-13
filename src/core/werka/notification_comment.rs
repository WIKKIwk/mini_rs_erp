use crate::core::auth::models::PrincipalRole;
use crate::core::werka::models::NotificationDetail;
use crate::core::werka::notification::{
    NotificationTargetType, resolve_notification_target, with_supplier_display_name,
};
use crate::core::werka::ports::{NotificationDetailWriter, WerkaPortError};
use crate::core::werka::service::WerkaService;
use crate::core::werka::unannounced::format_notification_comment;

const SUPPLIER_ACK_REMARK_PREFIX: &str = "Accord Supplier Tasdiq:";

impl WerkaService {
    pub async fn add_notification_comment(
        &self,
        role: PrincipalRole,
        principal_ref: &str,
        principal_display_name: &str,
        receipt_id: &str,
        message: &str,
    ) -> Result<Option<NotificationDetail>, WerkaPortError> {
        let trimmed_message = message.trim();
        if trimmed_message.is_empty() {
            return Err(WerkaPortError::WriteFailed(
                "comment is required".to_string(),
            ));
        }
        let target = resolve_notification_target(receipt_id)?;
        let Some(writer) = &self.notification_detail_writer else {
            return Ok(None);
        };

        self.notification_detail(
            role.clone(),
            principal_ref,
            principal_display_name,
            receipt_id,
        )
        .await?
        .ok_or_else(|| WerkaPortError::WriteFailed("notification detail failed".to_string()))?;

        let formatted =
            format_notification_comment(role_label(&role), principal_display_name, trimmed_message);
        match target.target_type {
            NotificationTargetType::DeliveryNote => {
                writer
                    .add_notification_delivery_note_comment(&target.name, &formatted)
                    .await?;
            }
            NotificationTargetType::PurchaseReceipt => {
                writer
                    .add_notification_purchase_receipt_comment(&target.name, &formatted)
                    .await?;
            }
        }
        if role == PrincipalRole::Supplier && is_supplier_acknowledgment_message(trimmed_message) {
            update_supplier_acknowledgment_remarks(writer.as_ref(), &target.name, trimmed_message)
                .await?;
        }

        let detail = self
            .notification_detail(
                role.clone(),
                principal_ref,
                principal_display_name,
                receipt_id,
            )
            .await?
            .ok_or_else(|| WerkaPortError::WriteFailed("notification detail failed".to_string()))?;
        Ok(Some(with_supplier_display_name(
            detail,
            &role,
            principal_display_name,
        )))
    }
}

async fn update_supplier_acknowledgment_remarks(
    writer: &dyn NotificationDetailWriter,
    target_name: &str,
    message: &str,
) -> Result<(), WerkaPortError> {
    let draft = writer
        .get_notification_purchase_receipt(target_name)
        .await?;
    let remarks = upsert_supplier_acknowledgment_in_remarks(&draft.remarks, message);
    let _ = writer
        .update_notification_purchase_receipt_remarks(target_name, &remarks)
        .await;
    Ok(())
}

fn is_supplier_acknowledgment_message(message: &str) -> bool {
    message.trim().to_lowercase().starts_with("tasdiqlayman")
}

fn role_label(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "Supplier",
        PrincipalRole::Werka => "Werka",
        PrincipalRole::Customer => "Customer",
        PrincipalRole::Aparatchi => "Aparatchi",
        PrincipalRole::Admin => "Admin",
    }
}

fn upsert_supplier_acknowledgment_in_remarks(existing_note: &str, message: &str) -> String {
    let mut lines = existing_note
        .replace("\r\n", "\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("Supplier tasdiqladi:"))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    lines.push(format!("{} {}", SUPPLIER_ACK_REMARK_PREFIX, message.trim()));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::upsert_supplier_acknowledgment_in_remarks;

    #[test]
    fn upsert_supplier_acknowledgment_replaces_display_line_like_go() {
        let result = upsert_supplier_acknowledgment_in_remarks(
            "Old note\r\nSupplier tasdiqladi: old\n",
            "tasdiqlayman, oldim",
        );

        assert_eq!(
            result,
            "Old note\nAccord Supplier Tasdiq: tasdiqlayman, oldim"
        );
    }
}
