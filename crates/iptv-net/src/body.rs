//! Chunked response bodies (network or local file).

use iptv_import::{BodyStream, FetchError, FetchErrorKind};
use tokio::io::AsyncReadExt;

use crate::error::from_reqwest;

/// Chunk size for local files.
const FILE_CHUNK: usize = 64 * 1024;

/// A body read with [`BodyStream::next_chunk`].
pub struct NetBody {
    pub(crate) inner: Inner,
    pub(crate) remaining: u64,
    pub(crate) deadline: tokio::time::Instant,
}

pub(crate) enum Inner {
    Http(reqwest::Response),
    File(tokio::fs::File),
    Done,
}

impl std::fmt::Debug for NetBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self.inner {
            Inner::Http(_) => "http",
            Inner::File(_) => "file",
            Inner::Done => "done",
        };
        f.debug_tuple("NetBody").field(&what).finish()
    }
}

impl NetBody {
    async fn next(&mut self) -> Result<Option<Vec<u8>>, FetchError> {
        loop {
            let chunk = match &mut self.inner {
                Inner::Http(resp) => resp.chunk().await.map_err(from_reqwest)?.map(Vec::from),
                Inner::File(file) => {
                    let mut buf = vec![0; FILE_CHUNK];
                    let n = file
                        .read(&mut buf)
                        .await
                        .map_err(|e| FetchError::new(FetchErrorKind::Io, e.kind().to_string()))?;
                    buf.truncate(n);
                    (n > 0).then_some(buf)
                }
                Inner::Done => None,
            };
            match chunk {
                Some(c) if c.is_empty() => continue,
                Some(c) => return Ok(Some(c)),
                None => {
                    self.inner = Inner::Done;
                    return Ok(None);
                }
            }
        }
    }
}

impl BodyStream for NetBody {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, FetchError> {
        let chunk = tokio::time::timeout_at(self.deadline, self.next())
            .await
            .map_err(|_| {
                FetchError::new(FetchErrorKind::Deadline, "total response deadline exceeded")
            })??;
        if let Some(bytes) = &chunk {
            self.remaining = self
                .remaining
                .checked_sub(bytes.len() as u64)
                .ok_or_else(|| {
                    FetchError::new(FetchErrorKind::TooLarge, "stream byte budget exceeded")
                })?;
        }
        Ok(chunk)
    }
}
