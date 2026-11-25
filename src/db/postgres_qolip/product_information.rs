use std::collections::HashMap;

use sqlx::PgPool;

use crate::core::auth::models::Principal;
use crate::core::qolip::product_information::{
    AttachmentEdit, ProductAttachment, ProductInformation, ProductInformationEdit,
    ProductInformationStatus,
};
use crate::core::qolip::{QolipError, role_code};

use super::reworks::storage_error;

type InformationRow = (i64, String, Option<String>, Option<i32>);

fn information(
    (revision, details, attachment_name, attachment_size): InformationRow,
) -> ProductInformation {
    ProductInformation {
        revision,
        details,
        attachment_name,
        attachment_size,
    }
}

pub(super) async fn statuses(
    pool: &PgPool,
    codes: &[String],
) -> Result<HashMap<String, ProductInformationStatus>, QolipError> {
    if codes.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query_as::<_, (String, i64, bool)>(
        "SELECT item_code_key, revision, (details <> '' OR attachment_name IS NOT NULL)
         FROM mini_qolip_product_information
         WHERE item_code_key IN (SELECT lower(btrim(code)) FROM unnest($1::text[]) AS codes(code))",
    )
    .bind(codes)
    .fetch_all(pool)
    .await
    .map_err(storage_error)?;
    Ok(rows
        .into_iter()
        .map(|(code, revision, has_content)| {
            (
                code,
                ProductInformationStatus {
                    revision,
                    has_content,
                },
            )
        })
        .collect())
}

pub(super) async fn read(pool: &PgPool, code: &str) -> Result<ProductInformation, QolipError> {
    let row = sqlx::query_as::<_, InformationRow>(
        "SELECT revision, details, attachment_name, octet_length(attachment_content) AS attachment_size
         FROM mini_qolip_product_information WHERE item_code_key = lower(btrim($1))",
    )
    .bind(code)
    .fetch_optional(pool)
    .await
    .map_err(storage_error)?;
    Ok(row.map(information).unwrap_or_default())
}

pub(super) async fn save(
    pool: &PgPool,
    code: &str,
    edit: ProductInformationEdit,
    principal: &Principal,
) -> Result<Option<ProductInformation>, QolipError> {
    let (update_attachment, filename, content) = match &edit.attachment {
        AttachmentEdit::Keep => (false, None, None),
        AttachmentEdit::Remove => (true, None, None),
        AttachmentEdit::Replace(file) => {
            (true, Some(file.name.as_str()), Some(file.content.as_ref()))
        }
    };
    let role = role_code(&principal.role);
    if edit.revision == 0 {
        return sqlx::query_as::<_, InformationRow>(
            "INSERT INTO mini_qolip_product_information
                (item_code_key, details, attachment_name, attachment_content, revision, updated_by_role, updated_by_ref)
             VALUES (lower(btrim($1)), $2, $3, $4, 1, $5, $6)
             ON CONFLICT (item_code_key) DO NOTHING
             RETURNING revision, details, attachment_name, octet_length(attachment_content) AS attachment_size",
        )
        .bind(code)
        .bind(&edit.details)
        .bind(filename)
        .bind(content)
        .bind(role)
        .bind(&principal.ref_)
        .fetch_optional(pool)
        .await
        .map(|row| row.map(information))
        .map_err(storage_error);
    }
    // Text and the attachment change together, guarded by the version the editor read.
    sqlx::query_as::<_, InformationRow>(
        "UPDATE mini_qolip_product_information
         SET details = $3,
             attachment_name = CASE WHEN $4 THEN $5 ELSE attachment_name END,
             attachment_content = CASE WHEN $4 THEN $6 ELSE attachment_content END,
             updated_by_role = $7, updated_by_ref = $8, updated_at = now(), revision = revision + 1
         WHERE item_code_key = lower(btrim($1)) AND revision = $2
         RETURNING revision, details, attachment_name, octet_length(attachment_content) AS attachment_size",
    )
    .bind(code)
    .bind(edit.revision)
    .bind(&edit.details)
    .bind(update_attachment)
    .bind(filename)
    .bind(content)
    .bind(role)
    .bind(&principal.ref_)
    .fetch_optional(pool)
    .await
    .map(|row| row.map(information))
    .map_err(storage_error)
}

pub(super) async fn attachment(
    pool: &PgPool,
    code: &str,
    revision: i64,
) -> Result<Option<ProductAttachment>, QolipError> {
    let row = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT attachment_name, attachment_content
         FROM mini_qolip_product_information
         WHERE item_code_key = lower(btrim($1)) AND revision = $2 AND attachment_name IS NOT NULL",
    )
    .bind(code)
    .bind(revision)
    .fetch_optional(pool)
    .await
    .map_err(storage_error)?;
    Ok(row.map(|(name, content)| ProductAttachment {
        name,
        content: content.into(),
    }))
}
