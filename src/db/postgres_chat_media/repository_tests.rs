use super::{AUTHORIZED_PRINCIPAL_SQL, prefixed_columns};

#[test]
fn media_authorization_requires_active_conversation_membership_and_can_post_when_requested() {
    let query = AUTHORIZED_PRINCIPAL_SQL.to_ascii_lowercase();

    assert!(query.contains("mini_chat_conversation_members"));
    assert!(query.contains("chat_principal.active = true"));
    assert!(query.contains("member.conversation_id = $3"));
    assert!(query.contains("member.left_at is null"));
    assert!(query.contains("member.can_post = true"));
    assert!(query.contains("chat_principal.principal_role <> 'customer'"));
    assert!(query.contains("customer_principal.principal_role = 'customer'"));
}

#[test]
fn orphan_returning_columns_are_qualified_for_update_query() {
    let columns = prefixed_columns("media").to_ascii_lowercase();

    assert!(columns.contains("media.media_id"));
    assert!(columns.contains("extract(epoch from media.expires_at)"));
    assert!(!columns.contains("\nmedia.extract"));
}
