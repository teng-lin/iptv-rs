//! Streaming catalog destination. A failed import must never publish its stage.
use crate::SinkError;
pub use iptv_providers::xtream::CatalogRow;

pub trait CatalogSink: Send {
    /// Begin a rollback boundary (an optional provider list may fail).
    fn begin_step(&mut self) -> Result<(), SinkError> {
        Ok(())
    }
    fn push(&mut self, row: CatalogRow) -> Result<(), SinkError>;
    /// Commit this list to the private stage, or discard it on failure.
    fn end_step(&mut self, success: bool) -> Result<(), SinkError>;
    fn finish(&mut self) -> Result<(), SinkError>;
}
