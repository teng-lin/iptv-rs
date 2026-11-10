//! Recent searches, most recent first.

use crate::text::fold;

/// Recent queries, most recent first, at most `capacity`. Queries that fold
/// to the same text (case, diacritics, punctuation, spacing) are one entry,
/// worded as last typed. Queries with no letters or digits are not kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHistory {
    entries: Vec<String>,
    capacity: usize,
}

impl Default for SearchHistory {
    fn default() -> Self {
        SearchHistory::new(SearchHistory::DEFAULT_CAPACITY)
    }
}

impl SearchHistory {
    pub const DEFAULT_CAPACITY: usize = 20;

    pub fn new(capacity: usize) -> Self {
        SearchHistory {
            entries: Vec::new(),
            capacity,
        }
    }

    /// Restores a saved list (most recent first), dropping blanks,
    /// duplicates and entries beyond `capacity`.
    pub fn from_list<S: AsRef<str>>(list: impl IntoIterator<Item = S>, capacity: usize) -> Self {
        let mut h = SearchHistory::new(capacity);
        for q in list {
            let q = q.as_ref().trim();
            let key = fold(q);
            if h.entries.len() < capacity
                && !key.is_empty()
                && !h.entries.iter().any(|e| fold(e) == key)
            {
                h.entries.push(q.to_owned());
            }
        }
        h
    }

    /// The list to save, most recent first.
    pub fn to_list(&self) -> Vec<String> {
        self.entries.clone()
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Records a search; returns whether it was kept.
    pub fn push(&mut self, query: &str) -> bool {
        let query = query.trim();
        let key = fold(query);
        if key.is_empty() || self.capacity == 0 {
            return false;
        }
        self.entries.retain(|e| fold(e) != key);
        self.entries.insert(0, query.to_owned());
        self.entries.truncate(self.capacity);
        true
    }

    /// Forgets one query (and any wording that folds the same).
    pub fn remove(&mut self, query: &str) {
        let key = fold(query);
        self.entries.retain(|e| fold(e) != key);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
        self.entries.truncate(capacity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn most_recent_first_and_deduplicated() {
        let mut h = SearchHistory::default();
        assert_eq!(h.capacity(), 20);
        assert!(h.push("Night Owl"));
        assert!(h.push("orchard"));
        assert!(h.push("  NIGHT-owl "));
        assert_eq!(h.entries(), ["NIGHT-owl", "orchard"]);
        assert!(!h.push(" ?! "));
        assert_eq!(h.len(), 2);
        h.remove("night owl");
        assert_eq!(h.entries(), ["orchard"]);
        h.clear();
        assert!(h.is_empty());
    }

    #[test]
    fn capacity_drops_the_oldest() {
        let mut h = SearchHistory::new(3);
        for q in ["a1", "b2", "c3", "d4"] {
            h.push(q);
        }
        assert_eq!(h.entries(), ["d4", "c3", "b2"]);
        h.push("b2");
        assert_eq!(h.entries(), ["b2", "d4", "c3"]);
        h.set_capacity(1);
        assert_eq!(h.entries(), ["b2"]);
        let mut none = SearchHistory::new(0);
        assert!(!none.push("x"));
    }

    #[test]
    fn round_trips_through_a_string_list() {
        let mut h = SearchHistory::new(4);
        for q in ["Écho", "kestrel", "Orchard News"] {
            h.push(q);
        }
        let saved = h.to_list();
        assert_eq!(saved, ["Orchard News", "kestrel", "Écho"]);
        assert_eq!(SearchHistory::from_list(&saved, 4), h);
        // Hand-edited or older lists are cleaned on load.
        let loaded = SearchHistory::from_list(["one", " ", "ONE", "two", "three"], 2);
        assert_eq!(loaded.entries(), ["one", "two"]);
    }
}
