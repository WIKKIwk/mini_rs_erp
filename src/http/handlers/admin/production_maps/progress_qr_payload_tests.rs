use super::effective_progress_qr_payload;

#[test]
fn canonical_qr_payload_precedes_the_legacy_alias() {
    assert_eq!(effective_progress_qr_payload("qr:new", "qr:old"), "qr:new");
    assert_eq!(effective_progress_qr_payload("", "qr:old"), "qr:old");
    assert_eq!(effective_progress_qr_payload("  ", "qr:old"), "qr:old");
}
