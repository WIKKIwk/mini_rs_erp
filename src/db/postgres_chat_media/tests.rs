use std::sync::Arc;

use bytes::Bytes;

use super::PostgresChatMediaRepository;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::chat::ChatService;
use crate::core::chat_media::{
    ChatMediaAccess, ChatMediaAccessVariant, ChatMediaByteStream, ChatMediaError,
    ChatMediaInitializeInput, ChatMediaKind, ChatMediaService, ChatMediaStatus,
};
use crate::db::postgres::apply_foundation_migration;
use crate::db::postgres_chat::PostgresChatStore;
use crate::store::chat_media_local::LocalChatMediaStorage;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops a mini_rs_erp_test_* database"]
async fn postgres_chat_media_enforces_authorization_idempotency_and_completion() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = format!("mini_rs_erp_test_chat_media_{}", std::process::id());
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop stale test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");
    admin_pool.close().await;

    let test_url = database_url(&admin_url, &db_name);
    let pool = sqlx::PgPool::connect(&test_url).await.expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migrations");
    seed_conversation(&pool).await;
    let directory = tempfile::tempdir().expect("media directory");
    let media_service = ChatMediaService::new(
        Arc::new(PostgresChatMediaRepository::new(pool.clone())),
        Arc::new(LocalChatMediaStorage::new(directory.path())),
    );

    let source = test_image();
    let input = ChatMediaInitializeInput {
        client_upload_id: "client_upload_1".to_string(),
        kind: ChatMediaKind::Image,
        filename: "photo.png".to_string(),
        content_type: "image/png".to_string(),
        size_bytes: source.len() as i64,
        duration_ms: None,
    };
    let first = media_service
        .initialize_upload(&owner(), "conversation_1", input.clone())
        .await
        .expect("initialize");
    let repeated = media_service
        .initialize_upload(&owner(), "conversation_1", input)
        .await
        .expect("idempotent initialize");
    assert!(first.created);
    assert!(!repeated.created);
    assert_eq!(first.media.media_id, repeated.media.media_id);

    assert_eq!(
        media_service
            .upload_status(&intruder(), "conversation_1", &first.media.upload_id)
            .await
            .unwrap_err(),
        ChatMediaError::Forbidden
    );
    media_service
        .upload_content(
            &owner(),
            "conversation_1",
            &first.media.upload_id,
            Some(source.len() as i64),
            Some("image/png"),
            bytes(source),
        )
        .await
        .expect("store upload");

    sqlx::query(
        "UPDATE mini_chat_conversation_members SET can_post = FALSE WHERE conversation_id = 'conversation_1'",
    )
    .execute(&pool)
    .await
    .expect("disable posting");
    assert_eq!(
        media_service
            .complete_upload(&owner(), "conversation_1", &first.media.upload_id)
            .await
            .unwrap_err(),
        ChatMediaError::Forbidden
    );
    sqlx::query(
        "UPDATE mini_chat_conversation_members SET can_post = TRUE WHERE conversation_id = 'conversation_1'",
    )
    .execute(&pool)
    .await
    .expect("enable posting");

    let completed = media_service
        .complete_upload(&owner(), "conversation_1", &first.media.upload_id)
        .await
        .expect("complete");
    assert_eq!(completed.status, ChatMediaStatus::Processing);
    let job_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mini_chat_media_jobs WHERE media_id = $1 AND job_status = 'pending'",
    )
    .bind(&completed.media_id)
    .fetch_one(&pool)
    .await
    .expect("job count");
    assert_eq!(job_count, 1);

    assert_eq!(
        media_service
            .process_pending_jobs(1)
            .await
            .expect("process media"),
        1
    );
    let ready = media_service
        .upload_status(&owner(), "conversation_1", &first.media.upload_id)
        .await
        .expect("ready status");
    assert_eq!(ready.status, ChatMediaStatus::Ready);
    assert_eq!(ready.width_pixels, Some(32));
    assert_eq!(ready.height_pixels, Some(18));

    let chat_service = ChatService::new(Arc::new(PostgresChatStore::new(pool.clone())));
    let sent = chat_service
        .send_media_message(
            &owner(),
            "conversation_1",
            "client_message_media_1",
            "",
            &ready.media_id,
        )
        .await
        .expect("send media message");
    assert_eq!(sent.message.message_type, "image");
    let attachment = sent.message.attachment.expect("attachment payload");
    assert_eq!(attachment.media_id, ready.media_id);
    assert_eq!(attachment.content_type, "image/jpeg");
    assert!(attachment.content_url.ends_with("/content"));
    assert!(attachment.thumbnail_url.ends_with("/thumbnail"));

    let ChatMediaAccess::Local { content } = media_service
        .media_access(&owner(), &ready.media_id, ChatMediaAccessVariant::Content)
        .await
        .expect("authorized private access")
    else {
        panic!("local development storage must be proxied");
    };
    assert!(!content.bytes.is_empty());

    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
    admin_pool.close().await;
}

async fn seed_conversation(pool: &sqlx::PgPool) {
    for (id, reference) in [
        ("principal_owner", "owner"),
        ("principal_intruder", "intruder"),
    ] {
        sqlx::query(
            r#"INSERT INTO mini_chat_principals
                 (principal_id, principal_role, principal_ref, display_name)
               VALUES ($1, 'customer', $2, $2)"#,
        )
        .bind(id)
        .bind(reference)
        .execute(pool)
        .await
        .expect("principal");
    }
    sqlx::query(
        r#"INSERT INTO mini_chat_conversations
             (conversation_id, kind, dm_key, created_by_principal_id)
           VALUES ('conversation_1', 'dm', 'owner:peer', 'principal_owner')"#,
    )
    .execute(pool)
    .await
    .expect("conversation");
    sqlx::query(
        r#"INSERT INTO mini_chat_conversation_members
             (conversation_id, principal_id, member_role, can_post)
           VALUES ('conversation_1', 'principal_owner', 'owner', TRUE)"#,
    )
    .execute(pool)
    .await
    .expect("member");
}

fn owner() -> Principal {
    principal("owner")
}

fn intruder() -> Principal {
    principal("intruder")
}

fn principal(reference: &str) -> Principal {
    Principal {
        role: PrincipalRole::Customer,
        display_name: reference.to_string(),
        legal_name: reference.to_string(),
        ref_: reference.to_string(),
        phone: String::new(),
        avatar_url: String::new(),
    }
}

fn bytes(value: Vec<u8>) -> ChatMediaByteStream {
    Box::pin(async_stream::stream! {
        yield Ok(Bytes::from(value));
    })
}

fn test_image() -> Vec<u8> {
    let image = image::DynamicImage::new_rgb8(32, 18);
    let mut output = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut output, image::ImageFormat::Png)
        .expect("encode test image");
    output.into_inner()
}

fn database_url(admin_url: &str, database: &str) -> String {
    let mut url = reqwest::Url::parse(admin_url).expect("admin database URL");
    url.set_path(&format!("/{database}"));
    url.to_string()
}
