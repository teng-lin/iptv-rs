//! Shared request plumbing: cancellation, counters, progress.

use iptv_providers::{HttpRequest, HttpResponse};

use crate::cancel::{CancelToken, check, or_cancel};
use crate::catalog::ImportStats;
use crate::error::{ImportError, Step};
use crate::fetch::{BodyStream, Fetch, StreamingResponse};
use crate::progress::{Phase, Progress, ProgressSink, REPORT_EVERY_BYTES};

pub(crate) struct Ctx<'a, F, P> {
    pub fetch: &'a F,
    pub cancel: &'a CancelToken,
    pub progress: &'a mut P,
    pub stats: ImportStats,
}

impl<'a, F: Fetch, P: ProgressSink> Ctx<'a, F, P> {
    pub fn new(fetch: &'a F, progress: &'a mut P, cancel: &'a CancelToken) -> Self {
        Self {
            fetch,
            cancel,
            progress,
            stats: ImportStats::default(),
        }
    }

    pub fn report(&mut self, progress: Progress) {
        self.progress.report(&progress);
    }

    pub fn phase(&mut self, phase: Phase) {
        self.report(Progress::phase(phase));
    }

    pub fn check(&self) -> Result<(), ImportError> {
        check(self.cancel)
    }

    /// A buffered request. Network failures carry `step`; HTTP status is
    /// left to the caller (probes treat errors as "try the next").
    pub async fn send(
        &mut self,
        step: Step,
        req: HttpRequest,
    ) -> Result<HttpResponse, ImportError> {
        let resp = or_cancel(self.cancel, self.fetch.fetch(req))
            .await?
            .map_err(|error| ImportError::Network { step, error })?;
        self.stats.requests += 1;
        self.stats.bytes += resp.body.len() as u64;
        Ok(resp)
    }

    /// A streamed request whose body goes to `sink` chunk by chunk.
    /// Non-success statuses are errors ([`ImportError::http`]).
    pub async fn stream(
        &mut self,
        step: Step,
        phase: Phase,
        req: HttpRequest,
        mut sink: impl FnMut(&[u8]) -> Result<u64, ImportError>,
    ) -> Result<u64, ImportError> {
        let resp: StreamingResponse<F::Body> = or_cancel(self.cancel, self.fetch.fetch_stream(req))
            .await?
            .map_err(|error| ImportError::Network { step, error })?;
        self.stats.requests += 1;
        if !resp.is_success() {
            return Err(ImportError::http(step, resp.status));
        }
        let mut body = resp.body;
        let mut progress = Progress::phase(phase);
        progress.total_bytes = resp.content_length;
        let mut last_report = 0;
        loop {
            let chunk = or_cancel(self.cancel, body.next_chunk())
                .await?
                .map_err(|error| ImportError::Network { step, error })?;
            let Some(chunk) = chunk else { break };
            progress.bytes += chunk.len() as u64;
            self.stats.bytes += chunk.len() as u64;
            progress.items = sink(&chunk)?;
            if progress.bytes - last_report >= REPORT_EVERY_BYTES {
                last_report = progress.bytes;
                self.report(progress.clone());
            }
        }
        self.report(progress.clone());
        Ok(progress.bytes)
    }
}
