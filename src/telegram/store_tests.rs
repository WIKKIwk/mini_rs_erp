use super::{load_or_create_key_file, parse_key};

#[test]
fn session_key_file_is_auto_created_and_reused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mobile_telegram.key");
    assert!(!path.exists());
    let first = load_or_create_key_file(&path).expect("create");
    assert!(path.exists());
    let second = load_or_create_key_file(&path).expect("reuse");
    assert_eq!(first, second);
    assert!(parse_key("  xyz  ").is_none());
}
