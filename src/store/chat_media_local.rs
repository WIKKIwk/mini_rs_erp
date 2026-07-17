use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::core::chat_media::{
    ChatMediaByteStream, ChatMediaStorage, ChatMediaStorageDownload, ChatMediaStorageError,
    ChatMediaStorageObject, ChatMediaStorageUpload, ChatMediaStoredContent,
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

    async fn prepare_download(
        &self,
        object_key: &str,
    ) -> Result<ChatMediaStorageDownload, ChatMediaStorageError> {
        self.path_for_key(object_key)?;
        Ok(ChatMediaStorageDownload::LocalProxy)
    }
}

fn write_stream(
    path: PathBuf,
    content_type: String,
    expected_size_bytes: i64,
    mut receiver: mpsc::Receiver<bytes::Bytes>,
) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
    let parent = path
        .parent()
        .ok_or(ChatMediaStorageError::InvalidObjectKey)?;
    fs::create_dir_all(parent).map_err(|_| ChatMediaStorageError::OperationFailed)?;
    let temporary_path = temporary_path(&path);
    let _ = fs::remove_file(&temporary_path);
    let mut file = File::create(&temporary_path)
        .map_err(|_| ChatMediaStorageError::OperationFailed)?;
    let mut written = 0_i64;
    while let Some(chunk) = receiver.blocking_recv() {
        written = written
            .checked_add(
                i64::try_from(chunk.len())
                    .map_err(|_| ChatMediaStorageError::SizeMismatch)?,
            )
            .ok_or(ChatMediaStorageError::SizeMismatch)?;
        if written > expected_size_bytes {
            let _ = fs::remove_file(&temporary_path);
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        file.write_all(&chunk)
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
    }
    if written != expected_size_bytes {
        let _ = fs::remove_file(&temporary_path);
        return Err(ChatMediaStorageError::SizeMismatch);
    }
    file.flush()
        .and_then(|_| file.sync_all())
        .map_err(|_| ChatMediaStorageError::OperationFailed)?;
    drop(file);
    fs::rename(&temporary_path, &path).map_err(|_| ChatMediaStorageError::OperationFailed)?;
    if fs::write(content_type_path(&path), &content_type).is_err() {
        let _ = fs::remove_file(&path);
        return Err(ChatMediaStorageError::OperationFailed);
    }
    Ok(ChatMediaStorageObject {
        size_bytes: written,
        content_type: Some(content_type),
        etag: None,
    })
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".part");
    PathBuf::from(value)
}

fn content_type_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".content-type");
    PathBuf::from(value)
}

async fn remove_if_exists(path: &Path) -> Result<(), ChatMediaStorageError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ChatMediaStorageError::OperationFailed),
    }
}

fn map_io_error(error: std::io::Error) -> ChatMediaStorageError {
    if error.kind() == std::io::ErrorKind::NotFound {
        ChatMediaStorageError::ObjectNotFound
    } else {
        ChatMediaStorageError::OperationFailed
    }
}
