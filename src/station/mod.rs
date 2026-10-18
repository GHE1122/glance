//! The station: a shelf to put things on for a while (files, pictures, text, links) and take them off again into
//! another program, and what was copied lately (see docs/station-and-notes.md).

pub mod clipboard;
pub mod drag;
pub mod drop;
pub mod folder;
pub mod keep;
pub mod run;
pub mod shelf;
pub mod thumbs;
#[cfg(test)]
mod spike;

/// A press on what a station's lane shows (see `view::Hit`), in the panel and in a widget alike: done; whether it was
/// one. A thing on it, or copied lately, is put on the clipboard; its head's word clears it.
pub fn pressed(hit: crate::ui::view::Hit) -> bool {
    use crate::ui::view::Hit;
    match hit {
        Hit::Shelf(id) | Hit::Copied(id) => run::ask(run::Ask::Copy(id)),
        Hit::ShelfClear => run::ask(run::Ask::Clear),
        _ => return false,
    }
    true
}

/// What a thing on the station's menu does: put it on the clipboard, open it, show it in Explorer, pin it (or not),
/// take it off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Copy,
    Open,
    Show,
    Pin,
    Remove,
}

/// The menu of the thing `id` on the station (a right click on it), at `at` on the screen (`over_the_panel`: as
/// `Menu::open_at` says): up, with what its rows do.
pub fn menu(id: u64, at: windows::Win32::Foundation::POINT, over_the_panel: bool) -> Option<(crate::ui::menu::Menu, Vec<Choice>)> {
    use crate::ui::icons::Icon;
    use crate::ui::menu::{Item, Menu};
    use shelf::What;
    let shown = run::shown();
    let item = shown.items.iter().find(|item| item.id == id)?;
    let lang = crate::update::language();
    let (opens, in_explorer) = match &item.what {
        What::Text { .. } => (false, false),
        What::Link { .. } => (true, false),
        What::Files { .. } | What::Picture { .. } | What::Saved { .. } => (true, true),
    };
    let mut rows: Vec<(Choice, &'static str, Icon, bool)> = vec![(Choice::Copy, lang.pick("复制", "Copy"), Icon::Clipboard, false)];
    if opens {
        rows.push((Choice::Open, lang.pick("打开", "Open"), Icon::File, false));
    }
    if in_explorer {
        rows.push((Choice::Show, lang.pick("在资源管理器中显示", "Show in Explorer"), Icon::Panel, false));
    }
    rows.push((Choice::Pin, if item.pinned { lang.pick("取消固定", "Unpin") } else { lang.pick("固定", "Pin") }, Icon::Pin, false));
    rows.push((Choice::Remove, lang.pick("移除", "Remove"), Icon::Close, true));
    let items = rows.iter().map(|(_, label, icon, rule_before)| Item { label, icon: Some(*icon), checked: false, rule_before: *rule_before }).collect();
    let menu = Menu::open_at(items, crate::os::apps_dark(), at, false, over_the_panel)?;
    Some((menu, rows.into_iter().map(|(choice, ..)| choice).collect()))
}

/// `choice` made in the menu of the thing `id` on the station (see `menu`): done, on the station's thread.
pub fn chose(id: u64, choice: Choice) {
    let pinned = run::shown().items.iter().find(|item| item.id == id).is_some_and(|item| item.pinned);
    run::ask(match choice {
        Choice::Copy => run::Ask::Copy(id),
        Choice::Open => run::Ask::Open(id),
        Choice::Show => run::Ask::Show(id),
        Choice::Pin => run::Ask::Pin(id, !pinned),
        Choice::Remove => run::Ask::Remove(id),
    });
}

/// Now, in ms since 1970 (as the shelf keeps when things were put on it).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
