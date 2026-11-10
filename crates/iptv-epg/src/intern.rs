//! String interner: one arena per EPG source, each distinct string stored
//! once and referred to by a `u32`. Symbol 0 is the empty string ("none").

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

pub(crate) type Sym = u32;
pub(crate) const NONE: Sym = 0;
const END: u32 = u32::MAX;

#[derive(Default)]
pub(crate) struct Interner {
    data: String,
    spans: Vec<(u32, u32)>,
    heads: HashMap<u64, u32, BuildHasherDefault<PassThrough>>,
    /// Collision chain: next symbol with the same hash.
    next: Vec<u32>,
}

impl Interner {
    pub(crate) fn new() -> Self {
        let mut i = Interner::default();
        i.spans.push((0, 0));
        i.next.push(END);
        i
    }

    pub(crate) fn intern(&mut self, s: &str) -> Sym {
        if s.is_empty() {
            return NONE;
        }
        let h = hash(s);
        let mut at = self.heads.get(&h).copied().unwrap_or(END);
        let head = at;
        while at != END {
            if self.get(at) == s {
                return at;
            }
            at = self.next[at as usize];
        }
        let sym = self.spans.len() as u32;
        self.spans.push((self.data.len() as u32, s.len() as u32));
        self.data.push_str(s);
        self.next.push(head);
        self.heads.insert(h, sym);
        sym
    }

    pub(crate) fn get(&self, sym: Sym) -> &str {
        let (off, len) = self.spans[sym as usize];
        &self.data[off as usize..(off + len) as usize]
    }

    pub(crate) fn opt(&self, sym: Sym) -> Option<&str> {
        (sym != NONE).then(|| self.get(sym))
    }

    pub(crate) fn shrink(&mut self) {
        self.data.shrink_to_fit();
        self.spans.shrink_to_fit();
        self.next.shrink_to_fit();
        self.heads.shrink_to_fit();
    }

    pub(crate) fn len(&self) -> usize {
        self.spans.len() - 1
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.data.capacity()
            + self.spans.capacity() * 8
            + self.next.capacity() * 4
            + self.heads.capacity() * 13
    }
}

/// Fx-style hash: fast and deterministic (no random state, same on wasm).
fn hash(s: &str) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let mut h = s.len() as u64;
    for chunk in s.as_bytes().chunks(8) {
        let mut w = [0u8; 8];
        w[..chunk.len()].copy_from_slice(chunk);
        h = (h.rotate_left(5) ^ u64::from_le_bytes(w)).wrapping_mul(K);
    }
    h
}

/// Keys are already hashes.
#[derive(Default)]
struct PassThrough(u64);

impl Hasher for PassThrough {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 << 8) | u64::from(b);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedups_and_round_trips() {
        let mut i = Interner::new();
        assert_eq!(i.intern(""), NONE);
        let a = i.intern("News");
        let b = i.intern("Film");
        assert_ne!(a, b);
        assert_eq!(i.intern("News"), a);
        assert_eq!(i.get(a), "News");
        assert_eq!(i.opt(NONE), None);
        assert_eq!(i.len(), 2);
        for n in 0..10_000 {
            let s = format!("title {n}");
            let sym = i.intern(&s);
            assert_eq!(i.get(sym), s);
        }
        assert_eq!(i.intern("title 42"), i.intern("title 42"));
        assert_eq!(i.len(), 10_002);
    }
}
