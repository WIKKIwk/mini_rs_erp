use std::fs;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use data_encoding::HEXLOWER;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::mpsc;

use super::chat_media_local_files::{
    assemble_multipart, content_type_path, copy_private_file, map_io_error,
    multipart_part_path, remove_if_exists, sha256_etag, temporary_path,
    write_stream,
};
use crate::core::chat_media::{
    ChatMediaByteStream, ChatMediaMultipartUpload, ChatMediaRangeRequest,
    ChatMediaStorage, ChatMediaStorageDownload, ChatMediaStorageError,
    ChatMediaStorageObject, ChatMediaStoragePart, ChatMediaStorageUpload,
    ChatMediaStoredContent, ChatMediaStoredStream, resolve_media_range,
};

#[derive(Clone)]
pub struct LocalChatMediaStorage {
    root: PathBuf,
}

impl LocalChatMediaStorage {
    pub fn from_env() -> Self {
        let root = std::env::var("MOBILE_CHAT_MEDIA_STORE_DIR")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "data/chat_media".to_string());
        Self::new(root)
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for_key(&self, object_key: &str) -> Result<PathBuf, ChatMediaStorageError> {
        let mut path = self.root.clone();
        for part in object_key.split('/') {
            if part.is_empty()
                || part == "."
                || part == ".."
                || part.contains('\0')
                || part.contains('\\')
            {
                return Err(ChatMediaStorageError::InvalidObjectKey);
            }
            path.push(part);
        }
        Ok(path)
    }

    fn multipart_path(
        &self,
        storage_upload_id: &str,
    ) -> Result<PathBuf, ChatMediaStorageError> {
        if storage_upload_id.is_empty()
            || storage_upload_id.len() > 128
            || !storage_upload_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(ChatMediaStorageError::InvalidObjectKey);
        }
        Ok(self.root.join(".multipart").join(storage_upload_id))
    }

    fn verified_multipart_path(
        &self,
        object_key: &str,
        storage_upload_id: &str,
    ) -> Result<PathBuf, ChatMediaStorageError> {
        self.path_for_key(object_key)?;
        let path = self.multipart_path(storage_upload_id)?;
        let stored_key = fs::read_to_string(path.join("object-key"))
            .map_err(map_io_error)?;
        if stored_key != object_key {
            return Err(ChatMediaStorageError::InvalidObjectKey);
        }
        Ok(path)
    }
}

#[async_trait]
impl ChatMediaStorage for LocalChatMediaStorage {
    async fn prepare_upload(
        &self,
        object_key: &str,
        _content_type: &str,
        expected_size_bytes: i64,
    ) -> Result<ChatMediaStorageUpload, ChatMediaStorageError> {
        self.path_for_key(object_key)?;
        if expected_size_bytes <= 0 {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        Ok(ChatMediaStorageUpload::LocalProxy)
    }

    async fn put_object(
        &self,
        object_key: &str,
        content_type: &str,
        expected_size_bytes: i64,
        mut stream: ChatMediaByteStream,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        if expected_size_bytes <= 0 {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        let path = self.path_for_key(object_key)?;
        let content_type = content_type.trim().to_string();
        let (sender, receiver) = mpsc::channel(4);
        let writer = tokio::task::spawn_blocking(move || {
            write_stream(path, content_type, expected_size_bytes, receiver)
        });

        while let Some(chunk) =
            std::future::poll_fn(|context| stream.as_mut().poll_next(context)).await
        {
            let chunk = chunk?;
            if sender.send(chunk).await.is_err() {
                break;
            }
        }
        drop(sender);
        writer
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?
    }

    async fn begin_multipart_upload(
        &self,
        object_key: &str,
        content_type: &str,
    ) -> Result<ChatMediaMultipartUpload, ChatMediaStorageError> {
        self.path_for_key(object_key)?;
        if content_type.trim().is_empty() {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        let id = format!("local_{}", HEXLOWER.encode(&rand::random::<[u8; 16]>()));
        let path = self.multipart_path(&id)?;
        tokio::fs::create_dir_all(&path)
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        if tokio::fs::write(path.join("object-key"), object_key)
            .await
            .is_err()
            || tokio::fs::write(path.join("content-type"), content_type.trim())
                .await
                .is_err()
        {
            let _ = tokio::fs::remove_dir_all(&path).await;
            return Err(ChatMediaStorageError::OperationFailed);
        }
        Ok(ChatMediaMultipartUpload {
            storage_upload_id: id,
        })
    }

    async fn put_multipart_part(
        &self,
        object_key: &str,
        storage_upload_id: &str,
        part_number: i32,
        content: bytes::Bytes,
    ) -> Result<ChatMediaStoragePart, ChatMediaStorageError> {
        if part_number <= 0 || content.is_empty() {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        let path = self.verified_multipart_path(object_key, storage_upload_id)?;
        let part_path = multipart_part_path(&path, part_number);
        let temporary_path = temporary_path(&part_path);
        tokio::fs::write(&temporary_path, &content)
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        tokio::fs::rename(&temporary_path, &part_path)
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        Ok(ChatMediaStoragePart {
            part_number,
            size_bytes: i64::try_from(content.len())
                .map_err(|_| ChatMediaStorageError::SizeMismatch)?,
            etag: sha256_etag(&content),
        })
    }

    async fn complete_multipart_upload(
        &self,
        object_key: &str,
        content_type: &str,
        storage_upload_id: &str,
        expected_size_bytes: i64,
        parts: &[ChatMediaStoragePart],
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        let multipart_path = self.verified_multipart_path(object_key, storage_upload_id)?;
        let object_path = self.path_for_key(object_key)?;
        let parts = parts.to_vec();
        let content_type = content_type.trim().to_string();
        tokio::task::spawn_blocking(move || {
            assemble_multipart(
                multipart_path,
                object_path,
                content_type,
                expected_size_bytes,
                &parts,
            )
        })
        .await
        .map_err(|_| ChatMediaStorageError::OperationFailed)?
    }

    async fn abort_multipart_upload(
        &self,
        object_key: &str,
        storage_upload_id: &str,
    ) -> Result<(), ChatMediaStorageError> {
        let path = match self.verified_multipart_path(object_key, storage_upload_id) {
            Ok(path) => path,
            Err(ChatMediaStorageError::ObjectNotFound) => return Ok(()),
            Err(error) => return Err(error),
        };
        match tokio::fs::remove_dir_all(path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(ChatMediaStorageError::OperationFailed),
        }
    }

    async fn object_metadata(
        &self,
        object_key: &str,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        let path = self.path_for_key(object_key)?;
        let metadata = tokio::fs::metadata(&path).await.map_err(map_io_error)?;
        let size_bytes = i64::try_from(metadata.len())
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        let content_type = tokio::fs::read_to_string(content_type_path(&path))
            .await
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Ok(ChatMediaStorageObject {
            size_bytes,
            content_type,
            etag: None,
        })
    }

    async fn delete_object(&self, object_key: &str) -> Result<(), ChatMediaStorageError> {
        let path = self.path_for_key(object_key)?;
        remove_if_exists(&path).await?;
        remove_if_exists(&content_type_path(&path)).await
    }

    async fn read_object(
        &self,
        object_key: &str,
    ) -> Result<ChatMediaStoredContent, ChatMediaStorageError> {
        let path = self.path_for_key(object_key)?;
        let bytes = tokio::fs::read(&path).await.map_err(map_io_error)?;
        let content_type = tokio::fs::read_to_string(content_type_path(&path))
            .await
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Ok(ChatMediaStoredContent {
            bytes: bytes.into(),
            content_type,
            etag: None,
        })
    }

    async fn stream_object(
        &self,
        object_key: &str,
        range: ChatMediaRangeRequest,
    ) -> Result<ChatMediaStoredStream, ChatMediaStorageError> {
        let path = self.path_for_key(object_key)?;
        let metadata = tokio::fs::metadata(&path).await.map_err(map_io_error)?;
        let total_size_bytes = i64::try_from(metadata.len())
            .map_err(|_| ChatMediaStorageError::SizeMismatch)?;
        let (start_byte, end_byte_inclusive, partial) =
            resolve_media_range(range, total_size_bytes)?;
        let content_length = end_byte_inclusive - start_byte + 1;
        let mut file = tokio::fs::File::open(&path).await.map_err(map_io_error)?;
        file.seek(SeekFrom::Start(
            u64::try_from(start_byte).map_err(|_| ChatMediaStorageError::SizeMismatch)?,
        ))
        .await
        .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        let content_type = tokio::fs::read_to_string(content_type_path(&path))
            .await
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let stream = Box::pin(async_stream::stream! {
            let mut reader = file.take(content_length as u64);
            let mut buffer = vec![0_u8; 64 * 1024];
            loop {
                match reader.read(&mut buffer).await {
                    Ok(0) => break,
                    Ok(read) => yield Ok(bytes::Bytes::copy_from_slice(&buffer[..read])),
                    Err(_) => {
                        yield Err(ChatMediaStorageError::OperationFailed);
                        break;
                    }
                }
            }
        });
        Ok(ChatMediaStoredStream {
            stream,
            content_type,
            etag: None,
            total_size_bytes,
            start_byte,
            end_byte_inclusive,
            partial,
        })
    }

    async fn download_object_to_file(
        &self,
        object_key: &str,
        destination: &Path,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        let source = self.path_for_key(object_key)?;
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        }
        tokio::fs::copy(&source, destination)
            .await
            .map_err(map_io_error)?;
        self.object_metadata(object_key).await
    }

    async fn put_private_object(
        &self,
        object_key: &str,
        content_type: &str,
        content: bytes::Bytes,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        let size = i64::try_from(content.len())
            .map_err(|_| ChatMediaStorageError::SizeMismatch)?;
        let stream = Box::pin(async_stream::stream! {
            yield Ok(content);
        });
        self.put_object(object_key, content_type, size, stream).await
    }

    async fn put_private_file(
        &self,
        object_key: &str,
        content_type: &str,
        source: &Path,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        let destination = self.path_for_key(object_key)?;
        let source = source.to_path_buf();
        let content_type = content_type.trim().to_string();
        tokio::task::spawn_blocking(move || copy_private_file(&source, &destination, &content_type))
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?
    }

    async fn prepare_download(
        &self,
        object_key: &str,
    ) -> Result<ChatMediaStorageDownload, ChatMediaStorageError> {
        self.path_for_key(object_key)?;
        Ok(ChatMediaStorageDownload::LocalProxy)
    }
}
