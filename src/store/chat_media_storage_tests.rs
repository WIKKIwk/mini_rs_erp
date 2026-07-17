use bytes::Bytes;

use super::chat_media_local::LocalChatMediaStorage;
use super::chat_media_r2::{R2ChatMediaConfig, R2ChatMediaStorage};
use crate::core::chat_media::{
    ChatMediaByteStream, ChatMediaStorage, ChatMediaStorageDownload, ChatMediaStorageError,
    ChatMediaStorageUpload,
};

#[tokio::test]
async fn local_chat_media_storage_writes_exact_bytes_and_deletes_privately() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let storage = LocalChatMediaStorage::new(directory.path());
    let object_key = "chat_media/conversation_1/media_1/source";

    assert_eq!(
        storage
            .prepare_upload(object_key, "image/jpeg", 6)
            .await
            .expect("prepare"),
        ChatMediaStorageUpload::LocalProxy
    );
    let stored = storage
        .put_object(
            object_key,
            "image/jpeg",
            6,
            chunks(vec![b"abc", b"def"]),
        )
        .await
        .expect("store");
    assert_eq!(stored.size_bytes, 6);
    assert_eq!(stored.content_type.as_deref(), Some("image/jpeg"));
    assert_eq!(
        storage
            .object_metadata(object_key)
            .await
            .expect("metadata"),
        stored
    );
    assert!(directory.path().join(object_key).is_file());

    storage.delete_object(object_key).await.expect("delete");
    assert_eq!(
        storage.object_metadata(object_key).await.unwrap_err(),
        ChatMediaStorageError::ObjectNotFound
    );
}

#[tokio::test]
async fn local_chat_media_storage_rejects_size_mismatch_and_path_traversal() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let storage = LocalChatMediaStorage::new(directory.path());

    assert_eq!(
        storage
            .put_object(
                "chat_media/conversation_1/media_1/source",
                "image/jpeg",
                4,
                chunks(vec![b"abc"]),
            )
            .await
            .unwrap_err(),
        ChatMediaStorageError::SizeMismatch
    );
    assert_eq!(
        storage
            .prepare_upload("chat_media/../secret", "image/jpeg", 1)
            .await
            .unwrap_err(),
        ChatMediaStorageError::InvalidObjectKey
    );
}

#[tokio::test]
async fn r2_chat_media_storage_returns_only_short_lived_private_put_access() {
    let storage = R2ChatMediaStorage::new(R2ChatMediaConfig {
        endpoint: "https://account.r2.cloudflarestorage.com".to_string(),
        bucket: "private-chat".to_string(),
        access_key_id: "test-key".to_string(),
        secret_access_key: "super-secret-value".to_string(),
        region: "auto".to_string(),
        upload_url_ttl_seconds: 300,
        client: reqwest::Client::new(),
    });

    let upload = storage
        .prepare_upload(
            "chat_media/conversation_1/media_1/source",
            "video/mp4",
            1024,
        )
        .await
        .expect("prepare R2 upload");
    let ChatMediaStorageUpload::DirectPut {
        url,
        headers,
        expires_at_unix,
    } = upload
    else {
        panic!("R2 must use direct PUT");
    };
    assert!(url.starts_with(
        "https://account.r2.cloudflarestorage.com/private-chat/chat_media/"
    ));
    assert!(url.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
    assert!(url.contains("X-Amz-Expires=300"));
    assert!(url.contains("X-Amz-Signature="));
    assert!(!url.contains("super-secret-value"));
    assert_eq!(headers.get("content-type").map(String::as_str), Some("video/mp4"));
    assert!(expires_at_unix > time::OffsetDateTime::now_utc().unix_timestamp());
    assert_eq!(
        storage
            .put_object(
                "chat_media/conversation_1/media_1/source",
                "video/mp4",
                1,
                chunks(vec![b"x"]),
            )
            .await
            .unwrap_err(),
        ChatMediaStorageError::DirectUploadRequired
    );

    let download = storage
        .prepare_download("chat_media/conversation_1/media_1/processed")
        .await
        .expect("prepare R2 download");
    let ChatMediaStorageDownload::DirectGet {
        url,
        expires_at_unix,
    } = download
    else {
        panic!("R2 must use a short-lived private GET");
    };
    assert!(url.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
    assert!(url.contains("X-Amz-Expires=300"));
    assert!(url.contains("X-Amz-Signature="));
    assert!(!url.contains("super-secret-value"));
    assert!(expires_at_unix > time::OffsetDateTime::now_utc().unix_timestamp());
}

fn chunks(chunks: Vec<&'static [u8]>) -> ChatMediaByteStream {
    Box::pin(async_stream::stream! {
        for chunk in chunks {
            yield Ok(Bytes::copy_from_slice(chunk));
        }
    })
}
