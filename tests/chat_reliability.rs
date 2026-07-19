use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use mini_rs_erp::core::auth::models::{Principal, PrincipalRole};
use mini_rs_erp::core::chat::{ChatPrincipalInput, ChatService, ChatStorePort};
use mini_rs_erp::core::chat_media::{
    ChatMediaAccessVariant, ChatMediaByteStream, ChatMediaError, ChatMediaRangeRequest,
    ChatMediaService, ChatMediaStorage, ChatMediaStorageError, ChatMediaStreamAccess,
};
use mini_rs_erp::db::postgres::apply_foundation_migration;
use mini_rs_erp::db::postgres_chat::{PostgresChatStore, start_realtime_listener};
use mini_rs_erp::db::postgres_chat_media::PostgresChatMediaRepository;
use mini_rs_erp::store::chat_media_local::LocalChatMediaStorage;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops a mini_rs_erp_test_* database"]
async fn chat_delivery_is_idempotent_resumable_and_cross_process() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = format!("mini_rs_erp_test_chat_reliability_{}", std::process::id());
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

    let first_store = Arc::new(PostgresChatStore::new(pool.clone()));
    let second_store = Arc::new(PostgresChatStore::new(pool.clone()));
    let first = ChatService::new(first_store.clone());
    let second = ChatService::new(second_store.clone());
    let owner = principal("owner");
    let peer = principal("peer");
    let conversation = first
        .create_or_get_dm(principal_input("owner"), principal_input("peer"))
        .await
        .expect("create conversation");

    let mut owner_events = second.hub().subscribe(&owner).await;
    let mut peer_events = second.hub().subscribe(&peer).await;
    start_realtime_listener(pool.clone(), second.hub().clone());
    tokio::time::sleep(Duration::from_millis(150)).await;

    let sent = first
        .send_message(
            &owner,
            &conversation.conversation_id,
            "client-message-1",
            "Salom",
        )
        .await
        .expect("send message");
    assert!(sent.created);
    assert_eq!(sent.message.sequence, 1);

    let owner_event = tokio::time::timeout(Duration::from_secs(3), owner_events.recv())
        .await
        .expect("owner realtime timeout")
        .expect("owner realtime event");
    let peer_event = tokio::time::timeout(Duration::from_secs(3), peer_events.recv())
        .await
        .expect("peer realtime timeout")
        .expect("peer realtime event");
    assert_eq!(owner_event.event_id, peer_event.event_id);
    assert!(owner_event.cursor > 0);

    let repeated = first
        .send_message(
            &owner,
            &conversation.conversation_id,
            "client-message-1",
            "Salom",
        )
        .await
        .expect("repeat idempotent message");
    assert!(!repeated.created);
    assert_eq!(repeated.message.message_id, sent.message.message_id);
    assert_eq!(
        first
            .send_message(
                &owner,
                &conversation.conversation_id,
                "client-message-1",
                "Boshqa mazmun",
            )
            .await
            .unwrap_err(),
        mini_rs_erp::core::chat::ChatError::Conflict
    );
    let event_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mini_chat_outbox_events WHERE conversation_id = $1",
    )
    .bind(&conversation.conversation_id)
    .fetch_one(&pool)
    .await
    .expect("event count");
    assert_eq!(event_count, 1);

    for principal in [&owner, &peer] {
        let page = first.sync(principal, 0, 100).await.expect("sync page");
        assert_eq!(page.events.len(), 1);
        assert_eq!(page.events[0].message.message_id, sent.message.message_id);
        assert_eq!(page.next_cursor, owner_event.cursor);
        assert!(!page.has_more);
    }
    let outsider_page = first
        .sync(&principal("outsider"), 0, 100)
        .await
        .expect("outsider watermark");
    assert!(outsider_page.events.is_empty());
    assert_eq!(outsider_page.next_cursor, owner_event.cursor);

    let gap_page = first
        .messages(&peer, &conversation.conversation_id, None, Some(0), 100)
        .await
        .expect("message gap page");
    assert_eq!(gap_page.items.len(), 1);
    assert_eq!(gap_page.items[0].sequence, 1);

    first
        .mark_delivered(&peer, &conversation.conversation_id, 1, "peer-device")
        .await
        .expect("mark delivered");
    first
        .mark_read(&peer, &conversation.conversation_id, 1, "peer-device")
        .await
        .expect("mark read");
    let cursors = sqlx::query_as::<_, (i64, i64)>(
        r#"SELECT cursor.last_delivered_sequence, cursor.last_read_sequence
           FROM mini_chat_device_cursors cursor
           JOIN mini_chat_principals principal ON principal.principal_id = cursor.principal_id
           WHERE principal.principal_ref = 'peer'
             AND cursor.device_id = 'peer-device'
             AND cursor.conversation_id = $1"#,
    )
    .bind(&conversation.conversation_id)
    .fetch_one(&pool)
    .await
    .expect("device cursors");
    assert_eq!(cursors, (1, 1));

    let deliveries = first_store
        .claim_push_deliveries(10)
        .await
        .expect("claim push deliveries");
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].recipient_key, "customer:peer");
    first_store
        .mark_push_delivered(&deliveries[0].event_id, &deliveries[0].recipient_key)
        .await
        .expect("finish push delivery");

    let (ticket, _) = first
        .issue_socket_ticket(owner.clone())
        .await
        .expect("issue socket ticket");
    assert_eq!(
        second
            .consume_socket_ticket(&ticket)
            .await
            .expect("consume ticket in second process"),
        owner
    );
    assert!(second.consume_socket_ticket(&ticket).await.is_err());

    let media_directory = tempfile::tempdir().expect("media directory");
    let media_storage = Arc::new(LocalChatMediaStorage::new(media_directory.path()));
    let media_bytes = b"video01";
    media_storage
        .put_object(
            "chat_media/test/processed.mp4",
            "video/mp4",
            media_bytes.len() as i64,
            byte_stream(media_bytes),
        )
        .await
        .expect("store playback object");
    let owner_id: String = sqlx::query_scalar(
        "SELECT principal_id FROM mini_chat_principals WHERE principal_ref = 'owner'",
    )
    .fetch_one(&pool)
    .await
    .expect("owner principal id");
    sqlx::query(
        r#"INSERT INTO mini_chat_media
             (media_id, upload_id, conversation_id, uploader_principal_id,
              client_upload_id, media_kind, upload_status, original_filename,
              declared_content_type, declared_size_bytes, declared_duration_ms,
              source_object_key, actual_size_bytes, detected_content_type,
              processed_object_key, processed_content_type, processed_size_bytes,
              width_pixels, height_pixels, duration_ms, expires_at)
           VALUES
             ('media-video-1', 'upload-video-1', $1, $2,
              'client-upload-video-1', 'video', 'ready', 'video.mp4',
              'video/mp4', $3, 1000,
              'chat_media/test/source.mp4', $3, 'video/mp4',
              'chat_media/test/processed.mp4', 'video/mp4', $3,
              320, 180, 1000, now() + interval '1 day')"#,
    )
    .bind(&conversation.conversation_id)
    .bind(owner_id)
    .bind(media_bytes.len() as i64)
    .execute(&pool)
    .await
    .expect("seed ready video");
    sqlx::query(
        r#"INSERT INTO mini_chat_message_attachments
             (attachment_id, message_id, conversation_id, media_id, ordinal)
           VALUES ('attachment-video-1', $1, $2, 'media-video-1', 0)"#,
    )
    .bind(&sent.message.message_id)
    .bind(&conversation.conversation_id)
    .execute(&pool)
    .await
    .expect("attach ready video");
    let media = ChatMediaService::new(
        Arc::new(PostgresChatMediaRepository::new(pool.clone())),
        media_storage,
    );
    let (playback_ticket, expires_at_unix) = media
        .issue_playback_ticket(&peer, "media-video-1")
        .await
        .expect("issue playback ticket");
    assert!(expires_at_unix > time::OffsetDateTime::now_utc().unix_timestamp());
    assert_eq!(
        media
            .issue_playback_ticket(&principal("outsider"), "media-video-1")
            .await
            .unwrap_err(),
        ChatMediaError::NotFound
    );
    for range in [
        ChatMediaRangeRequest::From {
            start_byte: 1,
            end_byte_inclusive: Some(3),
        },
        ChatMediaRangeRequest::From {
            start_byte: 4,
            end_byte_inclusive: None,
        },
    ] {
        let ChatMediaStreamAccess::Local { content } = media
            .media_stream_access_with_ticket(
                "media-video-1",
                &playback_ticket,
                ChatMediaAccessVariant::Content,
                range,
            )
            .await
            .expect("ticket range playback")
        else {
            panic!("local playback must be proxied");
        };
        assert!(content.partial);
        assert_eq!(content.total_size_bytes, media_bytes.len() as i64);
        assert!(content.content_length() > 0);
    }
    assert!(matches!(
        media
            .media_stream_access_with_ticket(
                "media-video-1",
                "wrong-ticket",
                ChatMediaAccessVariant::Content,
                ChatMediaRangeRequest::Full,
            )
            .await,
        Err(ChatMediaError::NotFound)
    ));

    // Hold the same row lock used by send_message so both requests complete
    // their optimistic idempotency lookup before either can insert. Once the
    // lock is released, one creates the message and the other must return it as
    // an idempotent retry rather than failing on the unique constraint.
    let mut send_gate = pool.begin().await.expect("concurrent send gate");
    sqlx::query(
        "SELECT last_message_sequence FROM mini_chat_conversations WHERE conversation_id = $1 FOR UPDATE",
    )
    .bind(&conversation.conversation_id)
    .fetch_one(&mut *send_gate)
    .await
    .expect("lock conversation for concurrent send");
    let first_sender = first.clone();
    let first_owner = owner.clone();
    let first_conversation_id = conversation.conversation_id.clone();
    let first_send = tokio::spawn(async move {
        first_sender
            .send_message(
                &first_owner,
                &first_conversation_id,
                "client-message-concurrent",
                "Bir marta",
            )
            .await
    });
    let second_sender = second.clone();
    let second_owner = owner.clone();
    let second_conversation_id = conversation.conversation_id.clone();
    let second_send = tokio::spawn(async move {
        second_sender
            .send_message(
                &second_owner,
                &second_conversation_id,
                "client-message-concurrent",
                "Bir marta",
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    send_gate
        .commit()
        .await
        .expect("release concurrent send gate");
    let (first_result, second_result) = tokio::join!(first_send, second_send);
    let first_result = first_result
        .expect("first concurrent task")
        .expect("first send");
    let second_result = second_result
        .expect("second concurrent task")
        .expect("second send");
    assert_eq!(
        usize::from(first_result.created) + usize::from(second_result.created),
        1
    );
    assert_eq!(
        first_result.message.message_id,
        second_result.message.message_id
    );
    assert_eq!(first_result.message.sequence, 2);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM mini_chat_outbox_events WHERE conversation_id = $1",
        )
        .bind(&conversation.conversation_id)
        .fetch_one(&pool)
        .await
        .expect("concurrent send outbox count"),
        2
    );

    sqlx::query("UPDATE mini_chat_principals SET active = FALSE WHERE principal_ref = 'peer'")
        .execute(&pool)
        .await
        .expect("deactivate only push recipient");
    let no_push = first
        .send_message(
            &owner,
            &conversation.conversation_id,
            "client-message-no-push",
            "Push oluvchi yo'q",
        )
        .await
        .expect("send without push recipients");
    assert_eq!(no_push.message.sequence, 3);
    assert!(
        sqlx::query_scalar::<_, bool>(
            r#"SELECT published_at IS NOT NULL
               FROM mini_chat_outbox_events
               WHERE conversation_id = $1 AND message_sequence = 3"#,
        )
        .bind(&conversation.conversation_id)
        .fetch_one(&pool)
        .await
        .expect("zero-recipient outbox finalized")
    );

    let compatibility_cursor: i64 = sqlx::query_scalar(
        r#"INSERT INTO mini_chat_outbox_events
             (event_id, topic, conversation_id, message_sequence,
              recipient_keys, payload_json, published_at)
           VALUES
             ('legacy-event-1', 'chat.message.created', $1, 1,
              '[]'::JSONB, '{}'::JSONB, now())
           RETURNING event_cursor"#,
    )
    .bind(&conversation.conversation_id)
    .fetch_one(&pool)
    .await
    .expect("legacy writer cursor trigger");
    assert!(compatibility_cursor > owner_event.cursor);

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

fn principal_input(reference: &str) -> ChatPrincipalInput {
    ChatPrincipalInput {
        role: PrincipalRole::Customer,
        ref_: reference.to_string(),
        display_name: reference.to_string(),
        avatar_url: String::new(),
    }
}

fn database_url(admin_url: &str, database: &str) -> String {
    let mut url = reqwest::Url::parse(admin_url).expect("admin database URL");
    url.set_path(&format!("/{database}"));
    url.to_string()
}

fn byte_stream(value: &'static [u8]) -> ChatMediaByteStream {
    Box::pin(async_stream::stream! {
        yield Ok::<Bytes, ChatMediaStorageError>(Bytes::from_static(value));
    })
}
