use super::models::WerkaCustomerIssueSource;
use super::service::{customer_issue_source_marker, normalize_customer_issue_source};

#[test]
fn customer_issue_source_marker_matches_go_order_and_trimming() {
    let marker = customer_issue_source_marker(&WerkaCustomerIssueSource {
        barcode: " 30AD ".to_string(),
        stock_entry_name: " MAT-STE-001 ".to_string(),
        line_index: Some(1),
    });

    assert_eq!(
        marker,
        "accord_customer_issue_source:source_barcode=30AD;source_stock_entry=MAT-STE-001;source_line_index=1"
    );
}

#[test]
fn customer_issue_source_ignores_negative_line_index_like_go() {
    let source = normalize_customer_issue_source(WerkaCustomerIssueSource {
        barcode: String::new(),
        stock_entry_name: " MAT-STE-001 ".to_string(),
        line_index: Some(-1),
    });

    assert_eq!(source.line_index, None);
    assert_eq!(
        customer_issue_source_marker(&source),
        "accord_customer_issue_source:source_stock_entry=MAT-STE-001"
    );
}
