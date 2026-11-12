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
    url.starts_with('/').then(|| Ok(PathBuf::from(url)))
}

pub(crate) fn io_error(e: &std::io::Error) -> FetchError {
    // The kind only: the message may repeat the path.
    FetchError::new(FetchErrorKind::Io, e.kind().to_string())
}

pub(crate) async fn open(path: &PathBuf) -> Result<(tokio::fs::File, Option<u64>), FetchError> {
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|e| io_error(&e))?;
    let meta = file.metadata().await.map_err(|e| io_error(&e))?;
    if meta.is_dir() {
        return Err(FetchError::new(FetchErrorKind::Io, "is a directory"));
    }
    Ok((file, Some(meta.len())))
}
