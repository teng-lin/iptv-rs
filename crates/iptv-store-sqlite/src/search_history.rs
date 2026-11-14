//! The search screen's recent searches (`search_history`, schema v9),
//! newest first. Older builds kept them in a plaintext file; the app
//! imports that file once and deletes it.

use rusqlite::params;

use crate::Store;
use crate::error::Result;

impl Store {
    /// The recent searches, newest first.
    pub fn search_history(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT query FROM search_history ORDER BY position")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Replaces the recent searches with `queries`, newest first, in one
    /// transaction.
    pub fn set_search_history(&mut self, queries: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM search_history", [])?;
        {
            let mut insert =
                tx.prepare_cached("INSERT INTO search_history (position, query) VALUES (?1, ?2)")?;
            for (position, query) in queries.iter().enumerate() {
                insert.execute(params![position as i64, query])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;

    #[test]
    fn recent_searches_are_replaced_whole_and_keep_their_order() {
        let mut store = Store::open_in_memory().unwrap();
        assert!(store.search_history().unwrap().is_empty());
        let first = ["synthetic b".to_owned(), "synthetic a".to_owned()];
        store.set_search_history(&first).unwrap();
        assert_eq!(store.search_history().unwrap(), first);
        let next = ["synthetic c".to_owned()];
        store.set_search_history(&next).unwrap();
        assert_eq!(store.search_history().unwrap(), next);
        store.set_search_history(&[]).unwrap();
        assert!(store.search_history().unwrap().is_empty());
    }
}
