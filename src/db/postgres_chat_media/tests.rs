use std::sync::Arc;

use bytes::Bytes;

use super::PostgresChatMediaRepository;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::chat_media::{
    ChatMediaByteStream, ChatMediaError, ChatMediaInitializeInput, ChatMediaKind,
    ChatMediaService, ChatMediaStatus,
};
use crate::db::postgres::apply_foundation_migration;
use crate::store::chat_media_local::LocalChatMediaStorage;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops a mini_rs_erp_test_* database"]
async fn postgres_chat_media_enforces_authorization_idempotency_and_completion() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = format!("mini_rs_erp_test_chat_media_{}", std::process::id());
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#))
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
    let service = ChatMediaService::new(
        Arc::new(PostgresChatMediaRepository::new(pool.clone())),
        Arc::new(LocalChatMediaStorage::new(directory.path())),
    );

    let input = ChatMediaInitializeInput {
        client_upload_id: "client_upload_1".to_string(),
        kind: ChatMediaKind::Image,
        filename: "photo.jpg".to_string(),
        content_type: "image/jpeg".to_string(),
        size_bytes: 3,
        duration_ms: None,
    };
    let first = service
        .initialize_upload(&owner(), "conversation_1", input.clone())
        .await
        .expect("initialize");
    let repeated = service
        .initialize_upload(&owner(), "conversation_1", input)
        .await
        .expect("idempotent initialize");
    assert!(first.created);
    assert!(!repeated.created);
    assert_eq!(first.media.media_id, repeated.media.media_id);

    assert_eq!(
        service
            .upload_status(&intruder(), "conversation_1", &first.media.upload_id)
            .await
            .unwrap_err(),
        ChatMediaError::Forbidden
    );
    service
        .upload_content(
            &owner(),
            "conversation_1",
            &first.media.upload_id,
            Some(3),
            Some("image/jpeg"),
            bytes(b"abc"),
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
        service
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

    let completed = service
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

    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#))
        .execute(&admin_pool)
        .await
        .expect("drop test db");
    admin_pool.close().await;
}

async fn seed_conversation(pool: &sqlx::PgPool) {
    for (id, reference) in [("principal_owner", "owner"), ("principal_intruder", "intruder")] {
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

fn bytes(value: &'static [u8]) -> ChatMediaByteStream {
    Box::pin(async_stream::stream! {
        yield Ok(Bytes::from_static(value));
    })
}

fn database_url(admin_url: &str, database: &str) -> String {
    let mut url = reqwest::Url::parse(admin_url).expect("admin database URL");
    url.set_path(&format!("/{database}"));
    url.to_string()
}
