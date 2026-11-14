//! Local playlist/guide files (`file://` URLs and absolute paths).

use std::path::PathBuf;

use iptv_import::{FetchError, FetchErrorKind};

/// The local path a request URL names, if it names one.
pub(crate) fn local_path(url: &str) -> Option<Result<PathBuf, FetchError>> {
    let url = url.trim();
    if url
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
    {
        return Some(
            url::Url::parse(url)
                .ok()
                .and_then(|u| u.to_file_path().ok())
                .ok_or_else(|| {
                    FetchError::new(FetchErrorKind::InvalidRequest, "not a local file URL")
                }),
        );
    }
    (url.starts_with('/') || std::path::Path::new(url).is_absolute())
        .then(|| Ok(PathBuf::from(url)))
}

pub(crate) fn io_error(e: &std::io::Error) -> FetchError {
    // The kind only: the message may repeat the path.
    FetchError::new(FetchErrorKind::Io, e.kind().to_string())
}

pub(crate) async fn open(path: &PathBuf) -> Result<(tokio::fs::File, Option<u64>), FetchError> {
    let meta = tokio::fs::metadata(path).await.map_err(|e| io_error(&e))?;
    if !meta.is_file() {
        return Err(FetchError::new(FetchErrorKind::Io, "not a regular file"));
    }
    open_regular(path).await
}

/// Opens `path` and checks what was opened, not what the name pointed to a
/// moment earlier.
///
/// The name can be replaced between the check in [`open`] and this call.
/// Opening a FIFO for reading waits for a writer, forever if none comes,
/// on a blocking thread no timeout can release. On Unix the file is
/// therefore opened without blocking; a regular file reads the same either
/// way, and anything else is refused by the check that follows.
async fn open_regular(path: &PathBuf) -> Result<(tokio::fs::File, Option<u64>), FetchError> {
    let mut options = tokio::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NONBLOCK);
    let file = options.open(path).await.map_err(|e| io_error(&e))?;
    let meta = file.metadata().await.map_err(|e| io_error(&e))?;
    if !meta.is_file() {
        return Err(FetchError::new(FetchErrorKind::Io, "not a regular file"));
    }
    Ok((file, Some(meta.len())))
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn a_fifo_swapped_in_after_the_check_is_refused_without_waiting_for_a_writer() {
        let dir = std::env::temp_dir().join(format!("iptv-net-fifo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("list.m3u");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        // What `open` does once its own check has passed.
        let opened = tokio::time::timeout(Duration::from_secs(5), open_regular(&fifo)).await;
        let regular = dir.join("regular.m3u");
        std::fs::write(&regular, b"#EXTM3U\n").unwrap();
        let read = open_regular(&regular).await;
        std::fs::remove_dir_all(&dir).unwrap();

        let error = opened
            .expect("the open must not wait for a writer")
            .unwrap_err();
        assert_eq!(error.kind, FetchErrorKind::Io);
        assert_eq!(error.detail, "not a regular file");
        assert_eq!(read.unwrap().1, Some(8));
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn native_absolute_paths_and_file_urls_open_the_same_local_file() {
        let expected = PathBuf::from("C:/Synthetic Data/list.m3u");
        for input in [
            "C:/Synthetic Data/list.m3u",
            "file:///C:/Synthetic%20Data/list.m3u",
        ] {
            assert_eq!(local_path(input).unwrap().unwrap(), expected);
        }
        assert!(local_path("C:relative.m3u").is_none());
        assert!(local_path("https://example.test/list.m3u").is_none());
    }
}
