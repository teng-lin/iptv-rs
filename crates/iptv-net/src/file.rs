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
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|e| io_error(&e))?;
    let meta = file.metadata().await.map_err(|e| io_error(&e))?;
    if !meta.is_file() {
        return Err(FetchError::new(FetchErrorKind::Io, "not a regular file"));
    }
    Ok((file, Some(meta.len())))
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
