//! Cooperative cancellation without a runtime.
//!
//! [`CancelToken::cancel`] stops an import at the next check: before each
//! request, between body chunks, and *while a request is in flight* (the
//! request future is dropped, which aborts it in reqwest). Dropping the
//! import future also cancels it.

use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use crate::error::ImportError;

#[derive(Default, Debug)]
struct Inner {
    cancelled: AtomicBool,
    wakers: Mutex<Vec<Waker>>,
}

/// Shared cancellation flag. Clone it into the UI; call [`CancelToken::cancel`].
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<Inner>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation and wakes any import waiting on the network.
    pub fn cancel(&self) {
        if !self.0.cancelled.swap(true, Ordering::SeqCst) {
            let wakers = std::mem::take(&mut *self.lock());
            for w in wakers {
                w.wake();
            }
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    /// Resolves once [`CancelToken::cancel`] is called.
    pub fn cancelled(&self) -> Cancelled<'_> {
        Cancelled(self)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Waker>> {
        self.0.wakers.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Future returned by [`CancelToken::cancelled`].
#[derive(Debug)]
pub struct Cancelled<'a>(&'a CancelToken);

impl Future for Cancelled<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let token = self.0;
        if token.is_cancelled() {
            return Poll::Ready(());
        }
        let mut wakers = token.lock();
        // Re-check under the lock: `cancel` drains the list after setting the flag.
        if token.is_cancelled() {
            return Poll::Ready(());
        }
        if !wakers.iter().any(|w| w.will_wake(cx.waker())) {
            // Bounded: one task normally polls with the same waker throughout.
            if wakers.len() >= 32 {
                wakers.remove(0);
            }
            wakers.push(cx.waker().clone());
        }
        Poll::Pending
    }
}

/// Runs `fut` unless cancellation comes first.
pub(crate) async fn or_cancel<T>(
    cancel: &CancelToken,
    fut: impl Future<Output = T>,
) -> Result<T, ImportError> {
    if cancel.is_cancelled() {
        return Err(ImportError::Cancelled);
    }
    let mut fut = pin!(fut);
    let mut stop = pin!(cancel.cancelled());
    poll_fn(|cx| {
        if let Poll::Ready(v) = fut.as_mut().poll(cx) {
            return Poll::Ready(Ok(v));
        }
        if stop.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(ImportError::Cancelled));
        }
        Poll::Pending
    })
    .await
}

/// `Err(Cancelled)` once cancellation was requested.
pub(crate) fn check(cancel: &CancelToken) -> Result<(), ImportError> {
    if cancel.is_cancelled() {
        Err(ImportError::Cancelled)
    } else {
        Ok(())
    }
}
