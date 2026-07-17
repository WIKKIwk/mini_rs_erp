use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;

use super::{
    ChatMediaAccess, ChatMediaAccessVariant, ChatMediaByteStream, ChatMediaCreateResult,
    ChatMediaError, ChatMediaInitializeInput, ChatMediaKind, ChatMediaMultipartUpload,
    ChatMediaProcessedContent, ChatMediaProcessedFiles, ChatMediaProcessingError,
    ChatMediaProcessingWorkItem,
    ChatMediaProcessor, ChatMediaReadyInput, ChatMediaRepository, ChatMediaService,
    ChatMediaStatus, ChatMediaStorage, ChatMediaStorageDownload, ChatMediaStorageError,
    ChatMediaStorageObject, ChatMediaStoragePart, ChatMediaStorageUpload,
    ChatMediaStoredContent, ChatMediaUploadMode, ChatMediaUploadRecord,
    ChatMediaUploadedChunk, NewChatMediaUpload, NewChatMediaUploadedChunk,
    MAX_CHAT_IMAGE_SIZE_BYTES, MAX_CHAT_PROCESSED_VIDEO_SIZE_BYTES,
    MAX_CHAT_VIDEO_DURATION_MS, MAX_CHAT_VIDEO_SIZE_BYTES,
};
use crate::core::auth::models::{Principal, PrincipalRole};

const CONVERSATION_ID: &str = "conversation_1";
const TEST_VIDEO_CHUNK_SIZE: i64 = 5 * 1024 * 1024;

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
    let initialized = service
        .initialize_upload(&owner(), CONVERSATION_ID, video)
        .await
        .expect("video initialization");
    assert_eq!(initialized.media.upload_mode, ChatMediaUploadMode::Chunked);
    assert_eq!(initialized.upload.strategy, "resumable_chunks");
    assert_eq!(initialized.upload.chunk_size_bytes, Some(8 * 1024 * 1024));
    assert_eq!(initialized.upload.total_chunks, Some(256));
    assert!(initialized.media.uploaded_chunks.is_empty());
}

#[tokio::test]
async fn chunked_video_resumes_after_restart_and_skips_duplicate_chunks() {
    let repository = Arc::new(MemoryRepository::default());
    let storage = Arc::new(MemoryStorage::default());
    let service = ChatMediaService::new(repository.clone(), storage.clone())
        .with_video_chunk_size(TEST_VIDEO_CHUNK_SIZE);
    let total_size = TEST_VIDEO_CHUNK_SIZE + 3;
    let mut request = input("resumable_video", ChatMediaKind::Video, total_size);
    request.duration_ms = Some(60_000);
    let initialized = service
        .initialize_upload(&owner(), CONVERSATION_ID, request.clone())
        .await
        .expect("initialize resumable upload");
    assert_eq!(initialized.upload.total_chunks, Some(2));

    let first_range = format!("bytes 0-{}/{total_size}", TEST_VIDEO_CHUNK_SIZE - 1);
    service
        .upload_chunk(
            &owner(),
            CONVERSATION_ID,
            &initialized.media.upload_id,
            0,
            Some(TEST_VIDEO_CHUNK_SIZE),
            Some(&first_range),
            owned_byte_stream(vec![7_u8; TEST_VIDEO_CHUNK_SIZE as usize]),
        )
        .await
        .expect("upload first chunk");
    assert_eq!(storage.multipart_part_puts.load(Ordering::SeqCst), 1);

    let restarted = ChatMediaService::new(repository.clone(), storage.clone());
    let recovered = restarted
        .initialize_upload(&owner(), CONVERSATION_ID, request)
        .await
        .expect("recover persisted upload");
    assert!(!recovered.created);
    assert_eq!(recovered.media.uploaded_chunks.len(), 1);
    assert_eq!(recovered.media.uploaded_chunks[0].chunk_index, 0);

    restarted
        .upload_chunk(
            &owner(),
            CONVERSATION_ID,
            &initialized.media.upload_id,
            0,
            Some(TEST_VIDEO_CHUNK_SIZE),
            Some(&first_range),
            owned_byte_stream(Vec::new()),
        )
        .await
        .expect("duplicate chunk is idempotent");
    assert_eq!(
        storage.multipart_part_puts.load(Ordering::SeqCst),
        1,
        "an already persisted chunk must not be uploaded again"
    );

    let final_range = format!(
        "bytes {}-{}/{total_size}",
        TEST_VIDEO_CHUNK_SIZE,
        TEST_VIDEO_CHUNK_SIZE + 2
    );
    restarted
        .upload_chunk(
            &owner(),
            CONVERSATION_ID,
            &initialized.media.upload_id,
            1,
            Some(3),
            Some(&final_range),
            owned_byte_stream(vec![8, 9, 10]),
        )
        .await
        .expect("upload final chunk");
    let completed = restarted
        .complete_upload(&owner(), CONVERSATION_ID, &initialized.media.upload_id)
        .await
        .expect("complete resumed upload");
    assert_eq!(completed.status, ChatMediaStatus::Processing);
    assert_eq!(completed.uploaded_chunks.len(), 2);
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
        ChatMediaError::DurationTooLong
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
    let cancelled = service
        .cancel_upload(&owner(), CONVERSATION_ID, &initialized.media.upload_id)
        .await
        .expect("processing cancellation");
    assert_eq!(cancelled.status, ChatMediaStatus::Cancelled);
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

#[tokio::test]
async fn cancelling_chunked_video_aborts_persisted_multipart_state() {
    let (service, _, storage) = service();
    let mut request = input("cancel_video", ChatMediaKind::Video, 3);
    request.duration_ms = Some(1_000);
    let initialized = service
        .initialize_upload(&owner(), CONVERSATION_ID, request)
        .await
        .expect("initialize video");

    let cancelled = service
        .cancel_upload(&owner(), CONVERSATION_ID, &initialized.media.upload_id)
        .await
        .expect("cancel chunked upload");

    assert_eq!(cancelled.status, ChatMediaStatus::Cancelled);
    assert!(storage.multipart_aborted.load(Ordering::SeqCst));
    assert!(storage.multipart_parts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn processing_canonicalizes_objects_marks_ready_and_serves_authorized_variants() {
    let (service, repository, storage) = service_with_processor(Ok(ChatMediaProcessedContent {
        content: Bytes::from_static(b"canonical"),
        content_type: "image/jpeg".to_string(),
        thumbnail: Bytes::from_static(b"thumbnail"),
        thumbnail_content_type: "image/jpeg".to_string(),
        width_pixels: 1200,
        height_pixels: 800,
        duration_ms: None,
        frame_rate_milli: None,
        video_codec: None,
        audio_codec: None,
    }));
    let initialized = service
        .initialize_upload(
            &owner(),
            CONVERSATION_ID,
            input("process", ChatMediaKind::Image, 3),
        )
        .await
        .expect("initialize");
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
        .expect("upload");
    service
        .complete_upload(&owner(), CONVERSATION_ID, &initialized.media.upload_id)
        .await
        .expect("complete");

    assert_eq!(service.process_pending_jobs(2).await.expect("process"), 1);
    let ready = repository.record.lock().unwrap().clone().expect("record");
    assert_eq!(ready.status, ChatMediaStatus::Ready);
    assert_eq!(ready.width_pixels, Some(1200));
    assert_eq!(ready.height_pixels, Some(800));
    assert_eq!(ready.processed_content_type.as_deref(), Some("image/jpeg"));

    let ChatMediaAccess::Local { content } = service
        .media_access(
            &owner(),
            &initialized.media.media_id,
            ChatMediaAccessVariant::Content,
        )
        .await
        .expect("content access")
    else {
        panic!("local storage must use an authorized proxy");
    };
    assert_eq!(content.bytes, Bytes::from_static(b"canonical"));
    let ChatMediaAccess::Local { content } = service
        .media_access(
            &owner(),
            &initialized.media.media_id,
            ChatMediaAccessVariant::Thumbnail,
        )
        .await
        .expect("thumbnail access")
    else {
        panic!("local storage must use an authorized proxy");
    };
    assert_eq!(content.bytes, Bytes::from_static(b"thumbnail"));
    assert_eq!(
        service
            .media_access(
                &intruder(),
                &initialized.media.media_id,
                ChatMediaAccessVariant::Content,
            )
            .await
            .unwrap_err(),
        ChatMediaError::Forbidden
    );
    assert!(storage.deleted.load(Ordering::SeqCst));
}

#[tokio::test]
async fn processing_failure_is_visible_in_upload_status() {
    let (service, repository, _) =
        service_with_processor(Err(ChatMediaProcessingError::InvalidContent));
    let initialized = service
        .initialize_upload(
            &owner(),
            CONVERSATION_ID,
            input("invalid", ChatMediaKind::Image, 3),
        )
        .await
        .expect("initialize");
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
        .expect("upload");
    service
        .complete_upload(&owner(), CONVERSATION_ID, &initialized.media.upload_id)
        .await
        .expect("complete");

    assert_eq!(service.process_pending_jobs(1).await.expect("process"), 1);
    let failed = repository.record.lock().unwrap().clone().expect("record");
    assert_eq!(failed.status, ChatMediaStatus::Failed);
    assert_eq!(failed.error_code.as_deref(), Some("invalid_media_content"));
}

#[tokio::test]
async fn final_processed_video_size_limit_is_enforced() {
    let repository = Arc::new(MemoryRepository::default());
    let storage = Arc::new(MemoryStorage::default());
    let service = ChatMediaService::new(repository.clone(), storage)
        .with_processor(Arc::new(OversizedFileProcessor));
    let mut request = input("oversized_processed", ChatMediaKind::Video, 3);
    request.duration_ms = Some(1_000);
    let initialized = service
        .initialize_upload(&owner(), CONVERSATION_ID, request)
        .await
        .expect("initialize video");
    service
        .upload_chunk(
            &owner(),
            CONVERSATION_ID,
            &initialized.media.upload_id,
            0,
            Some(3),
            Some("bytes 0-2/3"),
            owned_byte_stream(vec![1, 2, 3]),
        )
        .await
        .expect("upload video");
    service
        .complete_upload(&owner(), CONVERSATION_ID, &initialized.media.upload_id)
        .await
        .expect("complete video");

    assert_eq!(service.process_pending_jobs(1).await.expect("process"), 1);
    let failed = repository.record.lock().unwrap().clone().expect("record");
    assert_eq!(failed.status, ChatMediaStatus::Failed);
    assert_eq!(
        failed.error_code.as_deref(),
        Some("processed_video_too_large")
    );
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

fn service_with_processor(
    result: Result<ChatMediaProcessedContent, ChatMediaProcessingError>,
) -> (
    ChatMediaService,
    Arc<MemoryRepository>,
    Arc<MemoryStorage>,
) {
    let repository = Arc::new(MemoryRepository::default());
    let storage = Arc::new(MemoryStorage::default());
    let service = ChatMediaService::new(repository.clone(), storage.clone())
        .with_processor(Arc::new(StaticProcessor { result }));
    (service, repository, storage)
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

fn owned_byte_stream(bytes: Vec<u8>) -> ChatMediaByteStream {
    Box::pin(async_stream::stream! {
        yield Ok(Bytes::from(bytes));
    })
}

struct MemoryRepository {
    record: Mutex<Option<ChatMediaUploadRecord>>,
    chunks: Mutex<Vec<ChatMediaUploadedChunk>>,
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
        self.chunks.lock().unwrap().clear();
    }
}

impl Default for MemoryRepository {
    fn default() -> Self {
        Self {
            record: Mutex::new(None),
            chunks: Mutex::new(Vec::new()),
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
            upload_mode: upload.upload_mode,
            chunk_size_bytes: upload.chunk_size_bytes,
            total_chunks: upload.total_chunks,
            storage_multipart_upload_id: None,
            source_object_key: upload.source_object_key,
            actual_size_bytes: None,
            storage_etag: None,
            detected_content_type: None,
            processed_object_key: None,
            thumbnail_object_key: None,
            processed_content_type: None,
            processed_size_bytes: None,
            processed_etag: None,
            width_pixels: None,
            height_pixels: None,
            duration_ms: None,
            frame_rate_milli: None,
            video_codec: None,
            audio_codec: None,
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

    async fn set_multipart_upload_id(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        storage_upload_id: &str,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        self.authorize(principal, conversation_id, true)?;
        let mut record = self.record.lock().unwrap();
        let record = record.as_mut().ok_or(ChatMediaError::NotFound)?;
        if record.upload_id != upload_id || record.upload_mode != ChatMediaUploadMode::Chunked {
            return Err(ChatMediaError::Conflict);
        }
        if record.storage_multipart_upload_id.is_none() {
            record.storage_multipart_upload_id = Some(storage_upload_id.to_string());
        }
        Ok(record.clone())
    }

    async fn uploaded_chunks(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        require_can_post: bool,
    ) -> Result<Vec<ChatMediaUploadedChunk>, ChatMediaError> {
        self.authorize(principal, conversation_id, require_can_post)?;
        let record = self.record.lock().unwrap();
        if record.as_ref().is_none_or(|record| record.upload_id != upload_id) {
            return Err(ChatMediaError::NotFound);
        }
        Ok(self.chunks.lock().unwrap().clone())
    }

    async fn record_uploaded_chunk(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        chunk: NewChatMediaUploadedChunk,
    ) -> Result<ChatMediaUploadedChunk, ChatMediaError> {
        self.authorize(principal, conversation_id, true)?;
        let record = self.record.lock().unwrap();
        if record.as_ref().is_none_or(|record| record.upload_id != upload_id) {
            return Err(ChatMediaError::NotFound);
        }
        drop(record);
        let mut chunks = self.chunks.lock().unwrap();
        if let Some(existing) = chunks
            .iter()
            .find(|existing| existing.chunk_index == chunk.chunk_index)
        {
            return Ok(existing.clone());
        }
        let uploaded = ChatMediaUploadedChunk {
            chunk_index: chunk.chunk_index,
            offset_bytes: chunk.offset_bytes,
            size_bytes: chunk.size_bytes,
            storage_part_etag: chunk.storage_part_etag,
            uploaded_at_unix: time::OffsetDateTime::now_utc().unix_timestamp(),
        };
        chunks.push(uploaded.clone());
        chunks.sort_by_key(|chunk| chunk.chunk_index);
        Ok(uploaded)
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
        if record.status == ChatMediaStatus::Ready {
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

    async fn claim_processing_jobs(
        &self,
        _limit: usize,
    ) -> Result<Vec<ChatMediaProcessingWorkItem>, ChatMediaError> {
        let record = self.record.lock().unwrap().clone();
        Ok(record
            .filter(|record| record.status == ChatMediaStatus::Processing)
            .map(|media| ChatMediaProcessingWorkItem {
                job_id: "job_1".to_string(),
                attempts: 1,
                max_attempts: 5,
                media,
            })
            .into_iter()
            .collect())
    }

    async fn mark_processing_ready(
        &self,
        _job_id: &str,
        media_id: &str,
        ready: &ChatMediaReadyInput,
    ) -> Result<(), ChatMediaError> {
        let mut record = self.record.lock().unwrap();
        let record = record.as_mut().ok_or(ChatMediaError::NotFound)?;
        if record.media_id != media_id || record.status != ChatMediaStatus::Processing {
            return Err(ChatMediaError::Conflict);
        }
        record.status = ChatMediaStatus::Ready;
        record.processed_object_key = Some(ready.processed_object_key.clone());
        record.thumbnail_object_key = Some(ready.thumbnail_object_key.clone());
        record.processed_content_type = Some(ready.processed_content_type.clone());
        record.processed_size_bytes = Some(ready.processed_size_bytes);
        record.width_pixels = Some(ready.width_pixels);
        record.height_pixels = Some(ready.height_pixels);
        record.duration_ms = ready.duration_ms;
        record.frame_rate_milli = ready.frame_rate_milli;
        record.video_codec = ready.video_codec.clone();
        record.audio_codec = ready.audio_codec.clone();
        Ok(())
    }

    async fn mark_processing_failed(
        &self,
        _job_id: &str,
        media_id: &str,
        error_code: &str,
    ) -> Result<(), ChatMediaError> {
        let mut record = self.record.lock().unwrap();
        let record = record.as_mut().ok_or(ChatMediaError::NotFound)?;
        if record.media_id == media_id && record.status == ChatMediaStatus::Processing {
            record.status = ChatMediaStatus::Failed;
            record.error_code = Some(error_code.to_string());
        }
        Ok(())
    }

    async fn media_for_access(
        &self,
        principal: &Principal,
        media_id: &str,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        self.authorize(principal, CONVERSATION_ID, false)?;
        self.record
            .lock()
            .unwrap()
            .clone()
            .filter(|record| record.media_id == media_id && record.status == ChatMediaStatus::Ready)
            .ok_or(ChatMediaError::NotFound)
    }
}

#[derive(Default)]
struct MemoryStorage {
    metadata: Mutex<Option<ChatMediaStorageObject>>,
    objects: Mutex<HashMap<String, ChatMediaStoredContent>>,
    deleted: AtomicBool,
    multipart_parts: Mutex<HashMap<i32, Bytes>>,
    multipart_part_puts: AtomicUsize,
    multipart_aborted: AtomicBool,
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
        object_key: &str,
        content_type: &str,
        expected_size_bytes: i64,
        mut stream: ChatMediaByteStream,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        let mut bytes = Vec::new();
        while let Some(chunk) =
            std::future::poll_fn(|context| stream.as_mut().poll_next(context)).await
        {
            bytes.extend_from_slice(&chunk?);
        }
        let size = i64::try_from(bytes.len()).map_err(|_| ChatMediaStorageError::SizeMismatch)?;
        if size != expected_size_bytes {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        let metadata = ChatMediaStorageObject {
            size_bytes: size,
            content_type: Some(content_type.to_string()),
            etag: None,
        };
        *self.metadata.lock().unwrap() = Some(metadata.clone());
        self.objects.lock().unwrap().insert(
            object_key.to_string(),
            ChatMediaStoredContent {
                bytes: Bytes::from(bytes),
                content_type: Some(content_type.to_string()),
                etag: None,
            },
        );
        Ok(metadata)
    }

    async fn begin_multipart_upload(
        &self,
        _object_key: &str,
        _content_type: &str,
    ) -> Result<ChatMediaMultipartUpload, ChatMediaStorageError> {
        Ok(ChatMediaMultipartUpload {
            storage_upload_id: "memory_multipart".to_string(),
        })
    }

    async fn put_multipart_part(
        &self,
        _object_key: &str,
        _storage_upload_id: &str,
        part_number: i32,
        content: Bytes,
    ) -> Result<ChatMediaStoragePart, ChatMediaStorageError> {
        self.multipart_part_puts.fetch_add(1, Ordering::SeqCst);
        self.multipart_parts
            .lock()
            .unwrap()
            .insert(part_number, content.clone());
        Ok(ChatMediaStoragePart {
            part_number,
            size_bytes: content.len() as i64,
            etag: format!("etag-{part_number}-{}", content.len()),
        })
    }

    async fn complete_multipart_upload(
        &self,
        object_key: &str,
        content_type: &str,
        _storage_upload_id: &str,
        expected_size_bytes: i64,
        parts: &[ChatMediaStoragePart],
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        let stored_parts = self.multipart_parts.lock().unwrap();
        let mut bytes = Vec::new();
        for part in parts {
            bytes.extend_from_slice(
                stored_parts
                    .get(&part.part_number)
                    .ok_or(ChatMediaStorageError::ObjectNotFound)?,
            );
        }
        if bytes.len() as i64 != expected_size_bytes {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        let metadata = ChatMediaStorageObject {
            size_bytes: expected_size_bytes,
            content_type: Some(content_type.to_string()),
            etag: Some("multipart-etag".to_string()),
        };
        self.objects.lock().unwrap().insert(
            object_key.to_string(),
            ChatMediaStoredContent {
                bytes: Bytes::from(bytes),
                content_type: Some(content_type.to_string()),
                etag: metadata.etag.clone(),
            },
        );
        *self.metadata.lock().unwrap() = Some(metadata.clone());
        Ok(metadata)
    }

    async fn abort_multipart_upload(
        &self,
        _object_key: &str,
        _storage_upload_id: &str,
    ) -> Result<(), ChatMediaStorageError> {
        self.multipart_aborted.store(true, Ordering::SeqCst);
        self.multipart_parts.lock().unwrap().clear();
        Ok(())
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

    async fn delete_object(&self, object_key: &str) -> Result<(), ChatMediaStorageError> {
        self.deleted.store(true, Ordering::SeqCst);
        self.objects.lock().unwrap().remove(object_key);
        Ok(())
    }

    async fn read_object(
        &self,
        object_key: &str,
    ) -> Result<ChatMediaStoredContent, ChatMediaStorageError> {
        self.objects
            .lock()
            .unwrap()
            .get(object_key)
            .cloned()
            .ok_or(ChatMediaStorageError::ObjectNotFound)
    }

    async fn put_private_object(
        &self,
        object_key: &str,
        content_type: &str,
        content: Bytes,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        self.objects.lock().unwrap().insert(
            object_key.to_string(),
            ChatMediaStoredContent {
                bytes: content.clone(),
                content_type: Some(content_type.to_string()),
                etag: None,
            },
        );
        Ok(ChatMediaStorageObject {
            size_bytes: content.len() as i64,
            content_type: Some(content_type.to_string()),
            etag: None,
        })
    }

    async fn prepare_download(
        &self,
        _object_key: &str,
    ) -> Result<ChatMediaStorageDownload, ChatMediaStorageError> {
        Ok(ChatMediaStorageDownload::LocalProxy)
    }
}

struct StaticProcessor {
    result: Result<ChatMediaProcessedContent, ChatMediaProcessingError>,
}

#[async_trait]
impl ChatMediaProcessor for StaticProcessor {
    async fn process(
        &self,
        _media: &ChatMediaUploadRecord,
        _source: Bytes,
    ) -> Result<ChatMediaProcessedContent, ChatMediaProcessingError> {
        self.result.clone()
    }
}

struct OversizedFileProcessor;

#[async_trait]
impl ChatMediaProcessor for OversizedFileProcessor {
    async fn process(
        &self,
        _media: &ChatMediaUploadRecord,
        _source: Bytes,
    ) -> Result<ChatMediaProcessedContent, ChatMediaProcessingError> {
        Err(ChatMediaProcessingError::ProcessingFailed)
    }

    async fn process_file(
        &self,
        _media: &ChatMediaUploadRecord,
        _source_path: &Path,
        content_path: &Path,
        thumbnail_path: &Path,
    ) -> Result<ChatMediaProcessedFiles, ChatMediaProcessingError> {
        std::fs::File::create(content_path)
            .and_then(|file| file.set_len((MAX_CHAT_PROCESSED_VIDEO_SIZE_BYTES + 1) as u64))
            .map_err(|_| ChatMediaProcessingError::ProcessingFailed)?;
        std::fs::write(thumbnail_path, b"thumbnail")
            .map_err(|_| ChatMediaProcessingError::ProcessingFailed)?;
        Ok(ChatMediaProcessedFiles {
            content_path: content_path.to_path_buf(),
            content_type: "video/mp4".to_string(),
            thumbnail_path: thumbnail_path.to_path_buf(),
            thumbnail_content_type: "image/jpeg".to_string(),
            width_pixels: 1920,
            height_pixels: 1080,
            duration_ms: Some(1_000),
            frame_rate_milli: Some(60_000),
            video_codec: Some("h264".to_string()),
            audio_codec: Some("aac".to_string()),
        })
    }
}
