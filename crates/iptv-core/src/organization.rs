//! Identity-based organization editor. Imported items are never deleted.
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub hidden: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Visibility,
    Rename,
    ResetName,
    Earlier,
    Later,
    First,
    Last,
    ResetOrder,
    Select,
    HideSelected,
    ShowSelected,
    HideAll,
    ShowAll,
}
pub const ACTIONS: &[Action] = &[
    Action::Visibility,
    Action::Rename,
    Action::ResetName,
    Action::Earlier,
    Action::Later,
    Action::First,
    Action::Last,
    Action::ResetOrder,
    Action::Select,
    Action::HideSelected,
    Action::ShowSelected,
    Action::HideAll,
    Action::ShowAll,
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Editor {
    pub items: Vec<Item>,
    pub focus: Option<String>,
    pub action: Option<Action>,
    pub text: Option<String>,
    pub selected: BTreeSet<String>,
}
impl Editor {
    pub fn new(items: Vec<Item>, preferred: Option<String>) -> Self {
        let focus = preferred
            .filter(|id| items.iter().any(|i| &i.id == id))
            .or_else(|| items.first().map(|i| i.id.clone()));
        Self {
            items,
            focus,
            action: None,
            text: None,
            selected: BTreeSet::new(),
        }
    }
    pub fn focused(&self) -> Option<&Item> {
        self.items
            .iter()
            .find(|i| Some(&i.id) == self.focus.as_ref())
    }
    pub fn reload(&mut self, items: Vec<Item>) {
        let at = self
            .items
            .iter()
            .position(|i| Some(&i.id) == self.focus.as_ref())
            .unwrap_or(0);
        self.items = items;
        if self
            .focus
            .as_ref()
            .is_none_or(|id| !self.items.iter().any(|i| &i.id == id))
        {
            self.focus = self
                .items
                .get(at.min(self.items.len().saturating_sub(1)))
                .map(|i| i.id.clone());
            self.action = None;
        }
        self.selected
            .retain(|id| self.items.iter().any(|i| &i.id == id));
    }
    pub fn step(&mut self, by: isize) {
        if self.text.is_some() {
            return;
        }
        if let Some(a) = self.action {
            let at = ACTIONS.iter().position(|v| *v == a).unwrap_or(0);
            self.action =
                Some(ACTIONS[(at as isize + by).clamp(0, ACTIONS.len() as isize - 1) as usize]);
        } else if !self.items.is_empty() {
            let at = self
                .items
                .iter()
                .position(|i| Some(&i.id) == self.focus.as_ref())
                .unwrap_or(0);
            self.focus = Some(
                self.items[(at as isize + by).clamp(0, self.items.len() as isize - 1) as usize]
                    .id
                    .clone(),
            );
        }
    }
    pub fn open_actions(&mut self, id: String) {
        if self.items.iter().any(|i| i.id == id) {
            self.focus = Some(id);
            self.action = Some(Action::Visibility);
        }
    }
    /// True when a sheet was dismissed; false when the caller should close the editor.
    pub fn back(&mut self) -> bool {
        if self.text.take().is_some() {
            return true;
        }
        self.action.take().is_some()
    }
    pub fn toggle_selection(&mut self) {
        if let Some(id) = self.focus.clone() {
            if !self.selected.remove(&id) {
                self.selected.insert(id);
            }
        }
        self.action = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn item(id: &str) -> Item {
        Item {
            id: id.into(),
            name: "Duplicate name".into(),
            hidden: false,
        }
    }
    #[test]
    fn identity_survives_reorder_and_hidden_items_are_recoverable() {
        let mut e = Editor::new(vec![item("a"), item("b")], Some("b".into()));
        e.open_actions("b".into());
        e.toggle_selection();
        let mut b = item("b");
        b.hidden = true;
        e.reload(vec![b, item("a")]);
        assert_eq!(e.focus.as_deref(), Some("b"));
        assert!(e.focused().unwrap().hidden);
        assert!(e.selected.contains("b"));
        e.reload(vec![item("a")]);
        assert_eq!(e.focus.as_deref(), Some("a"));
        assert!(e.selected.is_empty());
        e.reload(vec![]);
        e.step(1);
        assert!(e.focus.is_none());
    }
    #[test]
    fn back_cancels_rename_then_actions_then_leaves() {
        let mut e = Editor::new(vec![item("a")], None);
        e.open_actions("a".into());
        e.text = Some("new".into());
        assert!(e.back());
        assert!(e.action.is_some());
        assert!(e.back());
        assert!(!e.back());
    }
}
