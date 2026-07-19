use std::fs::{self, File};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use data_encoding::HEXLOWER;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use crate::core::chat_media::{
    ChatMediaStorageError, ChatMediaStorageObject, ChatMediaStoragePart,
};

pub(super) fn write_stream(
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
    let mut file =
        File::create(&temporary_path).map_err(|_| ChatMediaStorageError::OperationFailed)?;
    let mut written = 0_i64;
    while let Some(chunk) = receiver.blocking_recv() {
        written = written
            .checked_add(
                i64::try_from(chunk.len()).map_err(|_| ChatMediaStorageError::SizeMismatch)?,
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

pub(super) fn assemble_multipart(
    multipart_path: PathBuf,
    object_path: PathBuf,
    content_type: String,
    expected_size_bytes: i64,
    parts: &[ChatMediaStoragePart],
) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
    if expected_size_bytes <= 0 || parts.is_empty() || content_type.is_empty() {
        return Err(ChatMediaStorageError::SizeMismatch);
    }
    let parent = object_path
        .parent()
        .ok_or(ChatMediaStorageError::InvalidObjectKey)?;
    fs::create_dir_all(parent).map_err(|_| ChatMediaStorageError::OperationFailed)?;
    let temporary = temporary_path(&object_path);
    let _ = fs::remove_file(&temporary);
    let mut destination =
        File::create(&temporary).map_err(|_| ChatMediaStorageError::OperationFailed)?;
    let mut total = 0_i64;
    for (index, part) in parts.iter().enumerate() {
        let expected_part =
            i32::try_from(index + 1).map_err(|_| ChatMediaStorageError::SizeMismatch)?;
        if part.part_number != expected_part || part.size_bytes <= 0 {
            let _ = fs::remove_file(&temporary);
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        let path = multipart_part_path(&multipart_path, part.part_number);
        let mut source = File::open(&path).map_err(map_io_error)?;
        let metadata_size = i64::try_from(
            source
                .metadata()
                .map_err(|_| ChatMediaStorageError::OperationFailed)?
                .len(),
        )
        .map_err(|_| ChatMediaStorageError::SizeMismatch)?;
        if metadata_size != part.size_bytes || file_etag(&mut source)? != part.etag {
            let _ = fs::remove_file(&temporary);
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        source
            .rewind()
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        std::io::copy(&mut source, &mut destination)
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        total = total
            .checked_add(part.size_bytes)
            .ok_or(ChatMediaStorageError::SizeMismatch)?;
    }
    if total != expected_size_bytes {
        let _ = fs::remove_file(&temporary);
        return Err(ChatMediaStorageError::SizeMismatch);
    }
    destination
        .flush()
        .and_then(|_| destination.sync_all())
        .map_err(|_| ChatMediaStorageError::OperationFailed)?;
    drop(destination);
    fs::rename(&temporary, &object_path).map_err(|_| ChatMediaStorageError::OperationFailed)?;
    if fs::write(content_type_path(&object_path), &content_type).is_err() {
        let _ = fs::remove_file(&object_path);
        return Err(ChatMediaStorageError::OperationFailed);
    }
    fs::remove_dir_all(multipart_path).map_err(|_| ChatMediaStorageError::OperationFailed)?;
    Ok(ChatMediaStorageObject {
        size_bytes: total,
        content_type: Some(content_type),
        etag: Some(format!("local-multipart-{total}-{}", parts.len())),
    })
}

pub(super) fn copy_private_file(
    source: &Path,
    destination: &Path,
    content_type: &str,
) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
    let size = i64::try_from(fs::metadata(source).map_err(map_io_error)?.len())
        .map_err(|_| ChatMediaStorageError::SizeMismatch)?;
    if size <= 0 || content_type.is_empty() {
        return Err(ChatMediaStorageError::SizeMismatch);
    }
    let parent = destination
        .parent()
        .ok_or(ChatMediaStorageError::InvalidObjectKey)?;
    fs::create_dir_all(parent).map_err(|_| ChatMediaStorageError::OperationFailed)?;
    let temporary = temporary_path(destination);
    let _ = fs::remove_file(&temporary);
    fs::copy(source, &temporary).map_err(|_| ChatMediaStorageError::OperationFailed)?;
    fs::rename(&temporary, destination).map_err(|_| ChatMediaStorageError::OperationFailed)?;
    if fs::write(content_type_path(destination), content_type).is_err() {
        let _ = fs::remove_file(destination);
        return Err(ChatMediaStorageError::OperationFailed);
    }
    Ok(ChatMediaStorageObject {
        size_bytes: size,
        content_type: Some(content_type.to_string()),
        etag: None,
    })
}

pub(super) fn multipart_part_path(path: &Path, part_number: i32) -> PathBuf {
    path.join(format!("part-{part_number:05}"))
}

pub(super) fn sha256_etag(content: &[u8]) -> String {
    HEXLOWER.encode(&Sha256::digest(content))
}

fn file_etag(file: &mut File) -> Result<String, ChatMediaStorageError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(HEXLOWER.encode(&hasher.finalize()))
}

pub(super) fn temporary_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".part");
    PathBuf::from(value)
}

pub(super) fn content_type_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".content-type");
    PathBuf::from(value)
}

pub(super) async fn remove_if_exists(path: &Path) -> Result<(), ChatMediaStorageError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ChatMediaStorageError::OperationFailed),
    }
}

pub(super) fn map_io_error(error: std::io::Error) -> ChatMediaStorageError {
    if error.kind() == std::io::ErrorKind::NotFound {
        ChatMediaStorageError::ObjectNotFound
    } else {
        ChatMediaStorageError::OperationFailed
    }
}
