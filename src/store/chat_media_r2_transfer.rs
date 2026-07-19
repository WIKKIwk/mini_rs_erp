use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use bytes::Bytes;
use tokio::sync::mpsc;

use crate::core::chat_media::{ChatMediaByteStream, ChatMediaStorageError, ChatMediaStoragePart};

pub(super) fn validate_multipart_parts(
    parts: &[ChatMediaStoragePart],
    expected_size_bytes: i64,
) -> Result<(), ChatMediaStorageError> {
    if parts.is_empty() || parts.len() > 10_000 || expected_size_bytes <= 0 {
        return Err(ChatMediaStorageError::SizeMismatch);
    }
    let mut total = 0_i64;
    for (index, part) in parts.iter().enumerate() {
        let expected_part =
            i32::try_from(index + 1).map_err(|_| ChatMediaStorageError::SizeMismatch)?;
        if part.part_number != expected_part || part.size_bytes <= 0 || part.etag.trim().is_empty()
        {
            return Err(ChatMediaStorageError::SizeMismatch);
        }
        total = total
            .checked_add(part.size_bytes)
            .ok_or(ChatMediaStorageError::SizeMismatch)?;
    }
    if total == expected_size_bytes {
        Ok(())
    } else {
        Err(ChatMediaStorageError::SizeMismatch)
    }
}

pub(super) fn complete_multipart_xml(parts: &[ChatMediaStoragePart]) -> String {
    let mut value = String::from("<CompleteMultipartUpload>");
    for part in parts {
        value.push_str("<Part><PartNumber>");
        value.push_str(&part.part_number.to_string());
        value.push_str("</PartNumber><ETag>&quot;");
        value.push_str(&xml_escape(&part.etag));
        value.push_str("&quot;</ETag></Part>");
    }
    value.push_str("</CompleteMultipartUpload>");
    value
}

pub(super) fn xml_tag<'a>(value: &'a str, tag: &str) -> Option<&'a str> {
    let start_tag = format!("<{tag}>");
    let end_tag = format!("</{tag}>");
    let start = value.find(&start_tag)? + start_tag.len();
    let end = value[start..].find(&end_tag)? + start;
    Some(value[start..end].trim())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub(super) fn xml_unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub(super) fn file_stream(path: PathBuf) -> ChatMediaByteStream {
    let (sender, mut receiver) = mpsc::channel(4);
    tokio::task::spawn_blocking(move || {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(_) => {
                let _ = sender.blocking_send(Err(ChatMediaStorageError::OperationFailed));
                return;
            }
        };
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if sender
                        .blocking_send(Ok(Bytes::copy_from_slice(&buffer[..read])))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => {
                    let _ = sender.blocking_send(Err(ChatMediaStorageError::OperationFailed));
                    break;
                }
            }
        }
    });
    Box::pin(async_stream::stream! {
        while let Some(chunk) = receiver.recv().await {
            yield chunk;
        }
    })
}

pub(super) fn write_download(
    destination: PathBuf,
    mut receiver: mpsc::Receiver<Result<Bytes, ChatMediaStorageError>>,
) -> Result<i64, ChatMediaStorageError> {
    let parent = destination
        .parent()
        .ok_or(ChatMediaStorageError::InvalidObjectKey)?;
    std::fs::create_dir_all(parent).map_err(|_| ChatMediaStorageError::OperationFailed)?;
    let temporary = temporary_path(&destination);
    let _ = std::fs::remove_file(&temporary);
    let mut file = File::create(&temporary).map_err(|_| ChatMediaStorageError::OperationFailed)?;
    let mut written = 0_i64;
    while let Some(chunk) = receiver.blocking_recv() {
        let chunk = chunk?;
        file.write_all(&chunk)
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        written = written
            .checked_add(
                i64::try_from(chunk.len()).map_err(|_| ChatMediaStorageError::SizeMismatch)?,
            )
            .ok_or(ChatMediaStorageError::SizeMismatch)?;
    }
    file.flush()
        .and_then(|_| file.sync_all())
        .map_err(|_| ChatMediaStorageError::OperationFailed)?;
    drop(file);
    std::fs::rename(&temporary, &destination)
        .map_err(|_| ChatMediaStorageError::OperationFailed)?;
    Ok(written)
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".part");
    PathBuf::from(value)
}

pub(super) fn env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{complete_multipart_xml, validate_multipart_parts, xml_tag, xml_unescape};
    use crate::core::chat_media::ChatMediaStoragePart;

    #[test]
    fn multipart_completion_is_ordered_and_size_checked() {
        let parts = vec![
            ChatMediaStoragePart {
                part_number: 1,
                size_bytes: 5,
                etag: "first".into(),
            },
            ChatMediaStoragePart {
                part_number: 2,
                size_bytes: 2,
                etag: "second".into(),
            },
        ];
        assert!(validate_multipart_parts(&parts, 7).is_ok());
        assert!(validate_multipart_parts(&parts, 8).is_err());
        let xml = complete_multipart_xml(&parts);
        assert!(xml.contains("<PartNumber>1</PartNumber>"));
        assert!(xml.contains("&quot;first&quot;"));
    }

    #[test]
    fn upload_id_is_read_from_s3_xml() {
        let value = "<InitiateMultipartUploadResult><UploadId>a&amp;b</UploadId></InitiateMultipartUploadResult>";
        assert_eq!(xml_unescape(xml_tag(value, "UploadId").unwrap()), "a&b");
    }
}
