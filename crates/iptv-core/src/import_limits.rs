//! Resource budgets for untrusted imports. Pure counters; adapters supply time.
use std::{
    fmt,
    io::{self, Write},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    InputBytes,
    DecodedBytes,
    RecordBytes,
    Items,
    Elapsed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitExceeded {
    pub kind: LimitKind,
    pub maximum: u64,
}
impl fmt::Display for LimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "import {:?} limit exceeded ({})",
            self.kind, self.maximum
        )
    }
}
impl std::error::Error for LimitExceeded {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportLimits {
    pub input_bytes: u64,
    pub decoded_bytes: u64,
    pub record_bytes: usize,
    pub items: u64,
    pub elapsed: Duration,
}
impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            input_bytes: 512 << 20,
            decoded_bytes: 1 << 30,
            record_bytes: 1 << 20,
            items: 10_000_000,
            elapsed: Duration::from_secs(300),
        }
    }
}
impl LimitExceeded {
    pub fn check(kind: LimitKind, actual: u64, maximum: u64) -> Result<(), Self> {
        if actual > maximum {
            Err(Self { kind, maximum })
        } else {
            Ok(())
        }
    }
}

/// An in-memory decoder target. Reject before growing the allocation, including
/// when a single compressed chunk expands beyond the remaining total budget.
pub struct DecodeBuffer {
    bytes: Vec<u8>,
    total: u64,
    limit: u64,
    exceeded: bool,
}
impl DecodeBuffer {
    pub fn new(limit: u64) -> Self {
        Self {
            bytes: Vec::new(),
            total: 0,
            limit,
            exceeded: false,
        }
    }
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }
    pub fn limit_error(&self) -> Option<LimitExceeded> {
        self.exceeded.then_some(LimitExceeded {
            kind: LimitKind::DecodedBytes,
            maximum: self.limit,
        })
    }
}
impl Write for DecodeBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.total.saturating_add(bytes.len() as u64) > self.limit {
            self.exceeded = true;
            return Err(io::Error::other("decoded byte budget exceeded"));
        }
        self.total += bytes.len() as u64;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
