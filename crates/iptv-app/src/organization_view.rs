//! Channel and provider-group organization, including hidden-item recovery.
use crate::list_rows::{self, Mark, ROW};
use crate::scale::Scale;
use crate::wizard_view::entry::{self, TextSheet};
use crate::{Ui, data, ui_kit};
use iptv_core::organization::{ACTIONS, Action as Edit, Editor, Item};
use iptv_nav::Action;
use iptv_providers::{ContentId, GroupId};
use iptv_store_sqlite::{OrderScope, SourceId, StoreError};
use nsh_core::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Channels,
    Groups,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    source: SourceId,
    kind: Kind,
    editor: Editor,
}
fn items(source: SourceId, kind: Kind) -> Result<Vec<Item>, StoreError> {
    let org = data::organization::read(source)?;
    Ok(match kind {
        Kind::Channels => org.channels,
        Kind::Groups => org.groups,
    }
    .into_iter()
    .map(|(id, name, hidden)| Item { id, name, hidden })
    .collect())
}
pub fn open(ui: Ui, source: SourceId, kind: Kind, focus: Option<String>) {
    match items(source, kind) {
        Ok(items) => ui.organization.set(Some(Open {
            source,
            kind,
            editor: Editor::new(items, focus),
        })),
        Err(e) => log::warn!("iptv organization: {e}"),
    }
}
pub fn is_open(ui: Ui) -> bool {
    ui.organization.with(Option::is_some)
}
fn state<R>(ui: Ui, f: impl FnOnce(&Open) -> R) -> Option<R> {
    ui.organization.with(|s| s.as_ref().map(f))
}
fn update(ui: Ui, f: impl FnOnce(&mut Open)) {
    ui.organization.update(|s| {
        if let Some(s) = s {
            f(s)
        }
    })
}
pub fn back(ui: Ui) -> bool {
    let mut close = false;
    update(ui, |s| close = !s.editor.back());
    dismiss_keyboard();
    if close {
        ui.organization.set(None);
    }
    true
}
fn close(ui: Ui) {
    ui.organization.set(None);
    dismiss_keyboard();
}
pub fn key(ui: Ui, action: Action) {
    match action {
        Action::MoveUp => update(ui, |s| s.editor.step(-1)),
        Action::MoveDown => update(ui, |s| s.editor.step(1)),
        Action::Back => {
            back(ui);
        }
        Action::Select => {
            if state(ui, |s| s.editor.text.is_some()).unwrap_or(false) {
                commit(ui);
                return;
            }
            if let Some(edit) = state(ui, |s| s.editor.action).flatten() {
                perform(ui, edit)
            } else {
                update(ui, |s| {
                    if let Some(id) = s.editor.focus.clone() {
                        s.editor.open_actions(id)
                    }
                })
            }
        }
        _ => {}
    }
}
fn perform(ui: Ui, edit: Edit) {
    let Some(s) = state(ui, Clone::clone) else {
        return;
    };
    let Some(item) = s.editor.focused().cloned() else {
        return;
    };
    if edit == Edit::Rename {
        update(ui, |s| s.editor.text = Some(item.name));
        return;
    }
    if edit == Edit::Select {
        update(ui, |s| s.editor.toggle_selection());
        return;
    }
    let result = (|| -> Result<(), StoreError> {
        let mut store = data::open_store()?;
        let id = ContentId::from_stored(&item.id);
        let group = GroupId::from_stored(&item.id);
        let at = s
            .editor
            .items
            .iter()
            .position(|i| i.id == item.id)
            .unwrap_or(0);
        let to = match edit {
            Edit::Earlier => at.saturating_sub(1),
            Edit::Later => at + 1,
            Edit::First => 0,
            Edit::Last => s.editor.items.len().saturating_sub(1),
            _ => at,
        };
        match edit {
            Edit::Visibility => match s.kind {
                Kind::Channels => store.set_hidden(s.source, &id, !item.hidden)?,
                Kind::Groups => store.hide_group(s.source, &group, !item.hidden)?,
            },
            Edit::ResetName => match s.kind {
                Kind::Channels => store.rename_channel(s.source, &id, None)?,
                Kind::Groups => store.rename_group(s.source, &group, None)?,
            },
            Edit::Earlier | Edit::Later | Edit::First | Edit::Last => match s.kind {
                Kind::Channels => {
                    store.move_channel(s.source, &OrderScope::All, &id, to)?;
                }
                Kind::Groups => store.move_group(s.source, &group, to)?,
            },
            Edit::ResetOrder => match s.kind {
                Kind::Channels => store.reset_manual_order(s.source, &OrderScope::All)?,
                Kind::Groups => store.reset_group_order(s.source)?,
            },
            Edit::HideSelected | Edit::ShowSelected | Edit::HideAll | Edit::ShowAll => {
                let all = matches!(edit, Edit::HideAll | Edit::ShowAll);
                let hidden = matches!(edit, Edit::HideSelected | Edit::HideAll);
                let ids: Vec<_> = s
                    .editor
                    .items
                    .iter()
                    .filter(|i| all || s.editor.selected.contains(&i.id))
                    .map(|i| i.id.clone())
                    .collect();
                store.set_library_visibility(s.source, &ids, s.kind == Kind::Groups, hidden)?;
            }
            _ => {}
        }
        Ok(())
    })();
    finish(ui, &s, result);
}
fn finish(ui: Ui, s: &Open, result: Result<(), StoreError>) {
    if let Err(e) = result {
        log::warn!("iptv organization: change refused: {e}");
        return;
    }
    if let Ok(items) = items(s.source, s.kind) {
        update(ui, |s| {
            s.editor.reload(items);
            s.editor.action = None;
            s.editor.text = None;
        });
    }
    data::organization::changed(ui, s.source);
}
fn commit(ui: Ui) {
    let Some(s) = state(ui, Clone::clone) else {
        return;
    };
    let Some((item, text)) = s.editor.focused().zip(s.editor.text.as_deref()) else {
        return;
    };
    let text = (!text.trim().is_empty()).then_some(text.trim());
    let result = data::open_store().and_then(|mut store| match s.kind {
        Kind::Channels => store.rename_channel(s.source, &ContentId::from_stored(&item.id), text),
        Kind::Groups => store.rename_group(s.source, &GroupId::from_stored(&item.id), text),
    });
    dismiss_keyboard();
    finish(ui, &s, result);
}
fn text_value(ui: Ui) -> String {
    state(ui, |s| s.editor.text.clone())
        .flatten()
        .unwrap_or_default()
}
fn set_text(ui: Ui, text: &str) {
    update(ui, |s| s.editor.text = Some(text.to_owned()))
}
fn cancel(ui: Ui) {
    update(ui, |s| s.editor.text = None);
    dismiss_keyboard();
}
fn label(edit: Edit, hidden: bool) -> &'static str {
    match edit {
        Edit::Visibility => {
            if hidden {
                "Show item"
            } else {
                "Hide item"
            }
        }
        Edit::Rename => "Rename",
        Edit::ResetName => "Use provider name",
        Edit::Earlier => "Move earlier",
        Edit::Later => "Move later",
        Edit::First => "Move to first",
        Edit::Last => "Move to last",
        Edit::ResetOrder => "Restore provider order",
        Edit::Select => "Select / deselect for bulk visibility",
        Edit::HideSelected => "Hide selected",
        Edit::ShowSelected => "Show selected",
        Edit::HideAll => "Hide all",
        Edit::ShowAll => "Show all",
    }
}
pub fn layer(ui: Ui) -> impl IntoElement {
    let size = Scale::new();
    effect(move || {
        ui.library.version.get();
        if let Some(s) = untracked(|| state(ui, Clone::clone)) {
            untracked(|| {
                if let Ok(items) = items(s.source, s.kind) {
                    update(ui, |s| s.editor.reload(items));
                }
            })
        }
    });
    when(
        move || is_open(ui),
        move || {
            let rows = element_ref();
            effect(move || {
                if let Some(at) = state(ui, |s| {
                    s.editor
                        .items
                        .iter()
                        .position(|i| Some(&i.id) == s.editor.focus.as_ref())
                })
                .flatten()
                {
                    rows.scroll_to_index(at as u32, percentage(0.5), px(0.0), false);
                }
            });
            ui_kit::screen(ui,size).test_id("organization-screen")
            .child(ui_kit::header(ui,size,move||if state(ui,|s|s.kind)==Some(Kind::Groups){"Organize groups"}else{"Organize channels"},close))
            .child(ui_kit::caption(size,"Includes hidden items. OK opens actions; select items for bulk hide/show. Back returns."))
            .child(when(move||state(ui,|s|s.editor.items.is_empty()).unwrap_or(true),move||ui_kit::body(size,"This playlist has no items to organize.")))
            .child(virtual_list(move||state(ui,|s|s.editor.items.len()).unwrap_or(0),move|at|{
                let item=move||state(ui,|s|s.editor.items.get(at.index()).cloned()).flatten();
                let mark=move||Mark::of(ui.keys_shown()&&state(ui,|s|s.editor.focus.as_ref()==item().as_ref().map(|i|&i.id)).unwrap_or(false),false);
                list_rows::row(size,ROW,mark).test_id(&format!("organization-row-{}",at.index())).flex_row().items_center()
                    .on_press(move||{ui.touched();if let Some(i)=item(){update(ui,|s|s.editor.open_actions(i.id));}})
                    .child(ui_kit::label(size,move||item().map(|i|i.name).unwrap_or_default()).flex_1().text_color(move||mark().text()))
                    .child(ui_kit::detail(size,move||item().map(|i|{
                        let selected=state(ui,|s|s.editor.selected.contains(&i.id)).unwrap_or(false);
                        format!("{}{}",if selected{"Selected · "}else{""},if i.hidden{"Hidden"}else{"Visible"})
                    }).unwrap_or_default()).text_color(move||mark().secondary()))
            }).estimated_item_size(list_rows::pitch(size,ROW)).element_ref(&rows).flex_1())
            .child(when(move||state(ui,|s|s.editor.action.is_some()).unwrap_or(false),move||{
                view().absolute().inset_0().items_center().justify_center()
                    .child(view().absolute().inset_0().bg(crate::tint(0,160)).on_press(move||{back(ui);}))
                    .child(ui_kit::card(ui,size,620.0).child(ui_kit::title(size,move||state(ui,|s|s.editor.focused().map(|i|i.name.clone())).flatten().unwrap_or_default()))
                        .child({
                            let rows=element_ref();
                            effect(move||{if let Some(edit)=state(ui,|s|s.editor.action).flatten(){
                                if let Some(at)=ACTIONS.iter().position(|a|*a==edit){rows.scroll_to_index(at as u32,percentage(0.5),px(0.0),false);}
                            }});
                            virtual_list(move||ACTIONS.len(),move|at|{
                                let edit=ACTIONS[at.index()];
                                ui_kit::action(ui,size,
                                    move||label(edit,state(ui,|s|s.editor.focused().is_some_and(|i|i.hidden)).unwrap_or(false)),
                                    move||state(ui,|s|s.editor.action)==Some(Some(edit)),0xFFFFFF,move||perform(ui,edit))
                            }).element_ref(&rows).estimated_item_size(list_rows::pitch(size,ROW)).h(move||size.px(420.0))
                        }))
            }))
            .child(when(move||state(ui,|s|s.editor.text.is_some()).unwrap_or(false),move||entry::text_sheet(ui,TextSheet{
                label:"Display name",placeholder:"Empty restores the provider name",url:false,masked:false,text:text_value,set_text,commit,cancel,
            })))
        },
    )
}
