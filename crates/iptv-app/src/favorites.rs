//! Shell entry points for favourite actions; state and persistence live in Runtime.
use crate::Ui;
use iptv_core::guide::Schedule;
use std::collections::BTreeSet;

pub use crate::runtime::favorites::*;

pub fn catalog_changed(_ui: Ui, catalog: &dyn Schedule) {
    crate::runtime::favorites::catalog_changed(catalog);
}
pub fn edit(_ui: Ui, change: impl FnOnce(&mut BTreeSet<usize>)) {
    crate::runtime::favorites::edit(change);
}
pub fn toggle(_ui: Ui, channel: usize) {
    crate::runtime::favorites::toggle(channel);
}
pub fn reorder(_ui: Ui, key: &str, index: usize) {
    crate::runtime::favorites::reorder(key, index);
}
