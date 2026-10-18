//! The station: a shelf to put things on for a while (files, pictures, text, links) and take them off again into
//! another program, and what was copied lately (see docs/station-and-notes.md).

pub mod catch;
pub mod clipboard;
pub mod drag;
pub mod drop;
pub mod folder;
pub mod keep;
pub mod recent;
pub mod run;
pub mod shelf;
pub mod thumbs;
#[cfg(test)]
mod spike;

/// A press on what a station's lane shows (see `view::Hit`), in the panel and in a widget alike: done; whether it was
/// one. A thing on it, or copied lately, is put on the clipboard (pressed again within a double click's time, a thing
/// on it is opened).
pub fn pressed(hit: crate::ui::view::Hit) -> bool {
    use crate::ui::view::Hit;
    /// The thing on it last pressed, and when.
    static LAST: std::sync::Mutex<Option<(u64, std::time::Instant)>> = std::sync::Mutex::new(None);
    match hit {
        Hit::Shelf(id) => {
            let double = std::time::Duration::from_millis(u64::from(unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() }));
            let mut last = LAST.lock().unwrap();
            let again = last.is_some_and(|(was, at)| was == id && at.elapsed() <= double);
            *last = (!again).then(|| (id, std::time::Instant::now()));
            run::ask(if again { run::Ask::Open(id) } else { run::Ask::Copy(id) });
        }
        Hit::Copied(id) => run::ask(run::Ask::Copy(id)),
        // A file come lately: put on the clipboard as a file (pressed again within a double click's time, opened).
        Hit::Recent(file) => {
            let double = std::time::Duration::from_millis(u64::from(unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() }));
            static LAST_FILE: std::sync::Mutex<Option<(u128, std::time::Instant)>> = std::sync::Mutex::new(None);
            let mut last = LAST_FILE.lock().unwrap();
            let again = last.is_some_and(|(was, at)| was == file && at.elapsed() <= double);
            *last = (!again).then(|| (file, std::time::Instant::now()));
            if let Some(path) = recent::path_of(file) {
                run::ask(run::Ask::Files(vec![path], if again { run::FilesAct::Open } else { run::FilesAct::Copy }));
            }
        }
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

/// What a thing pressed on a station's lane and moved far enough (see `run::drag_begins`) drags off: a thing on it, or
/// copied lately, as the station gives it; a file come lately, where it is (pressed on a window of the thread
/// numbered `thread`). Whether it was one dragged.
pub fn drag_off(hit: crate::ui::view::Hit, thread: u32) -> bool {
    use crate::ui::view::Hit;
    match hit {
        Hit::Shelf(id) | Hit::Copied(id) => run::ask(run::Ask::DragOut(id, thread)),
        Hit::Recent(file) => match recent::path_of(file) {
            Some(path) => run::ask(run::Ask::DragFiles(vec![path], thread)),
            None => return false,
        },
        _ => return false,
    }
    true
}

/// What a file come lately's menu does: opens it, shows it in Explorer, puts it on the station, copies its path, lists
/// no more of its folder's files.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecentChoice {
    Open,
    Show,
    Station,
    CopyPath,
    Hide,
}

/// The menu of the file come lately numbered `file` (a right click on it), at `at` on the screen: up, with what its
/// rows do.
pub fn recent_menu(file: u128, at: windows::Win32::Foundation::POINT, over_the_panel: bool) -> Option<(crate::ui::menu::Menu, Vec<RecentChoice>)> {
    use crate::ui::icons::Icon;
    use crate::ui::menu::{Item, Menu};
    recent::path_of(file)?;
    let lang = crate::update::language();
    let rows: [(RecentChoice, &str, Icon, bool); 5] = [
        (RecentChoice::Open, lang.pick("打开", "Open"), Icon::File, false),
        (RecentChoice::Show, lang.pick("在资源管理器中显示", "Show in Explorer"), Icon::Panel, false),
        (RecentChoice::Station, lang.pick("放入中转站", "Put on the station"), Icon::Inbox, false),
        (RecentChoice::CopyPath, lang.pick("复制路径", "Copy its path"), Icon::Clipboard, false),
        (RecentChoice::Hide, lang.pick("不再列出这个文件夹", "List this folder no more"), Icon::Close, true),
    ];
    let items = rows.iter().map(|(_, label, icon, rule_before)| Item { label, icon: Some(*icon), checked: false, rule_before: *rule_before }).collect();
    let menu = Menu::open_at(items, crate::os::apps_dark(), at, false, over_the_panel)?;
    Some((menu, rows.into_iter().map(|(choice, ..)| choice).collect()))
}

/// `choice` made in the menu of the file come lately numbered `file` (see `recent_menu`): done.
pub fn recent_chose(file: u128, choice: RecentChoice) {
    let Some(path) = recent::path_of(file) else { return };
    match choice {
        RecentChoice::Open => run::ask(run::Ask::Files(vec![path], run::FilesAct::Open)),
        RecentChoice::Show => run::ask(run::Ask::Files(vec![path], run::FilesAct::Show)),
        RecentChoice::Station => run::ask(run::Ask::Take(clipboard::Content::Files(vec![path]))),
        RecentChoice::CopyPath => run::ask(run::Ask::CopyText(path.display().to_string())),
        RecentChoice::Hide => {
            if let Some(folder) = path.parent().map(std::path::Path::to_path_buf) {
                std::thread::spawn(move || crate::app().change(|settings| if !settings.recent_hidden.contains(&folder) { settings.recent_hidden.push(folder) }));
            }
        }
    }
}

/// What a row of the station's head's menus does: shows what is on it so; takes off what is not pinned, of what is on
/// it or of what was copied lately; keeps no more of what is copied; opens the settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeadChoice {
    View(crate::settings::ShelfView),
    Clear,
    ClearCopies,
    StopCopies,
    StopRecent,
    Settings,
}

/// The menu a press on the station's head opens (`hit`: the button of how what is on it shows, or of the rest, the
/// page `tab` shown), at `at` on the screen (`over_the_panel`: as `Menu::open_at` says): up, with what its rows do.
pub fn head_menu(hit: crate::ui::view::Hit, tab: crate::ui::view::Tab, at: windows::Win32::Foundation::POINT, over_the_panel: bool) -> Option<(crate::ui::menu::Menu, Vec<HeadChoice>)> {
    use crate::settings::ShelfView;
    use crate::ui::icons::Icon;
    use crate::ui::menu::{Item, Menu};
    use crate::ui::view::{Hit, Tab};
    let lang = crate::update::language();
    let view = run::shown().view;
    // Each row: what it does, its name, its icon, whether it is ticked, and whether a rule sets it off.
    let rows: Vec<(HeadChoice, &'static str, Icon, bool, bool)> = match (hit, tab) {
        (Hit::StationView, _) => [(ShelfView::Cards, lang.pick("卡片", "Cards"), Icon::LayoutGrid), (ShelfView::List, lang.pick("列表", "List"), Icon::List), (ShelfView::Pictures, lang.pick("图片", "Pictures"), Icon::Image)]
            .into_iter()
            .map(|(shows, label, icon)| (HeadChoice::View(shows), label, icon, shows == view, false))
            .collect(),
        (Hit::StationMore, Tab::Shelf) => vec![
            (HeadChoice::Clear, lang.pick("全部清空（保留固定的项目）", "Clear all but the pinned"), Icon::Close, false, false),
            (HeadChoice::Settings, lang.pick("中转站设置", "Station settings"), Icon::Settings, false, true),
        ],
        (Hit::StationMore, Tab::Recent(_)) => vec![
            (HeadChoice::StopRecent, lang.pick("不再列出最近文件", "Stop listing recent files"), Icon::Close, false, false),
            (HeadChoice::Settings, lang.pick("中转站设置", "Station settings"), Icon::Settings, false, true),
        ],
        (Hit::StationMore, Tab::Copies) => vec![
            (HeadChoice::ClearCopies, lang.pick("清空最近复制（保留固定的项目）", "Clear Recently copied but the pinned"), Icon::Close, false, false),
            (HeadChoice::StopCopies, lang.pick("不再记录复制的内容", "Stop keeping what is copied"), Icon::Clipboard, false, false),
            (HeadChoice::Settings, lang.pick("中转站设置", "Station settings"), Icon::Settings, false, true),
        ],
        _ => return None,
    };
    let items = rows.iter().map(|(_, label, icon, checked, rule_before)| Item { label, icon: Some(*icon), checked: *checked, rule_before: *rule_before }).collect();
    let menu = Menu::open_at(items, crate::os::apps_dark(), at, false, over_the_panel)?;
    Some((menu, rows.into_iter().map(|(choice, ..)| choice).collect()))
}

/// `choice` made in a menu of the station's head (see `head_menu`): done (the settings changed off the calling thread,
/// which they tell to restyle).
pub fn head_chose(choice: HeadChoice) {
    match choice {
        HeadChoice::View(view) => {
            std::thread::spawn(move || crate::app().change(|settings| settings.station.view = view));
        }
        HeadChoice::Clear => run::ask(run::Ask::Clear(false)),
        HeadChoice::ClearCopies => run::ask(run::Ask::Clear(true)),
        HeadChoice::StopCopies => {
            std::thread::spawn(|| crate::app().change(|settings| settings.station.history = false));
        }
        HeadChoice::StopRecent => {
            std::thread::spawn(|| crate::app().change(|settings| settings.station.recent = false));
        }
        HeadChoice::Settings => crate::show_settings(),
    }
}

/// Now, in ms since 1970 (as the shelf keeps when things were put on it).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
