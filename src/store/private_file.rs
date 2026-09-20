use std::io::Write;
use std::path::Path;

/// Replace a local secret-bearing file without leaving a permissive temp copy.
pub(crate) fn write_private_sync(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".credential-write-{:032x}.tmp",
        rand::random::<u128>()
    ));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub(crate) async fn write_private(path: &Path, bytes: Vec<u8>) -> std::io::Result<()> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || write_private_sync(&path, &bytes))
        .await
        .map_err(std::io::Error::other)?
}
