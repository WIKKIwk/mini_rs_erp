use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::any;

use crate::app::AppState;
use crate::core::chat_media::{MAX_CHAT_IMAGE_SIZE_BYTES, MAX_CHAT_MEDIA_CHUNK_SIZE_BYTES};
use crate::http::handlers::{
    auth, calculate, chat, customer, gscale, iroh_discovery, notifications, profile, push, qolip,
    returned_paint, rezka, rps_batch, stock_entry, supplier, werka,
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/mobile/auth/login", any(auth::login))
        .route("/v1/mobile/auth/logout", any(auth::logout))
        .route("/v1/mobile/me", any(auth::me))
        .route(
            "/v1/mobile/returned-paint/requests",
            any(returned_paint::requests),
        )
        .route(
            "/v1/mobile/returned-paint/requests/complete",
            any(returned_paint::complete_request),
        )
        .route(
            "/v1/mobile/returned-paint/images",
            any(returned_paint::images).layer(DefaultBodyLimit::max(6 * 1024 * 1024)),
        )
        .route(
            "/v1/mobile/returned-paint/images/view",
            any(returned_paint::image_view),
        )
        .route("/v1/mobile/iroh-ticket", any(iroh_discovery::ticket))
        .route("/v1/mobile/calculate", any(calculate::calculate_route))
        .route(
            "/v1/mobile/calculate/orders",
            any(calculate::calculate_orders_route),
        )
        .route(
            "/v1/mobile/calculate/orders/delete",
            any(calculate::calculate_order_delete_route),
        )
        .route(
            "/v1/mobile/calculate/orders/image",
            any(calculate::calculate_order_image_upload_route),
        )
        .route(
            "/v1/mobile/calculate/orders/image/view",
            any(calculate::calculate_order_image_view_route),
        )
        .route("/v1/mobile/profile", any(profile::profile))
        .route("/v1/mobile/profile/avatar", any(profile::avatar_upload))
        .route("/v1/mobile/profile/avatar/view", any(profile::avatar_view))
        .route("/v1/mobile/push/token", any(push::token))
        .route("/v1/mobile/chat/directory", any(chat::directory))
        .route("/v1/mobile/chat/conversations", any(chat::conversations))
        .route("/v1/mobile/chat/conversations/dm", any(chat::create_dm))
        .route(
            "/v1/mobile/chat/conversations/{conversation_id}/messages",
            any(chat::conversation_messages),
        )
        .route(
            "/v1/mobile/chat/conversations/{conversation_id}/media/uploads",
            any(chat::media_uploads),
        )
        .route(
            "/v1/mobile/chat/conversations/{conversation_id}/media/uploads/{upload_id}",
            any(chat::media_upload),
        )
        .route(
            "/v1/mobile/chat/conversations/{conversation_id}/media/uploads/{upload_id}/content",
            any(chat::media_upload_content)
                .layer(DefaultBodyLimit::max(MAX_CHAT_IMAGE_SIZE_BYTES as usize)),
        )
        .route(
            "/v1/mobile/chat/conversations/{conversation_id}/media/uploads/{upload_id}/chunks/{chunk_index}",
            any(chat::media_upload_chunk)
                .layer(DefaultBodyLimit::max(MAX_CHAT_MEDIA_CHUNK_SIZE_BYTES as usize)),
        )
        .route(
            "/v1/mobile/chat/conversations/{conversation_id}/media/uploads/{upload_id}/complete",
            any(chat::media_upload_complete),
        )
        .route(
            "/v1/mobile/chat/media/{media_id}/{variant}",
            any(chat::media_access),
        )
        .route(
            "/v1/mobile/chat/conversations/{conversation_id}/read",
            any(chat::mark_read),
        )
        .route("/v1/mobile/chat/socket-ticket", any(chat::socket_ticket))
        .route("/v1/mobile/chat/device-token", any(chat::device_token))
        .route("/v1/mobile/chat/live", any(chat::live))
        .route("/v1/mobile/gscale/items", any(gscale::items))
        .route(
            "/v1/mobile/gscale/material-receipt/print",
            any(gscale::material_receipt_print),
        )
        .route("/v1/mobile/rps/batch/start", any(rps_batch::start))
        .route("/v1/mobile/rps/batch/state", any(rps_batch::state))
        .route("/v1/mobile/rps/batch/stop", any(rps_batch::stop))
        .route("/v1/mobile/rps/batch/print", any(rps_batch::print))
        .route(
            "/v1/mobile/rps/batch/client-print/prepare",
            any(rps_batch::client_print_prepare),
        )
        .route(
            "/v1/mobile/rps/batch/client-print/confirm",
            any(rps_batch::client_print_confirm),
        )
        .route("/v1/mobile/qolip/blocks", any(qolip::blocks))
        .route("/v1/mobile/qolip/products", any(qolip::products))
        .route("/v1/mobile/qolip/product-specs", any(qolip::product_specs))
        .route("/v1/mobile/qolip/locations", any(qolip::locations))
        .route("/v1/mobile/qolip/locations/move", any(qolip::location_move))
        .route("/v1/mobile/qolip/scan", any(qolip::scan))
        .route("/v1/mobile/qolip/cell-qr", any(qolip::cell_qr))
        .route("/v1/mobile/qolip/cell-qr/print", any(qolip::cell_qr_print))
        .route("/v1/mobile/qolip/code-qr/print", any(qolip::code_qr_print))
        .route("/v1/mobile/qolip/workers", any(qolip::workers))
        .route("/v1/mobile/qolip/checkouts", any(qolip::checkouts))
        .route(
            "/v1/mobile/qolip/checkouts/return",
            any(qolip::checkout_return),
        )
        .route("/v1/mobile/rezka/source", any(rezka::source))
        .route("/v1/mobile/rezka/split", any(rezka::split))
        .route(
            "/v1/mobile/rezka/split/client-print/prepare",
            any(rezka::split_client_prepare),
        )
        .route(
            "/v1/mobile/rezka/split/client-print/confirm",
            any(rezka::split_client_confirm),
        )
        .route("/v1/mobile/stock-entry/lookup", any(stock_entry::lookup))
        .route("/v1/mobile/customer/summary", any(customer::summary))
        .route("/v1/mobile/customer/history", any(customer::history))
        .route(
            "/v1/mobile/customer/status-details",
            any(customer::status_details),
        )
        .route("/v1/mobile/customer/detail", any(customer::detail))
        .route("/v1/mobile/customer/respond", any(customer::respond))
        .route(
            "/v1/mobile/notifications/detail",
            any(notifications::detail),
        )
        .route(
            "/v1/mobile/notifications/comments",
            any(notifications::comment),
        )
        .route(
            "/v1/mobile/supplier/dispatch",
            any(supplier::create_dispatch),
        )
        .route("/v1/mobile/supplier/history", any(supplier::history))
        .route("/v1/mobile/supplier/items", any(supplier::items))
        .route(
            "/v1/mobile/supplier/status-breakdown",
            any(supplier::status_breakdown),
        )
        .route(
            "/v1/mobile/supplier/status-details",
            any(supplier::status_details),
        )
        .route("/v1/mobile/supplier/summary", any(supplier::summary))
        .route(
            "/v1/mobile/supplier/unannounced/respond",
            any(supplier::unannounced_respond),
        )
        .route("/v1/mobile/werka/archive", any(werka::archive))
        .route("/v1/mobile/werka/archive/pdf", any(werka::archive_pdf))
        .route(
            "/v1/mobile/werka/ai-search-suggestion",
            any(werka::ai_search_suggestion),
        )
        .route("/v1/mobile/werka/confirm", any(werka::confirm))
        .route(
            "/v1/mobile/werka/customer-issue/create",
            any(werka::customer_issue_create),
        )
        .route(
            "/v1/mobile/werka/customer-issue/batch-create",
            any(werka::customer_issue_batch_create),
        )
        .route(
            "/v1/mobile/werka/unannounced/create",
            any(werka::unannounced_create),
        )
        .route("/v1/mobile/werka/history", any(werka::history))
        .route("/v1/mobile/werka/notifications", any(werka::history))
        .route("/v1/mobile/werka/pending", any(werka::pending))
        .route(
            "/v1/mobile/werka/status-breakdown",
            any(werka::status_breakdown),
        )
        .route(
            "/v1/mobile/werka/status-details",
            any(werka::status_details),
        )
        .route("/v1/mobile/werka/summary", any(werka::summary))
        .route(
            "/v1/mobile/werka/customer-item-options",
            any(werka::customer_item_options),
        )
        .route(
            "/v1/mobile/werka/customer-items",
            any(werka::customer_items),
        )
        .route(
            "/v1/mobile/werka/supplier-items",
            any(werka::supplier_items),
        )
        .route("/v1/mobile/werka/customers", any(werka::customers))
        .route("/v1/mobile/werka/suppliers", any(werka::suppliers))
        .route("/v1/mobile/werka/home", any(werka::home))
}
