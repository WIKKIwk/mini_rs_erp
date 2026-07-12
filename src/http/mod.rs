mod archive_pdf;
pub mod handlers;
pub mod router;

#[cfg(test)]
mod admin_route_tests;
#[cfg(test)]
mod calculate_route_tests;
#[cfg(test)]
mod chat_route_tests;
#[cfg(test)]
mod customer_route_tests;
#[cfg(test)]
mod gscale_route_tests;
#[cfg(test)]
mod notification_comment_route_tests;
#[cfg(test)]
mod notification_detail_route_tests;
#[cfg(test)]
mod profile_route_tests;
#[cfg(test)]
mod push_notify_tests;
#[cfg(test)]
mod push_route_tests;
#[cfg(test)]
mod rezka_route_tests;
#[cfg(test)]
mod router_tests;
#[cfg(test)]
mod stock_entry_route_tests;
#[cfg(test)]
mod supplier_dispatch_route_tests;
#[cfg(test)]
mod supplier_items_route_tests;
#[cfg(test)]
mod supplier_read_route_tests;
#[cfg(test)]
mod supplier_unannounced_route_tests;
#[cfg(test)]
mod werka_ai_search_route_tests;
#[cfg(test)]
mod werka_archive_route_tests;
#[cfg(test)]
mod werka_confirm_route_tests;
#[cfg(test)]
mod werka_customer_issue_route_tests;
#[cfg(test)]
mod werka_directory_route_tests;
#[cfg(test)]
mod werka_items_route_tests;
#[cfg(test)]
mod werka_route_tests;
#[cfg(test)]
mod werka_unannounced_route_tests;
