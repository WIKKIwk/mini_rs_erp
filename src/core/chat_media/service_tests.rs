use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;

use super::{
    ChatMediaByteStream, ChatMediaCreateResult, ChatMediaError, ChatMediaInitializeInput,
    ChatMediaKind, ChatMediaRepository, ChatMediaService, ChatMediaStatus, ChatMediaStorage,
    ChatMediaStorageError, ChatMediaStorageObject, ChatMediaStorageUpload,
    ChatMediaUploadRecord, NewChatMediaUpload, MAX_CHAT_IMAGE_SIZE_BYTES,
    MAX_CHAT_VIDEO_DURATION_MS, MAX_CHAT_VIDEO_SIZE_BYTES,
};
use crate::core::auth::models::{Principal, PrincipalRole};

const CONVERSATION_ID: &str = "conversation_1";

#[tokio::test]
async fn initializes_private_upload_and_accepts_approved_limits() {
    let (service, repository, _) = service();

    let image = service
        .initialize_upload(
            &owner(),
            CONVERSATION_ID,
            input("image_1", ChatMediaKind::Image, MAX_CHAT_IMAGE_SIZE_BYTES),
        )
        .await
        .expect("image initialization");
    assert!(image.created);
    assert_eq!(image.upload.strategy, "local_proxy");
    assert_eq!(image.upload.method, "PUT");
    assert!(image.upload.url.ends_with("/content"));
    assert!(!image.upload.url.contains("chat_media/"));
    assert!(!serde_json::to_string(&image).unwrap().contains("source_object_key"));

    repository.clear();
    let mut video = input(
        "video_1",
        ChatMediaKind::Video,
        MAX_CHAT_VIDEO_SIZE_BYTES,
    );
    video.duration_ms = Some(MAX_CHAT_VIDEO_DURATION_MS);
    service
        .initialize_upload(&owner(), CONVERSATION_ID, video)
        .await
        .expect("video initialization");
}

#[tokio::test]
async fn rejects_oversized_mismatched_and_invalid_inputs() {
    let (service, _, _) = service();

    for request in [
        input(
            "large_image",
            ChatMediaKind::Image,
            MAX_CHAT_IMAGE_SIZE_BYTES + 1,
        ),
        input(
            "large_video",
            ChatMediaKind::Video,
            MAX_CHAT_VIDEO_SIZE_BYTES + 1,
        ),
    ] {
        assert_eq!(
            service
                .initialize_upload(&owner(), CONVERSATION_ID, request)
                .await
                .expect_err("oversized upload rejected"),
            ChatMediaError::TooLarge
        );
    }

    let mut invalid_duration = input("video", ChatMediaKind::Video, 10);
    invalid_duration.duration_ms = Some(MAX_CHAT_VIDEO_DURATION_MS + 1);
    assert_eq!(
        service
            .initialize_upload(&owner(), CONVERSATION_ID, invalid_duration)
            .await
            .unwrap_err(),
        ChatMediaError::InvalidInput
    );

    let mut wrong_type = input("wrong_type", ChatMediaKind::Image, 10);
    wrong_type.content_type = "video/mp4".to_string();
    assert_eq!(
        service
            .initialize_upload(&owner(), CONVERSATION_ID, wrong_type)
            .await
            .unwrap_err(),
        ChatMediaError::InvalidInput
    );

    let mut invalid_id = input("bad id", ChatMediaKind::Image, 10);
    invalid_id.client_upload_id = "bad/id".to_string();
    assert_eq!(
        service
            .initialize_upload(&owner(), CONVERSATION_ID, invalid_id)
            .await
            .unwrap_err(),
        ChatMediaError::InvalidInput
    );
}

#[tokio::test]
async fn client_upload_id_is_idempotent_but_conflicting_metadata_is_rejected() {
    let (service, _, _) = service();
    let request = input("same_id", ChatMediaKind::Image, 20);
    let first = service
        .initialize_upload(&owner(), CONVERSATION_ID, request.clone())
        .await
        .expect("first initialization");
    let second = service
        .initialize_upload(&owner(), CONVERSATION_ID, request)
        .await
        .expect("idempotent initialization");

    assert!(first.created);
    assert!(!second.created);
    assert_eq!(first.media.media_id, second.media.media_id);
    assert_eq!(first.media.upload_id, second.media.upload_id);

    let mut conflict = input("same_id", ChatMediaKind::Image, 21);
    conflict.filename = "different.jpg".to_string();
    assert_eq!(
        service
            .initialize_upload(&owner(), CONVERSATION_ID, conflict)
            .await
            .unwrap_err(),
        ChatMediaError::Conflict
    );
}

#[tokio::test]
async fn membership_can_post_ownership_and_conversation_are_enforced() {
    let (service, repository, _) = service();
    let initialized = service
        .initialize_upload(
            &owner(),
            CONVERSATION_ID,
            input("owned", ChatMediaKind::Image, 3),
        )
        .await
        .expect("initialization");

    assert_eq!(
        service
            .upload_status(&intruder(), CONVERSATION_ID, &initialized.media.upload_id)
            .await
            .unwrap_err(),
        ChatMediaError::Forbidden
    );
    assert_eq!(
        service
            .upload_status(&owner(), "conversation_2", &initialized.media.upload_id)
            .await
            .unwrap_err(),
        ChatMediaError::Forbidden
    );

    repository.can_post.store(false, Ordering::SeqCst);
    assert_eq!(
        service
            .upload_content(
                &owner(),
                CONVERSATION_ID,
                &initialized.media.upload_id,
                Some(3),
                Some("image/jpeg"),
                byte_stream(b"abc"),
            )
            .await
            .unwrap_err(),
        ChatMediaError::Forbidden
    );
    assert!(service
        .upload_status(&owner(), CONVERSATION_ID, &initialized.media.upload_id)
        .await
        .is_ok());
}

#[tokio::test]
async fn local_upload_completion_is_exact_and_enqueues_one_durable_job() {
    let (service, repository, _) = service();
    let initialized = service
        .initialize_upload(
            &owner(),
            CONVERSATION_ID,
            input("complete", ChatMediaKind::Image, 3),
        )
        .await
        .expect("initialization");
    let uploaded = service
        .upload_content(
            &owner(),
            CONVERSATION_ID,
            &initialized.media.upload_id,
            Some(3),
            Some("image/jpeg"),
            byte_stream(b"abc"),
        )
        .await
        .expect("upload content");
    assert_eq!(uploaded.status, ChatMediaStatus::Uploaded);

    let completed = service
        .complete_upload(&owner(), CONVERSATION_ID, &initialized.media.upload_id)
        .await
        .expect("completion");
    assert_eq!(completed.status, ChatMediaStatus::Processing);
    assert_eq!(repository.jobs.load(Ordering::SeqCst), 1);

    service
        .complete_upload(&owner(), CONVERSATION_ID, &initialized.media.upload_id)
        .await
        .expect("idempotent completion");
    assert_eq!(repository.jobs.load(Ordering::SeqCst), 1);
    assert_eq!(
        service
            .initialize_upload(
                &owner(),
                CONVERSATION_ID,
                input("complete", ChatMediaKind::Image, 3),
            )
            .await
            .unwrap_err(),
        ChatMediaError::Conflict
    );
    assert_eq!(
        service
            .cancel_upload(&owner(), CONVERSATION_ID, &initialized.media.upload_id)
            .await
            .unwrap_err(),
        ChatMediaError::Conflict
    );
}

#[tokio::test]
async fn local_upload_requires_the_declared_content_type() {
    let (service, _, _) = service();
    let initialized = service
        .initialize_upload(
            &owner(),
            CONVERSATION_ID,
            input("content_type", ChatMediaKind::Image, 3),
        )
        .await
        .expect("initialization");

    for content_type in [None, Some("video/mp4")] {
        assert_eq!(
            service
                .upload_content(
                    &owner(),
                    CONVERSATION_ID,
                    &initialized.media.upload_id,
                    Some(3),
                    content_type,
                    byte_stream(b"abc"),
                )
                .await
                .unwrap_err(),
            ChatMediaError::InvalidInput
        );
    }
}

#[tokio::test]
async fn completion_rejects_storage_size_mismatch() {
    let (service, _, storage) = service();
    let initialized = service
        .initialize_upload(
            &owner(),
            CONVERSATION_ID,
            input("mismatch", ChatMediaKind::Image, 3),
        )
        .await
        .expect("initialization");
    *storage.metadata.lock().unwrap() = Some(ChatMediaStorageObject {
        size_bytes: 2,
        content_type: Some("image/jpeg".to_string()),
        etag: None,
    });

    assert_eq!(
        service
            .complete_upload(&owner(), CONVERSATION_ID, &initialized.media.upload_id)
            .await
            .unwrap_err(),
        ChatMediaError::InvalidInput
    );
}

#[tokio::test]
async fn cancellation_and_orphan_cleanup_remove_private_objects() {
    let (service, repository, storage) = service();
    let initialized = service
        .initialize_upload(
            &owner(),
            CONVERSATION_ID,
            input("cancel", ChatMediaKind::Image, 3),
        )
        .await
        .expect("initialization");
    let cancelled = service
        .cancel_upload(&owner(), CONVERSATION_ID, &initialized.media.upload_id)
        .await
        .expect("cancel");
    assert_eq!(cancelled.status, ChatMediaStatus::Cancelled);
    assert!(storage.deleted.load(Ordering::SeqCst));

    storage.deleted.store(false, Ordering::SeqCst);
    let cleaned = service.cleanup_orphaned_uploads(10).await.expect("cleanup");
    assert_eq!(cleaned, 1);
    assert!(storage.deleted.load(Ordering::SeqCst));
    assert!(repository.cleaned.load(Ordering::SeqCst));
}

fn service() -> (
    ChatMediaService,
    Arc<MemoryRepository>,
    Arc<MemoryStorage>,
) {
    let repository = Arc::new(MemoryRepository::default());
    let storage = Arc::new(MemoryStorage::default());
    (
        ChatMediaService::new(repository.clone(), storage.clone()),
        repository,
        storage,
    )
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

fn input(client_id: &str, kind: ChatMediaKind, size_bytes: i64) -> ChatMediaInitializeInput {
    ChatMediaInitializeInput {
        client_upload_id: client_id.to_string(),
        kind,
        filename: match kind {
            ChatMediaKind::Image => "photo.jpg",
            ChatMediaKind::Video => "video.mp4",
        }
        .to_string(),
        content_type: match kind {
            ChatMediaKind::Image => "image/jpeg",
            ChatMediaKind::Video => "video/mp4",
        }
        .to_string(),
        size_bytes,
        duration_ms: None,
    }
}

fn byte_stream(bytes: &'static [u8]) -> ChatMediaByteStream {
    Box::pin(async_stream::stream! {
        yield Ok(Bytes::from_static(bytes));
    })
}

struct MemoryRepository {
    record: Mutex<Option<ChatMediaUploadRecord>>,
    can_post: AtomicBool,
    jobs: AtomicUsize,
    cleaned: AtomicBool,
}

impl MemoryRepository {
    fn authorize(
        &self,
        principal: &Principal,
        conversation_id: &str,
        require_can_post: bool,
    ) -> Result<(), ChatMediaError> {
        if principal.ref_ != "owner" || conversation_id != CONVERSATION_ID {
            return Err(ChatMediaError::Forbidden);
        }
        if require_can_post && !self.can_post.load(Ordering::SeqCst) {
            return Err(ChatMediaError::Forbidden);
        }
        Ok(())
    }

    fn clear(&self) {
        *self.record.lock().unwrap() = None;
    }
}

impl Default for MemoryRepository {
    fn default() -> Self {
        Self {
            record: Mutex::new(None),
            can_post: AtomicBool::new(true),
            jobs: AtomicUsize::new(0),
            cleaned: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl ChatMediaRepository for MemoryRepository {
    async fn initialize_upload(
        &self,
        principal: &Principal,
        upload: NewChatMediaUpload,
    ) -> Result<ChatMediaCreateResult, ChatMediaError> {
        self.authorize(principal, &upload.conversation_id, true)?;
        let mut stored = self.record.lock().unwrap();
        if let Some(record) = stored.as_ref() {
            if record.client_upload_id == upload.client_upload_id {
                return Ok(ChatMediaCreateResult {
                    record: record.clone(),
                    created: false,
                });
            }
        }
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let record = ChatMediaUploadRecord {
            media_id: upload.media_id,
            upload_id: upload.upload_id,
            conversation_id: upload.conversation_id,
            uploader_principal_id: "principal_owner".to_string(),
            client_upload_id: upload.client_upload_id,
            kind: upload.kind,
            status: ChatMediaStatus::Pending,
            original_filename: upload.original_filename,
            declared_content_type: upload.declared_content_type,
            declared_size_bytes: upload.declared_size_bytes,
            declared_duration_ms: upload.declared_duration_ms,
            source_object_key: upload.source_object_key,
            actual_size_bytes: None,
            storage_etag: None,
            error_code: None,
            expires_at_unix: upload.expires_at_unix,
            created_at_unix: now,
            updated_at_unix: now,
        };
        *stored = Some(record.clone());
        Ok(ChatMediaCreateResult {
            record,
            created: true,
        })
    }

    async fn upload(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        require_can_post: bool,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        self.authorize(principal, conversation_id, require_can_post)?;
        self.record
            .lock()
            .unwrap()
            .clone()
            .filter(|record| record.upload_id == upload_id)
            .ok_or(ChatMediaError::NotFound)
    }

    async fn mark_uploaded(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        storage: &ChatMediaStorageObject,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        self.authorize(principal, conversation_id, true)?;
        let mut record = self.record.lock().unwrap();
        let record = record.as_mut().ok_or(ChatMediaError::NotFound)?;
        if record.upload_id != upload_id {
            return Err(ChatMediaError::NotFound);
        }
        record.status = ChatMediaStatus::Uploaded;
        record.actual_size_bytes = Some(storage.size_bytes);
        Ok(record.clone())
    }

    async fn complete_upload(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        storage: &ChatMediaStorageObject,
        _job_id: &str,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        self.authorize(principal, conversation_id, true)?;
        let mut record = self.record.lock().unwrap();
        let record = record.as_mut().ok_or(ChatMediaError::NotFound)?;
        if record.upload_id != upload_id {
            return Err(ChatMediaError::NotFound);
        }
        if record.status != ChatMediaStatus::Processing {
            self.jobs.fetch_add(1, Ordering::SeqCst);
        }
        record.status = ChatMediaStatus::Processing;
        record.actual_size_bytes = Some(storage.size_bytes);
        Ok(record.clone())
    }

    async fn cancel_upload(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        self.authorize(principal, conversation_id, false)?;
        let mut record = self.record.lock().unwrap();
        let record = record.as_mut().ok_or(ChatMediaError::NotFound)?;
        if record.upload_id != upload_id {
            return Err(ChatMediaError::NotFound);
        }
        if matches!(record.status, ChatMediaStatus::Processing | ChatMediaStatus::Ready) {
            return Err(ChatMediaError::Conflict);
        }
        record.status = ChatMediaStatus::Cancelled;
        Ok(record.clone())
    }

    async fn claim_orphaned_uploads(
        &self,
        _now_unix: i64,
        _limit: usize,
    ) -> Result<Vec<ChatMediaUploadRecord>, ChatMediaError> {
        Ok(self.record.lock().unwrap().clone().into_iter().collect())
    }

    async fn mark_orphan_cleaned(&self, _media_id: &str) -> Result<(), ChatMediaError> {
        self.cleaned.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn release_orphan_cleanup(&self, _media_id: &str) -> Result<(), ChatMediaError> {
        Ok(())
    }
}

#[derive(Default)]
struct MemoryStorage {
    metadata: Mutex<Option<ChatMediaStorageObject>>,
    deleted: AtomicBool,
}

#[async_trait]
impl ChatMediaStorage for MemoryStorage {
    async fn prepare_upload(
        &self,
        _object_key: &str,
        _content_type: &str,
        _expected_size_bytes: i64,
    ) -> Result<ChatMediaStorageUpload, ChatMediaStorageError> {
        Ok(ChatMediaStorageUpload::LocalProxy)
    }

    async fn put_object(
        &self,
        _object_key: &str,
        content_type: &str,
        expected_size_bytes: i64,
        mut stream: ChatMediaByteStream,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        let mut size = 0_i64;
        while let Some(chunk) =
            std::future::poll_fn(|context| stream.as_mut().poll_next(context)).await
        {
            size += i64::try_from(chunk?.len())
                .map_err(|_| ChatMediaStorageError::SizeMismatch)?;
        }
        if size != expected_size_bytes {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        let metadata = ChatMediaStorageObject {
            size_bytes: size,
            content_type: Some(content_type.to_string()),
            etag: None,
        };
        *self.metadata.lock().unwrap() = Some(metadata.clone());
        Ok(metadata)
    }

    async fn object_metadata(
        &self,
        _object_key: &str,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        self.metadata
            .lock()
            .unwrap()
            .clone()
            .ok_or(ChatMediaStorageError::ObjectNotFound)
    }

    async fn delete_object(&self, _object_key: &str) -> Result<(), ChatMediaStorageError> {
        self.deleted.store(true, Ordering::SeqCst);
        Ok(())
    }
}
