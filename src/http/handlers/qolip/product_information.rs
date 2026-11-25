use axum::body::Body;
use axum::extract::Multipart;
use axum::extract::multipart::MultipartError;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use crate::core::auth::models::Principal;
use crate::core::qolip::product_information::{
    AttachmentEdit, MAX_ATTACHMENT_BYTES, MAX_DETAILS_CHARS, ProductAttachment, ProductInformation,
    ProductInformationEdit,
};

use super::catalog::catalog_product_code;
use super::*;

pub const MAX_PRODUCT_INFORMATION_BODY: usize = MAX_ATTACHMENT_BYTES + 64 * 1024;

type InformationError = (StatusCode, Json<QolipErrorResponse>);

#[derive(Deserialize)]
pub struct InformationQuery {
    id: String,
}

#[derive(Deserialize)]
pub struct AttachmentQuery {
    id: String,
    revision: i64,
}

#[derive(Deserialize)]
struct DetailsEdit {
    revision: i64,
    details: String,
    #[serde(default)]
    remove_attachment: bool,
}

async fn authorize_product(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
) -> Result<(Principal, String), InformationError> {
    let principal = authenticated_principal(state, headers).await?;
    ensure_qolip_access(state, &principal).await?;
    let code = catalog_product_code(id).ok_or_else(|| bad_request("invalid_product_id"))?;
    let is_admin = state
        .admin
        .principal_has_admin_workspace_access(&principal)
        .await;
    let products = state
        .qolip
        .catalog_detail(&principal, is_admin, &code)
        .await
        .map_err(qolip_error)?;
    let product = products
        .first()
        .ok_or_else(|| qolip_error(QolipError::ProductNotFound))?;
    Ok((principal, product.code.clone()))
}

pub async fn product_information(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<InformationQuery>,
) -> Result<Response, InformationError> {
    let (_, code) = authorize_product(&state, &headers, &query.id).await?;
    let information = state
        .qolip
        .product_information(&code)
        .await
        .map_err(qolip_error)?;
    Ok(information_response(information))
}

pub async fn save_product_information(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<InformationQuery>,
    multipart: Multipart,
) -> Result<Response, InformationError> {
    let (principal, code) = authorize_product(&state, &headers, &query.id).await?;
    let edit = parse_edit(multipart).await?;
    let information = state
        .qolip
        .save_product_information(&code, edit, &principal)
        .await
        .map_err(qolip_error)?
        .ok_or_else(information_changed)?;
    Ok(information_response(information))
}

pub async fn product_attachment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AttachmentQuery>,
) -> Result<Response, InformationError> {
    let (_, code) = authorize_product(&state, &headers, &query.id).await?;
    let file = state
        .qolip
        .product_attachment(&code, query.revision)
        .await
        .map_err(qolip_error)?
        .ok_or_else(information_changed)?;
    let disposition = format!(
        "attachment; filename*=UTF-8''{}",
        urlencoding::encode(&file.name)
    );
    Response::builder()
        .header(header::CACHE_CONTROL, "private, no-store")
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from(file.content))
        .map_err(|_| qolip_error(QolipError::StoreFailed))
}

fn information_response(information: ProductInformation) -> Response {
    (
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(json!({"ok": true, "information": information})),
    )
        .into_response()
}

async fn parse_edit(mut multipart: Multipart) -> Result<ProductInformationEdit, InformationError> {
    let mut edit = None;
    let mut attachment = None;
    while let Some(field) = multipart.next_field().await.map_err(upload_error)? {
        match field.name() {
            Some("edit") if edit.is_none() => {
                edit = Some(
                    serde_json::from_slice::<DetailsEdit>(
                        &field.bytes().await.map_err(upload_error)?,
                    )
                    .map_err(|_| bad_request("invalid_qolip_information"))?,
                );
            }
            Some("attachment") if attachment.is_none() => {
                let name = field
                    .file_name()
                    .map(str::trim)
                    .filter(|name| {
                        !name.is_empty()
                            && name.len() <= 255
                            && !matches!(*name, "." | "..")
                            && !name.contains(['/', '\\'])
                            && !name.chars().any(char::is_control)
                    })
                    .ok_or_else(|| bad_request("invalid_qolip_attachment"))?
                    .to_owned();
                let content = field.bytes().await.map_err(upload_error)?;
                if content.len() > MAX_ATTACHMENT_BYTES {
                    return Err(attachment_too_large());
                }
                if content.is_empty() {
                    return Err(bad_request("invalid_qolip_attachment"));
                }
                attachment = Some(ProductAttachment { name, content });
            }
            _ => return Err(bad_request("invalid_qolip_information")),
        }
    }
    let edit = edit.ok_or_else(|| bad_request("invalid_qolip_information"))?;
    let details = edit.details.trim();
    if edit.revision < 0
        || details.len() > MAX_DETAILS_CHARS * 4
        || details.chars().count() > MAX_DETAILS_CHARS
        || details.contains('\0')
    {
        return Err(bad_request("invalid_qolip_information"));
    }
    let attachment = match (attachment, edit.remove_attachment) {
        (Some(file), false) => AttachmentEdit::Replace(file),
        (None, false) => AttachmentEdit::Keep,
        (None, true) => AttachmentEdit::Remove,
        (Some(_), true) => return Err(bad_request("invalid_qolip_information")),
    };
    Ok(ProductInformationEdit {
        revision: edit.revision,
        details: details.to_owned(),
        attachment,
    })
}

fn upload_error(error: MultipartError) -> InformationError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        attachment_too_large()
    } else {
        bad_request("invalid_qolip_attachment")
    }
}

fn attachment_too_large() -> InformationError {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(QolipErrorResponse::new("qolip_attachment_too_large")),
    )
}

fn information_changed() -> InformationError {
    conflict("qolip_information_changed")
}
